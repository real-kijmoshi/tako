//! `tako` with no arguments: a full-screen dashboard to run, configure and
//! watch the router. It never runs inside the daemon, so it costs the daemon nothing.

mod draw;
pub(crate) mod net;

use crate::catalog::{self, Entry};
use crate::config::{self, Config};
use crate::util::e2s;
use crate::{daemon, edit, util};
use ratatui::crossterm::event::{
    self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::crossterm::execute;
use ratatui::DefaultTerminal;
use serde_json::Value;
use std::io::{Read, Seek, SeekFrom, Write};
use std::process::{Command, Stdio};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Dashboard,
    Providers,
    Combos,
    Logs,
    Connect,
    Chat,
}

impl Tab {
    pub const ALL: [Tab; 6] = [
        Tab::Dashboard,
        Tab::Providers,
        Tab::Combos,
        Tab::Logs,
        Tab::Connect,
        Tab::Chat,
    ];
    pub fn title(self) -> &'static str {
        match self {
            Tab::Dashboard => "Dashboard",
            Tab::Providers => "Providers",
            Tab::Combos => "Combos",
            Tab::Logs => "Logs",
            Tab::Connect => "Connect",
            Tab::Chat => "Chat",
        }
    }
    fn index(self) -> usize {
        Tab::ALL.iter().position(|t| *t == self).unwrap_or(0)
    }
}

#[derive(Clone)]
pub enum Row {
    Configured(String),
    Catalog(usize),
}

pub struct ModelItem {
    pub id: String, // provider/model
    pub free: bool,
    pub suggested: bool,
    pub picked: bool,
}

pub struct Picker {
    pub providers: Vec<String>,
    pub pending: usize,
    pub items: Vec<ModelItem>,
    pub filter: String,
    pub sel: usize,
    pub combos: Vec<String>,
    pub combo: usize,
    pub errors: Vec<String>,
    pub free_only: bool,
}

impl Picker {
    pub fn visible(&self) -> Vec<usize> {
        let f = self.filter.to_lowercase();
        (0..self.items.len())
            .filter(|&i| {
                let it = &self.items[i];
                (!self.free_only || it.free) && (f.is_empty() || it.id.to_lowercase().contains(&f))
            })
            .collect()
    }
    pub fn combo_name(&self) -> &str {
        &self.combos[self.combo]
    }
}

pub enum Modal {
    Key {
        entry: Option<usize>,
        provider: String,
        input: String,
        reveal: bool,
    },
    CloudflareAccount {
        entry: usize,
        key: String,
        input: String,
    },
    Models(Picker),
    Input {
        title: String,
        hint: String,
        value: String,
    },
    Confirm {
        msg: String,
        then: Then,
    },
    Help,
}

pub enum Then {
    RemoveProvider(String),
    DeleteCombo(String),
}

pub enum Msg {
    Done(Result<String, String>),
    Chat(Result<String, String>),
    Models {
        provider: String,
        result: Result<Vec<(String, bool)>, String>,
    },
}

#[derive(Default)]
pub struct Chat {
    pub input: String,
    pub messages: Vec<Value>,
    pub model: Option<String>,
    pub pending: bool,
    pub error: Option<String>,
    pub scroll: usize,
}

pub struct Toast {
    pub ok: bool,
    pub text: String,
    pub at: Instant,
}

pub struct App {
    pub tab: Tab,
    pub chat: Chat,
    click_targets: Vec<draw::ClickTarget>,
    pub cfg: Result<Config, String>,
    pub live: Option<Value>,
    pub pid: Option<i32>,
    pub autostart: bool,
    pub prov_sel: usize,
    pub combo_sel: usize,
    pub target_sel: usize,
    pub combo_right: bool,
    pub logs: Vec<String>,
    log_pos: u64,
    pub log_scroll: usize,
    pub connect_sel: usize,
    pub modal: Option<Modal>,
    pub toasts: Vec<Toast>,
    pub busy: usize,
    pub busy_label: String,
    pub tick: u64,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    rt: tokio::runtime::Runtime,
    quit: bool,
    last_poll: Instant,
}

pub fn run() -> Result<(), String> {
    config::ensure()?;
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .map_err(e2s)?;
    let (tx, rx) = channel();
    let mut app = App {
        tab: Tab::Dashboard,
        chat: Chat::default(),
        click_targets: Vec::new(),
        cfg: Err(String::new()),
        live: None,
        pid: None,
        autostart: false,
        prov_sel: 0,
        combo_sel: 0,
        target_sel: 0,
        combo_right: false,
        logs: Vec::new(),
        log_pos: 0,
        log_scroll: 0,
        connect_sel: 0,
        modal: None,
        toasts: Vec::new(),
        busy: 0,
        busy_label: String::new(),
        tick: 0,
        tx,
        rx,
        rt,
        quit: false,
        last_poll: Instant::now(),
    };
    app.refresh();
    if app.providers().is_empty() {
        app.tab = Tab::Providers;
        app.prov_sel = 0;
    }
    let mut term = ratatui::init();
    let _ = execute!(std::io::stdout(), EnableBracketedPaste, EnableMouseCapture);
    let r = app.main_loop(&mut term);
    let _ = execute!(
        std::io::stdout(),
        DisableBracketedPaste,
        DisableMouseCapture
    );
    ratatui::restore();
    r
}

impl App {
    fn main_loop(&mut self, term: &mut DefaultTerminal) -> Result<(), String> {
        while !self.quit {
            term.draw(|f| self.click_targets = draw::draw(f, self))
                .map_err(e2s)?;
            if event::poll(Duration::from_millis(80)).map_err(e2s)? {
                match event::read().map_err(e2s)? {
                    Event::Key(k) if k.kind == KeyEventKind::Press => self.key(k, term),
                    Event::Paste(s) => self.paste(&s),
                    Event::Mouse(m) => self.mouse(m, term),
                    _ => {}
                }
            }
            while let Ok(m) = self.rx.try_recv() {
                self.msg(m);
            }
            self.tick = self.tick.wrapping_add(1);
            if self.last_poll.elapsed() >= Duration::from_millis(1000) {
                self.refresh();
            }
            self.toasts
                .retain(|t| t.at.elapsed() < Duration::from_secs(5));
        }
        Ok(())
    }

    // ---- data -----------------------------------------------------------

    pub fn refresh(&mut self) {
        self.last_poll = Instant::now();
        self.cfg = config::load(&config::path());
        self.pid = daemon::running();
        self.autostart = daemon::service_installed();
        let listen = self.listen();
        self.live = self
            .pid
            .and_then(|_| util::get(&listen, "/status").ok())
            .and_then(|s| serde_json::from_str(&s).ok());
        self.read_logs();
        self.clamp();
    }

    fn read_logs(&mut self) {
        let Ok(mut f) = std::fs::File::open(daemon::log_file()) else {
            return;
        };
        let len = f.metadata().map(|m| m.len()).unwrap_or(0);
        if len < self.log_pos {
            self.log_pos = 0;
            self.logs.clear();
        }
        if len == self.log_pos {
            return;
        }
        let start = if self.log_pos == 0 {
            len.saturating_sub(128 * 1024)
        } else {
            self.log_pos
        };
        let mut buf = String::new();
        if f.seek(SeekFrom::Start(start)).is_ok() && f.read_to_string(&mut buf).is_ok() {
            let mut lines: Vec<&str> = buf.lines().collect();
            if self.log_pos == 0 && start > 0 && !lines.is_empty() {
                lines.remove(0); // partial line
            }
            let added = lines.len();
            self.logs.extend(lines.into_iter().map(String::from));
            if self.log_scroll > 0 {
                self.log_scroll += added; // keep the view still while reading history
            }
            let excess = self.logs.len().saturating_sub(3000);
            self.logs.drain(..excess);
        }
        self.log_pos = len;
    }

    pub fn listen(&self) -> String {
        self.cfg
            .as_ref()
            .map(|c| c.listen.clone())
            .unwrap_or_else(|_| config::DEFAULT_LISTEN.into())
    }

    pub fn token(&self) -> String {
        self.cfg
            .as_ref()
            .ok()
            .and_then(|c| crate::auth::local_token().or(c.api_key.clone()))
            .unwrap_or_else(|| "tako".into())
    }

    pub fn default_combo(&self) -> Option<String> {
        let c = self.cfg.as_ref().ok()?;
        c.default
            .clone()
            .or_else(|| c.combos.keys().next().cloned())
    }

    pub fn providers(&self) -> Vec<String> {
        self.cfg
            .as_ref()
            .map(|c| c.providers.keys().cloned().collect())
            .unwrap_or_default()
    }

    pub fn combos(&self) -> Vec<String> {
        self.cfg
            .as_ref()
            .map(|c| c.combos.keys().cloned().collect())
            .unwrap_or_default()
    }

    pub fn combo_targets(&self, name: &str) -> Vec<String> {
        self.cfg
            .as_ref()
            .ok()
            .and_then(|c| c.combos.get(name).cloned())
            .unwrap_or_default()
    }

    pub fn rows(&self) -> Vec<Row> {
        let have = self.providers();
        let mut v: Vec<Row> = have.iter().cloned().map(Row::Configured).collect();
        v.extend(
            catalog::ALL
                .iter()
                .enumerate()
                .filter(|(_, e)| !have.iter().any(|h| h == e.id))
                .map(|(i, _)| Row::Catalog(i)),
        );
        v
    }

    fn clamp(&mut self) {
        self.prov_sel = self.prov_sel.min(self.rows().len().saturating_sub(1));
        let combos = self.combos();
        self.combo_sel = self.combo_sel.min(combos.len().saturating_sub(1));
        let n = combos
            .get(self.combo_sel)
            .map(|c| self.combo_targets(c).len())
            .unwrap_or(0);
        self.target_sel = self.target_sel.min(n.saturating_sub(1));
        self.connect_sel = self.connect_sel.min(draw::CONNECT.len() - 1);
    }

    // ---- background work ------------------------------------------------

    fn toast(&mut self, ok: bool, text: impl Into<String>) {
        self.toasts.push(Toast {
            ok,
            text: text.into(),
            at: Instant::now(),
        });
        if self.toasts.len() > 4 {
            self.toasts.remove(0);
        }
    }

    fn job(&mut self, label: &str, f: impl FnOnce() -> Result<String, String> + Send + 'static) {
        self.busy += 1;
        self.busy_label = label.to_string();
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(Msg::Done(f()));
        });
    }

    fn job_async(
        &mut self,
        label: &str,
        f: impl std::future::Future<Output = Result<String, String>> + Send + 'static,
    ) {
        self.busy += 1;
        self.busy_label = label.to_string();
        let tx = self.tx.clone();
        self.rt.spawn(async move {
            let _ = tx.send(Msg::Done(f.await));
        });
    }

    fn msg(&mut self, m: Msg) {
        match m {
            Msg::Chat(result) => {
                self.chat.pending = false;
                match result {
                    Ok(text) => self
                        .chat
                        .messages
                        .push(serde_json::json!({"role": "assistant", "content": text})),
                    Err(error) => {
                        if let Some(message) = self.chat.messages.pop() {
                            let prompt = message["content"].as_str().unwrap_or_default();
                            if self.chat.input.is_empty() {
                                self.chat.input = prompt.to_string();
                            }
                        }
                        self.chat.error = Some(error);
                    }
                }
                self.chat.scroll = 0;
            }
            Msg::Done(r) => {
                self.busy = self.busy.saturating_sub(1);
                match r {
                    Ok(s) => self.toast(true, s),
                    Err(e) => self.toast(false, e),
                }
                self.refresh();
            }
            Msg::Models { provider, result } => {
                let Some(Modal::Models(p)) = &mut self.modal else {
                    return;
                };
                p.pending = p.pending.saturating_sub(1);
                let entry = catalog::get(&provider);
                let existing: Vec<String> = self
                    .cfg
                    .as_ref()
                    .ok()
                    .and_then(|c| c.combos.get(&p.combos[p.combo]).cloned())
                    .unwrap_or_default();
                let suggested: Vec<&str> = entry.map(|e| e.models.to_vec()).unwrap_or_default();
                let (list, live) = match result {
                    Ok(l) => (l, true),
                    Err(e) => {
                        p.errors.push(format!("{provider}: {e}"));
                        (
                            suggested.iter().map(|m| (m.to_string(), false)).collect(),
                            false,
                        )
                    }
                };
                // Suggestions go stale; only pre-pick ones the provider still lists.
                let any_sug = list.iter().any(|(id, _)| suggested.contains(&id.as_str()));
                let mut auto_free = if live && !any_sug { 2 } else { 0 };
                for (id, priced_free) in list {
                    let full = format!("{provider}/{id}");
                    if p.items.iter().any(|i| i.id == full) {
                        continue;
                    }
                    let sug = suggested.contains(&id.as_str());
                    let free = entry.is_some_and(|e| e.is_free(&id, priced_free));
                    let mut picked = sug && !existing.contains(&full);
                    if !picked && free && auto_free > 0 && !existing.contains(&full) {
                        picked = true; // no suggestion survived: start with a couple of free ones
                        auto_free -= 1;
                    }
                    p.items.push(ModelItem {
                        id: full,
                        free,
                        suggested: sug,
                        picked,
                    });
                }
                // suggested first (in catalog order), then free, then the rest
                let rank = |id: &str| {
                    let (prov, m) = id.split_once('/').unwrap_or(("", id));
                    catalog::get(prov)
                        .and_then(|e| e.models.iter().position(|s| *s == m))
                        .unwrap_or(usize::MAX)
                };
                p.items.sort_by(|a, b| {
                    (rank(&a.id), !a.free, &a.id).cmp(&(rank(&b.id), !b.free, &b.id))
                });
            }
        }
    }

    fn open_picker(&mut self, providers: Vec<String>, combo: Option<String>) {
        let Ok(cfg) = &self.cfg else { return };
        if providers.is_empty() {
            self.toast(false, "connect a provider first (tab 2)");
            return;
        }
        let mut combos: Vec<String> = cfg.combos.keys().cloned().collect();
        let want = combo
            .or_else(|| cfg.default.clone())
            .unwrap_or_else(|| "free".into());
        if !combos.contains(&want) {
            combos.insert(0, want.clone());
        }
        let idx = combos.iter().position(|c| *c == want).unwrap_or(0);
        for name in &providers {
            let Some(p) = cfg.providers.get(name) else {
                continue;
            };
            let t = net::Target::from(name, p);
            let tx = self.tx.clone();
            let provider = name.clone();
            self.rt.spawn(async move {
                let result = net::models(t).await;
                let _ = tx.send(Msg::Models { provider, result });
            });
        }
        self.modal = Some(Modal::Models(Picker {
            pending: providers.len(),
            providers,
            items: Vec::new(),
            filter: String::new(),
            sel: 0,
            combos,
            combo: idx,
            errors: Vec::new(),
            free_only: false,
        }));
    }

    fn apply_picker(&mut self, p: &Picker) -> bool {
        let combo = p.combo_name().to_string();
        let mut picked: Vec<String> = p
            .items
            .iter()
            .filter(|i| i.picked)
            .map(|i| i.id.clone())
            .collect();
        if picked.is_empty() && !p.filter.is_empty() && p.visible().is_empty() {
            let f = p.filter.trim();
            let has_prefix = f
                .split_once('/')
                .is_some_and(|(pre, _)| p.providers.iter().any(|x| x == pre));
            if has_prefix {
                picked.push(f.to_string());
            } else if p.providers.len() == 1 {
                picked.push(format!("{}/{f}", p.providers[0]));
            } else {
                self.toast(false, "type it as provider/model to add a custom model");
                return true;
            }
        }
        if picked.is_empty() {
            self.toast(false, "nothing selected: press space on a model");
            return true;
        }
        match edit::add_to_combo(&combo, &picked) {
            Ok(n) => {
                let mut msg = format!(
                    "added {n} model{} to '{combo}'",
                    if n == 1 { "" } else { "s" }
                );
                if self.cfg.as_ref().is_ok_and(|c| c.default.is_none())
                    && edit::set_default(&combo).is_ok()
                {
                    msg.push_str(", now the default");
                }
                self.toast(true, msg);
                self.refresh();
                if self.pid.is_none() {
                    self.toast(true, "press s to start tako");
                }
                if let Some(i) = self.combos().iter().position(|c| *c == combo) {
                    self.combo_sel = i;
                }
                false
            }
            Err(e) => {
                self.toast(false, e);
                true
            }
        }
    }

    fn test_provider(&mut self, name: &str) {
        let Ok(cfg) = &self.cfg else { return };
        let Some(p) = cfg.providers.get(name) else {
            return;
        };
        let prefix = format!("{name}/");
        let model = cfg
            .combos
            .values()
            .flatten()
            .find_map(|t| t.strip_prefix(&prefix).map(String::from)); // else net::test picks a listed model
        let t = net::Target::from(name, p);
        self.job_async(&format!("testing {name}"), net::test(t, model));
    }

    fn connect_entry(&mut self, i: usize) {
        let e: &Entry = &catalog::ALL[i];
        if e.needs_key() {
            self.modal = Some(Modal::Key {
                entry: Some(i),
                provider: e.id.to_string(),
                input: String::new(),
                reveal: false,
            });
            return;
        }
        match edit::add_provider(e.id, e.wire.name(), e.base_url, None, e.max_tokens) {
            Ok(()) => {
                self.toast(true, format!("added {}", e.name));
                self.refresh();
                self.open_picker(vec![e.id.to_string()], None);
            }
            Err(err) => self.toast(false, err),
        }
    }

    fn suspend(&mut self, term: &mut DefaultTerminal, f: impl FnOnce()) {
        let _ = execute!(
            std::io::stdout(),
            DisableBracketedPaste,
            DisableMouseCapture
        );
        ratatui::restore();
        f();
        *term = ratatui::init();
        let _ = execute!(std::io::stdout(), EnableBracketedPaste, EnableMouseCapture);
        let _ = term.clear();
        self.refresh();
    }

    fn launch_claude(&mut self, term: &mut DefaultTerminal) {
        let combo = self.default_combo();
        self.suspend(term, || {
            let exe = std::env::current_exe().unwrap_or_else(|_| "tako".into());
            let mut c = Command::new(exe);
            c.arg("claude");
            if let Some(m) = combo {
                c.arg(m);
            }
            let ok = c.status().map(|s| s.success()).unwrap_or(false);
            if !ok {
                print!("\n[tako] press Enter to go back ");
                let _ = std::io::stdout().flush();
                let _ = std::io::stdin().read_line(&mut String::new());
            }
        });
    }

    // ---- input ----------------------------------------------------------

    fn key(&mut self, k: KeyEvent, term: &mut DefaultTerminal) {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        if ctrl && k.code == KeyCode::Char('c') {
            self.quit = true;
            return;
        }
        if self.modal.is_some() {
            return self.modal_key(k);
        }
        if self.tab == Tab::Chat && !matches!(k.code, KeyCode::Tab | KeyCode::BackTab) {
            self.chat_key(k);
            return;
        }
        match k.code {
            KeyCode::Char('q') | KeyCode::Esc => self.quit = true,
            KeyCode::Char('?') => self.modal = Some(Modal::Help),
            KeyCode::Char(c @ '1'..='6') => self.tab = Tab::ALL[(c as u8 - b'1') as usize],
            KeyCode::Tab => self.tab = Tab::ALL[(self.tab.index() + 1) % Tab::ALL.len()],
            KeyCode::BackTab => {
                self.tab = Tab::ALL[(self.tab.index() + Tab::ALL.len() - 1) % Tab::ALL.len()]
            }
            KeyCode::Char('s') => {
                if self.pid.is_some() {
                    self.job("stopping", daemon::stop)
                } else {
                    self.job("starting", daemon::start)
                }
            }
            KeyCode::Char('R') => self.job("restarting", daemon::restart),
            KeyCode::Char('A') => {
                if self.autostart {
                    self.job("turning autostart off", daemon::disable)
                } else {
                    self.job("turning autostart on", daemon::enable)
                }
            }
            KeyCode::Char('E') => {
                let mut res = Ok(());
                self.suspend(term, || res = config::open_editor());
                let err = res.err().or_else(|| self.cfg.as_ref().err().cloned());
                match err {
                    Some(e) => self.toast(false, e),
                    None => self.toast(true, "config saved, changes are live"),
                }
            }
            _ => match self.tab {
                Tab::Dashboard | Tab::Chat => {}
                Tab::Providers => self.providers_key(k),
                Tab::Combos => self.combos_key(k),
                Tab::Logs => self.logs_key(k),
                Tab::Connect => self.connect_key(k, term),
            },
        }
    }

    pub fn chat_model(&self) -> Option<String> {
        self.chat.model.clone().or_else(|| self.default_combo())
    }

    fn chat_key(&mut self, k: KeyEvent) {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        match k.code {
            KeyCode::Esc => self.tab = Tab::Dashboard,
            KeyCode::Char('u') if ctrl => self.chat.input.clear(),
            KeyCode::Char('n') if ctrl && !self.chat.pending => {
                self.chat.messages.clear();
                self.chat.error = None;
                self.chat.scroll = 0;
            }
            KeyCode::Left | KeyCode::Right if ctrl && !self.chat.pending => {
                let models = self.combos();
                if !models.is_empty() {
                    let index = models
                        .iter()
                        .position(|m| Some(m) == self.chat_model().as_ref())
                        .unwrap_or(0);
                    let next = if k.code == KeyCode::Right {
                        index + 1
                    } else {
                        index + models.len() - 1
                    };
                    self.chat.model = Some(models[next % models.len()].clone());
                    self.chat.messages.clear();
                    self.chat.error = None;
                    self.chat.scroll = 0;
                }
            }
            KeyCode::Up | KeyCode::PageUp => self.chat.scroll = self.chat.scroll.saturating_add(5),
            KeyCode::Down | KeyCode::PageDown => {
                self.chat.scroll = self.chat.scroll.saturating_sub(5)
            }
            KeyCode::End => self.chat.scroll = 0,
            KeyCode::Backspace => {
                self.chat.input.pop();
            }
            KeyCode::Enter if k.modifiers.contains(KeyModifiers::ALT) => self.chat.input.push('\n'),
            KeyCode::Enter => self.send_chat(),
            KeyCode::Char(c) if !ctrl && !k.modifiers.contains(KeyModifiers::ALT) => {
                self.chat.input.push(c)
            }
            _ => {}
        }
    }

    fn send_chat(&mut self) {
        if self.chat.pending || self.chat.input.trim().is_empty() {
            return;
        }
        let Some(model) = self.chat_model() else {
            self.chat.error = Some("Add a combo in tab 3 before chatting.".into());
            return;
        };
        if self.pid.is_none() {
            self.chat.error = Some("Router stopped. Press Esc, then s to start tako.".into());
            return;
        }
        self.chat.model = Some(model.clone());
        let prompt = std::mem::take(&mut self.chat.input);
        self.chat
            .messages
            .push(serde_json::json!({"role": "user", "content": prompt}));
        self.chat.pending = true;
        self.chat.error = None;
        self.chat.scroll = 0;
        let messages = self.chat.messages.clone();
        let listen = self.listen();
        let token = self.token();
        let tx = self.tx.clone();
        self.rt.spawn(async move {
            let result = net::chat(listen, token, model, messages).await;
            let _ = tx.send(Msg::Chat(result));
        });
    }

    fn providers_key(&mut self, k: KeyEvent) {
        let rows = self.rows();
        let row = rows.get(self.prov_sel).cloned();
        let configured = match &row {
            Some(Row::Configured(id)) => Some(id.clone()),
            _ => None,
        };
        match k.code {
            KeyCode::Up | KeyCode::Char('k') => self.prov_sel = self.prov_sel.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') => {
                self.prov_sel = (self.prov_sel + 1).min(rows.len().saturating_sub(1))
            }
            KeyCode::Home | KeyCode::Char('g') => self.prov_sel = 0,
            KeyCode::End | KeyCode::Char('G') => self.prov_sel = rows.len().saturating_sub(1),
            KeyCode::Enter | KeyCode::Char('m') => match row {
                Some(Row::Catalog(i)) => self.connect_entry(i),
                Some(Row::Configured(id)) => self.open_picker(vec![id], None),
                None => {}
            },
            KeyCode::Char('n') => {
                if let Some(id) = configured {
                    let entry = catalog::ALL.iter().position(|e| e.id == id);
                    self.modal = Some(Modal::Key {
                        entry,
                        provider: id,
                        input: String::new(),
                        reveal: false,
                    });
                }
            }
            KeyCode::Char('t') => {
                if let Some(id) = configured {
                    self.test_provider(&id)
                }
            }
            KeyCode::Char('o') => {
                let id = match &row {
                    Some(Row::Catalog(i)) => Some(catalog::ALL[*i].id.to_string()),
                    Some(Row::Configured(id)) => Some(id.clone()),
                    None => None,
                };
                if let Some(e) = id
                    .as_deref()
                    .and_then(catalog::get)
                    .filter(|e| e.needs_key())
                {
                    self.open_page(e.key_url);
                }
            }
            KeyCode::Char('d') | KeyCode::Delete | KeyCode::Backspace => {
                if let Some(id) = configured {
                    self.modal = Some(Modal::Confirm {
                        msg: format!("Remove provider '{id}' and its models from all combos?"),
                        then: Then::RemoveProvider(id),
                    });
                }
            }
            _ => {}
        }
    }

    fn combos_key(&mut self, k: KeyEvent) {
        let combos = self.combos();
        let combo = combos.get(self.combo_sel).cloned();
        let targets = combo
            .as_deref()
            .map(|c| self.combo_targets(c))
            .unwrap_or_default();
        let shift = k.modifiers.contains(KeyModifiers::SHIFT);
        match k.code {
            KeyCode::Up | KeyCode::Down if shift && self.combo_right => self.move_target(&combo, targets, k.code == KeyCode::Up),
            KeyCode::Char('K') | KeyCode::Char('J') if self.combo_right => self.move_target(&combo, targets, k.code == KeyCode::Char('K')),
            KeyCode::Up | KeyCode::Char('k') => {
                if self.combo_right {
                    self.target_sel = self.target_sel.saturating_sub(1)
                } else {
                    self.combo_sel = self.combo_sel.saturating_sub(1);
                    self.target_sel = 0;
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if self.combo_right {
                    self.target_sel = (self.target_sel + 1).min(targets.len().saturating_sub(1))
                } else {
                    self.combo_sel = (self.combo_sel + 1).min(combos.len().saturating_sub(1));
                    self.target_sel = 0;
                }
            }
            KeyCode::Left | KeyCode::Char('h') => self.combo_right = false,
            KeyCode::Right | KeyCode::Char('l') | KeyCode::Enter => {
                if !targets.is_empty() {
                    self.combo_right = true
                }
            }
            KeyCode::Char('n') => {
                self.modal = Some(Modal::Input {
                    title: "New combo".into(),
                    hint: "A combo is a model name that tries its models in order. Name it, then pick models.".into(),
                    value: String::new(),
                })
            }
            KeyCode::Char('a') => {
                let p = self.providers();
                self.open_picker(p, combo)
            }
            KeyCode::Char('*') | KeyCode::Char(' ') => {
                if let Some(c) = combo {
                    match edit::set_default(&c) {
                        Ok(()) => self.toast(true, format!("'{c}' is now the default: unknown model names go here")),
                        Err(e) => self.toast(false, e),
                    }
                    self.refresh();
                }
            }
            KeyCode::Char('d') | KeyCode::Char('x') | KeyCode::Delete | KeyCode::Backspace => {
                let Some(c) = combo else { return };
                if self.combo_right {
                    let mut t = targets;
                    if self.target_sel < t.len() {
                        let gone = t.remove(self.target_sel);
                        match edit::set_combo(&c, &t) {
                            Ok(()) => self.toast(true, format!("removed {gone} from '{c}'")),
                            Err(e) => self.toast(false, e),
                        }
                        if t.is_empty() {
                            self.combo_right = false;
                        }
                        self.refresh();
                    }
                } else {
                    self.modal = Some(Modal::Confirm { msg: format!("Delete combo '{c}'?"), then: Then::DeleteCombo(c) });
                }
            }
            _ => {}
        }
    }

    fn move_target(&mut self, combo: &Option<String>, mut t: Vec<String>, up: bool) {
        let Some(c) = combo else { return };
        let i = self.target_sel;
        let j = if up {
            i.checked_sub(1)
        } else {
            Some(i + 1).filter(|j| *j < t.len())
        };
        let Some(j) = j else { return };
        t.swap(i, j);
        match edit::set_combo(c, &t) {
            Ok(()) => self.target_sel = j,
            Err(e) => self.toast(false, e),
        }
        self.refresh();
    }

    fn logs_key(&mut self, k: KeyEvent) {
        let max = self.logs.len();
        match k.code {
            KeyCode::Up | KeyCode::Char('k') => self.log_scroll = (self.log_scroll + 1).min(max),
            KeyCode::Down | KeyCode::Char('j') => {
                self.log_scroll = self.log_scroll.saturating_sub(1)
            }
            KeyCode::PageUp => self.log_scroll = (self.log_scroll + 20).min(max),
            KeyCode::PageDown => self.log_scroll = self.log_scroll.saturating_sub(20),
            KeyCode::Home | KeyCode::Char('g') => self.log_scroll = max,
            KeyCode::End | KeyCode::Char('G') | KeyCode::Char('f') => self.log_scroll = 0,
            _ => {}
        }
    }

    fn connect_key(&mut self, k: KeyEvent, term: &mut DefaultTerminal) {
        match k.code {
            KeyCode::Up | KeyCode::Char('k') => {
                self.connect_sel = self.connect_sel.saturating_sub(1)
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.connect_sel = (self.connect_sel + 1).min(draw::CONNECT.len() - 1)
            }
            KeyCode::Enter if self.connect_sel == 0 => self.launch_claude(term),
            KeyCode::Char('c') | KeyCode::Enter => {
                let (_, _, snippet) = draw::connect_info(self, self.connect_sel);
                match copy(&snippet) {
                    Ok(()) => self.toast(true, "copied to clipboard"),
                    Err(e) => self.toast(false, e),
                }
            }
            _ => {}
        }
    }

    fn modal_key(&mut self, k: KeyEvent) {
        let Some(mut modal) = self.modal.take() else {
            return;
        };
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        let keep = match &mut modal {
            Modal::Help => !matches!(
                k.code,
                KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q') | KeyCode::Char('?')
            ),
            Modal::Confirm { then, .. } => match k.code {
                KeyCode::Char('y') | KeyCode::Enter => {
                    let r = match then {
                        Then::RemoveProvider(id) => {
                            edit::remove_provider(id).map(|_| format!("removed {id}"))
                        }
                        Then::DeleteCombo(c) => {
                            edit::delete_combo(c).map(|_| format!("deleted combo '{c}'"))
                        }
                    };
                    match r {
                        Ok(m) => self.toast(true, m),
                        Err(e) => self.toast(false, e),
                    }
                    self.refresh();
                    false
                }
                KeyCode::Char('n') | KeyCode::Esc => false,
                _ => true,
            },
            Modal::Key {
                entry,
                provider,
                input,
                reveal,
            } => {
                match k.code {
                    KeyCode::Esc => false,
                    KeyCode::Tab => {
                        *reveal = !*reveal;
                        true
                    }
                    KeyCode::Char('o') if ctrl => {
                        if let Some(e) = entry.map(|i| &catalog::ALL[i]) {
                            self.open_page(e.key_url);
                        }
                        true
                    }
                    KeyCode::Char('u') if ctrl => {
                        input.clear();
                        true
                    }
                    KeyCode::Backspace => {
                        input.pop();
                        true
                    }
                    KeyCode::Char(c) if !ctrl => {
                        input.push(c);
                        true
                    }
                    KeyCode::Enter => {
                        let key = input.trim().to_string();
                        let provider = provider.clone();
                        if key.is_empty() {
                            self.toast(false, "paste your API key first");
                            true
                        } else {
                            let new = !self.providers().contains(&provider);
                            if new && entry.is_some_and(|i| catalog::ALL[i].id == "cloudflare") {
                                self.modal = Some(Modal::CloudflareAccount {
                                    entry: entry.unwrap(),
                                    key,
                                    input: String::new(),
                                });
                                return;
                            }
                            let r = match entry.map(|i| &catalog::ALL[i]) {
                                Some(e) if new => edit::add_provider(
                                    e.id,
                                    e.wire.name(),
                                    e.base_url,
                                    Some(&key),
                                    e.max_tokens,
                                ),
                                _ => edit::add_key(&provider, &key),
                            };
                            match r {
                                Ok(()) => {
                                    self.refresh();
                                    if new {
                                        self.toast(
                                            true,
                                            format!("connected {provider}, now pick models"),
                                        );
                                        self.open_picker(vec![provider], None);
                                    } else {
                                        self.toast(true, format!("added another key to {provider}, tako rotates them"));
                                    }
                                    false
                                }
                                Err(e) => {
                                    self.toast(false, e);
                                    true
                                }
                            }
                        }
                    }
                    _ => true,
                }
            }
            Modal::CloudflareAccount { entry, key, input } => match k.code {
                KeyCode::Esc => false,
                KeyCode::Backspace => {
                    input.pop();
                    true
                }
                KeyCode::Char('u') if ctrl => {
                    input.clear();
                    true
                }
                KeyCode::Char(c) if !ctrl && c.is_ascii_hexdigit() => {
                    input.push(c);
                    true
                }
                KeyCode::Enter => match catalog::cloudflare_url(input.trim()) {
                    Err(e) => {
                        self.toast(false, e);
                        true
                    }
                    Ok(url) => {
                        let e = &catalog::ALL[*entry];
                        match edit::add_provider(e.id, e.wire.name(), &url, Some(key), e.max_tokens)
                        {
                            Ok(()) => {
                                self.refresh();
                                self.toast(true, "connected Cloudflare, now pick models");
                                self.open_picker(vec![e.id.to_string()], None);
                                false
                            }
                            Err(e) => {
                                self.toast(false, e);
                                true
                            }
                        }
                    }
                },
                _ => true,
            },
            Modal::Input { value, .. } => match k.code {
                KeyCode::Esc => false,
                KeyCode::Backspace => {
                    value.pop();
                    true
                }
                KeyCode::Char(c) if c.is_ascii_alphanumeric() || c == '-' || c == '_' => {
                    value.push(c);
                    true
                }
                KeyCode::Enter => {
                    let name = value.trim().to_string();
                    if name.is_empty() {
                        true
                    } else if self.combos().contains(&name) {
                        self.toast(false, format!("'{name}' already exists"));
                        true
                    } else {
                        let p = self.providers();
                        self.open_picker(p, Some(name));
                        false
                    }
                }
                _ => true,
            },
            Modal::Models(p) => {
                let vis = p.visible();
                match k.code {
                    KeyCode::Esc => false,
                    KeyCode::Up => {
                        p.sel = p.sel.saturating_sub(1);
                        true
                    }
                    KeyCode::Down => {
                        p.sel = (p.sel + 1).min(vis.len().saturating_sub(1));
                        true
                    }
                    KeyCode::PageUp => {
                        p.sel = p.sel.saturating_sub(10);
                        true
                    }
                    KeyCode::PageDown => {
                        p.sel = (p.sel + 10).min(vis.len().saturating_sub(1));
                        true
                    }
                    KeyCode::Left => {
                        p.combo = (p.combo + p.combos.len() - 1) % p.combos.len();
                        true
                    }
                    KeyCode::Right => {
                        p.combo = (p.combo + 1) % p.combos.len();
                        true
                    }
                    KeyCode::Tab => {
                        p.free_only = !p.free_only;
                        p.sel = 0;
                        true
                    }
                    KeyCode::Char(' ') => {
                        if let Some(&i) = vis.get(p.sel) {
                            p.items[i].picked = !p.items[i].picked;
                            p.sel = (p.sel + 1).min(vis.len().saturating_sub(1));
                        }
                        true
                    }
                    KeyCode::Char('a') if ctrl => {
                        let all = vis.iter().all(|&i| p.items[i].picked);
                        vis.iter().for_each(|&i| p.items[i].picked = !all);
                        true
                    }
                    KeyCode::Char('u') if ctrl => {
                        p.filter.clear();
                        p.sel = 0;
                        true
                    }
                    KeyCode::Backspace => {
                        p.filter.pop();
                        p.sel = 0;
                        true
                    }
                    KeyCode::Char(c) if !ctrl => {
                        p.filter.push(c);
                        p.sel = 0;
                        true
                    }
                    KeyCode::Enter => self.apply_picker(p),
                    _ => true,
                }
            }
        };
        if keep && self.modal.is_none() {
            self.modal = Some(modal);
        }
    }

    fn paste(&mut self, s: &str) {
        if self.modal.is_none() && self.tab == Tab::Chat {
            self.chat
                .input
                .push_str(&s.replace("\r\n", "\n").replace('\r', "\n"));
            return;
        }
        let s = s.trim();
        match &mut self.modal {
            Some(Modal::Key { input, .. }) | Some(Modal::CloudflareAccount { input, .. }) => {
                input.push_str(s)
            }
            Some(Modal::Models(p)) => {
                p.filter.push_str(s);
                p.sel = 0;
            }
            Some(Modal::Input { value, .. }) => value.push_str(s),
            _ => {}
        }
    }

    fn open_page(&mut self, url: &str) {
        let url = url.to_string();
        self.job("opening browser", move || {
            open_url(&url).map(|()| format!("opened {url}"))
        });
    }

    fn mouse(&mut self, m: MouseEvent, term: &mut DefaultTerminal) {
        let key = |c| KeyEvent::new(c, KeyModifiers::NONE);
        match m.kind {
            MouseEventKind::ScrollUp => self.key(key(KeyCode::Up), term),
            MouseEventKind::ScrollDown => self.key(key(KeyCode::Down), term),
            MouseEventKind::Down(MouseButton::Left) if m.row == 1 && self.modal.is_none() => {
                if let Some(t) = draw::tab_at(m.column) {
                    self.tab = t;
                }
            }
            MouseEventKind::Down(MouseButton::Left) => {
                let point = ratatui::layout::Position::new(m.column, m.row);
                if let Some(target) = self
                    .click_targets
                    .iter()
                    .rev()
                    .find(|t| t.area.contains(point))
                {
                    match target.action.clone() {
                        draw::ClickAction::OpenUrl(url) => self.open_page(&url),
                        draw::ClickAction::Provider(i) => self.prov_sel = i,
                    }
                }
            }
            _ => {}
        }
    }
}

pub fn open_url(url: &str) -> Result<(), String> {
    let opener = if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    let status = Command::new(opener)
        .arg(url)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|e| format!("could not open browser: {e}. Open {url} manually"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "browser opener failed ({status}). Open {url} manually"
        ))
    }
}

pub fn copy(text: &str) -> Result<(), String> {
    let tools: &[(&str, &[&str])] = if cfg!(target_os = "macos") {
        &[("pbcopy", &[])]
    } else {
        &[
            ("wl-copy", &[]),
            ("xclip", &["-selection", "clipboard"]),
            ("xsel", &["--clipboard", "--input"]),
        ]
    };
    for (cmd, args) in tools {
        if let Ok(mut child) = Command::new(cmd)
            .args(*args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(text.as_bytes());
            }
            let _ = child.wait();
            return Ok(());
        }
    }
    Err("no clipboard tool found (install wl-clipboard or xclip)".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};

    fn app() -> App {
        let (tx, rx) = channel();
        App {
            chat: Chat::default(),
            tab: Tab::Providers,
            click_targets: Vec::new(),
            cfg: Err(String::new()),
            live: None,
            pid: None,
            autostart: false,
            prov_sel: 0,
            combo_sel: 0,
            target_sel: 0,
            combo_right: false,
            logs: Vec::new(),
            log_pos: 0,
            log_scroll: 0,
            connect_sel: 0,
            modal: None,
            toasts: Vec::new(),
            busy: 0,
            busy_label: String::new(),
            tick: 0,
            tx,
            rx,
            rt: tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap(),
            quit: false,
            last_poll: Instant::now(),
        }
    }

    fn render(app: &mut App, width: u16, height: u16) -> Terminal<TestBackend> {
        let mut term = Terminal::new(TestBackend::new(width, height)).unwrap();
        term.draw(|f| app.click_targets = draw::draw(f, app))
            .unwrap();
        term
    }

    #[test]
    fn cloudflare_setup_collects_account_before_saving() {
        let mut app = app();
        let entry = catalog::ALL
            .iter()
            .position(|e| e.id == "cloudflare")
            .unwrap();
        app.modal = Some(Modal::Key {
            entry: Some(entry),
            provider: "cloudflare".into(),
            input: "test-token".into(),
            reveal: false,
        });
        app.modal_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(
            matches!(&app.modal, Some(Modal::CloudflareAccount { key, .. }) if key == "test-token")
        );
        app.paste("invalid");
        app.modal_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(matches!(&app.modal, Some(Modal::CloudflareAccount { .. })));
        render(&mut app, 100, 30);
        app.modal_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(app.modal.is_none());
    }

    #[test]
    fn chat_typing_does_not_trigger_global_shortcuts() {
        let mut app = app();
        app.tab = Tab::Chat;
        let mut term = Terminal::with_options(
            ratatui::backend::CrosstermBackend::new(std::io::stdout()),
            ratatui::TerminalOptions {
                viewport: ratatui::Viewport::Fixed(ratatui::layout::Rect::new(0, 0, 100, 24)),
            },
        )
        .unwrap();
        for c in "qsARE?123456".chars() {
            app.key(
                KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE),
                &mut term,
            );
        }
        assert_eq!(app.chat.input, "qsARE?123456");
        assert!(!app.quit);
        assert!(app.modal.is_none());
        assert_eq!(app.busy, 0);
        app.paste("\n  pasted text  ");
        assert!(app.chat.input.ends_with("\n  pasted text  "));
        app.key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE), &mut term);
        assert!(app.tab == Tab::Dashboard);
        app.key(
            KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT),
            &mut term,
        );
        assert!(app.tab == Tab::Chat);
        app.key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE), &mut term);
        assert!(app.tab == Tab::Dashboard);
    }

    #[test]
    fn chat_failure_restores_prompt_and_preserves_history() {
        let mut app = app();
        app.chat.pending = true;
        app.chat.messages = vec![
            serde_json::json!({"role": "user", "content": "hello"}),
            serde_json::json!({"role": "assistant", "content": "hi"}),
            serde_json::json!({"role": "user", "content": "retry me"}),
        ];
        app.msg(Msg::Chat(Err("rate limited".into())));
        assert!(!app.chat.pending);
        assert_eq!(app.chat.messages.len(), 2);
        assert_eq!(app.chat.input, "retry me");
        assert_eq!(app.chat.error.as_deref(), Some("rate limited"));
    }

    #[test]
    fn chat_renders_wrapped_history_and_small_terminals() {
        let mut app = app();
        app.tab = Tab::Chat;
        app.chat.messages.push(serde_json::json!({"role": "assistant", "content": "long reply ".repeat(200) + "END_OF_REPLY"}));
        for (width, height) in [(120, 36), (80, 24), (40, 12), (10, 5)] {
            let term = render(&mut app, width, height);
            if width >= 40 {
                let buf = term.backend().buffer();
                let text: String = buf.content.iter().map(|cell| cell.symbol()).collect();
                assert!(text.contains("END_OF_REPLY"), "{width}x{height}: {text}");
            }
        }
    }

    #[test]
    fn key_links_have_visible_click_targets_in_panel_and_dialog() {
        let mut app = app();
        let i = catalog::ALL.iter().position(|e| e.needs_key()).unwrap();
        app.prov_sel = i;
        for dialog in [false, true] {
            if dialog {
                app.modal = Some(Modal::Key {
                    entry: Some(i),
                    provider: catalog::ALL[i].id.into(),
                    input: "unsaved-key".into(),
                    reveal: false,
                });
            }
            for (width, height) in [(120, 36), (80, 24), (60, 18)] {
                let term = render(&mut app, width, height);
                let links: Vec<_> = app
                    .click_targets
                    .iter()
                    .filter(|t| matches!(t.action, draw::ClickAction::OpenUrl(_)))
                    .collect();
                assert_eq!(links.len(), 1);
                let link = links[0];
                assert!(link.area.width > 0 && link.area.height >= 2);
                assert!(
                    matches!(&link.action, draw::ClickAction::OpenUrl(url) if url == catalog::ALL[i].key_url)
                );
                // Both the action label and URL occupy the registered clickable region.
                let buf = term.backend().buffer();
                assert_eq!(buf[(link.area.x, link.area.y)].symbol(), "O");
                assert_eq!(buf[(link.area.x, link.area.y + 1)].symbol(), "h");
                if dialog {
                    assert!(!app
                        .click_targets
                        .iter()
                        .any(|t| matches!(t.action, draw::ClickAction::Provider(_))));
                }
            }
        }
    }

    #[test]
    fn provider_click_targets_follow_scrolling_and_skip_section_labels() {
        let mut app = app();
        for selected in [0, catalog::ALL.len() - 1] {
            app.prov_sel = selected;
            let term = render(&mut app, 100, 16);
            let buf = term.backend().buffer();
            let mut found = false;
            for target in &app.click_targets {
                if let draw::ClickAction::Provider(i) = target.action {
                    let row: String = (target.area.x..target.area.right())
                        .map(|x| buf[(x, target.area.y)].symbol())
                        .collect();
                    let name: String = catalog::ALL[i].name.chars().take(10).collect();
                    assert!(row.contains(&name), "wrong provider at click target: {row}");
                    found |= i == selected;
                }
            }
            assert!(found);
        }
        app.modal = Some(Modal::Help);
        render(&mut app, 100, 36);
        assert!(app.click_targets.is_empty());
    }
}
