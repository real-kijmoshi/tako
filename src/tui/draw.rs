use super::{App, Modal, Picker, Row, Tab};
use crate::catalog::{self, Tier};
use crate::util::dur;
use ratatui::layout::Constraint::{Fill, Length, Min};
use ratatui::prelude::*;
use ratatui::widgets::*;
use serde_json::Value;

// 256-colour palette: looks the same in every terminal, including macOS Terminal.
const ACCENT: Color = Color::Indexed(111); // soft blue
const ACCENT2: Color = Color::Indexed(183); // lavender
const OK: Color = Color::Indexed(114);
const WARN: Color = Color::Indexed(221);
const ERR: Color = Color::Indexed(203);
const DIM: Color = Color::Indexed(245);
const FAINT: Color = Color::Indexed(238);
const CODE_BG: Color = Color::Indexed(235);
const SPIN: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

fn dim(s: impl Into<String>) -> Span<'static> {
    Span::styled(s.into(), Style::new().fg(DIM))
}
fn fg(s: impl Into<String>, c: Color) -> Span<'static> {
    Span::styled(s.into(), Style::new().fg(c))
}
fn bold(s: impl Into<String>, c: Color) -> Span<'static> {
    Span::styled(s.into(), Style::new().fg(c).add_modifier(Modifier::BOLD))
}
fn badge(s: &str, bg: Color) -> Span<'static> {
    Span::styled(
        format!(" {s} "),
        Style::new()
            .fg(Color::Indexed(234))
            .bg(bg)
            .add_modifier(Modifier::BOLD),
    )
}
fn tier_badge(t: Tier) -> Span<'static> {
    badge(
        t.label(),
        match t {
            Tier::Free => OK,
            Tier::Local => ACCENT,
            Tier::Paid => WARN,
        },
    )
}
fn panel(title: &str) -> Block<'static> {
    Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(FAINT))
        .title(Line::from(vec![
            Span::raw(" "),
            bold(title.to_string(), ACCENT2),
            Span::raw(" "),
        ]))
        .padding(Padding::horizontal(1))
}
fn mb(bytes: u64) -> String {
    if bytes == 0 {
        "n/a".into()
    } else {
        format!("{:.1} MB", bytes as f64 / 1_048_576.0)
    }
}
fn clock(t: u64) -> String {
    let t = t as libc::time_t;
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    unsafe { libc::localtime_r(&t, &mut tm) };
    format!("{:02}:{:02}:{:02}", tm.tm_hour, tm.tm_min, tm.tm_sec)
}
fn centered(area: Rect, w: u16, h: u16) -> Rect {
    let w = w.min(area.width.saturating_sub(2));
    let h = h.min(area.height.saturating_sub(2));
    Rect {
        x: area.x + (area.width - w) / 2,
        y: area.y + (area.height - h) / 2,
        width: w,
        height: h,
    }
}
fn mask(k: &str) -> String {
    let n = k.chars().count();
    if n <= 10 {
        "•".repeat(n)
    } else {
        let head: String = k.chars().take(4).collect();
        let tail: String = k.chars().skip(n - 4).collect();
        format!("{head}…{tail}")
    }
}

#[derive(Clone)]
pub enum ClickAction {
    OpenUrl(String),
    Provider(usize),
}

pub struct ClickTarget {
    pub area: Rect,
    pub action: ClickAction,
}

fn key_link(f: &mut Frame, area: Rect, url: &str, shortcut: &str, targets: &mut Vec<ClickTarget>) {
    if area.is_empty() {
        return;
    }
    let [label, link] = Layout::vertical([Length(1), Min(0)]).areas(area);
    f.render_widget(
        Paragraph::new(bold(
            format!("Open API key page ↗  (click or {shortcut})"),
            ACCENT,
        )),
        label,
    );
    f.render_widget(
        Paragraph::new(Span::styled(
            url,
            Style::new().fg(ACCENT).add_modifier(Modifier::UNDERLINED),
        )),
        link,
    );
    targets.push(ClickTarget {
        area,
        action: ClickAction::OpenUrl(url.into()),
    });
}

// ---- frame --------------------------------------------------------------

pub fn draw(f: &mut Frame, app: &App) -> Vec<ClickTarget> {
    let mut targets = Vec::new();
    let [head, tabs, body, foot] =
        Layout::vertical([Length(1), Length(2), Min(0), Length(1)]).areas(f.area());
    header(f, head, app);
    tab_bar(f, tabs, app);
    let body = body.inner(Margin::new(1, 0));
    match app.tab {
        Tab::Dashboard => dashboard(f, body, app),
        Tab::Providers => providers(f, body, app, &mut targets),
        Tab::Combos => combos(f, body, app),
        Tab::Logs => logs(f, body, app),
        Tab::Connect => connect(f, body, app),
        Tab::Chat => chat(f, body, app),
    }
    footer(f, foot, app);
    if let Some(m) = &app.modal {
        targets.clear(); // overlays must never activate controls underneath
        modal(f, f.area(), app, m, &mut targets);
    }
    toasts(f, body, app);
    targets
}

fn header(f: &mut Frame, area: Rect, app: &App) {
    let mut spans = vec![badge("◆ tako", ACCENT), Span::raw("  ")];
    match (&app.live, app.pid) {
        (Some(v), _) => {
            spans.push(bold("● running", OK));
            spans.push(dim(format!(
                "   v{}  ·  up {}  ·  {}  ·  http://{}",
                v["version"].as_str().unwrap_or("?"),
                dur(v["uptime"].as_u64().unwrap_or(0)),
                mb(v["rss"].as_u64().unwrap_or(0)),
                app.listen()
            )));
        }
        (None, Some(pid)) => spans.push(bold(format!("● pid {pid} not answering"), WARN)),
        (None, None) => {
            spans.push(bold("○ stopped", DIM));
            spans.push(dim("   press s to start"));
        }
    }
    f.render_widget(Paragraph::new(Line::from(spans)), area);
    let auto = if app.autostart {
        fg("autostart on ", OK)
    } else {
        dim("autostart off ")
    };
    f.render_widget(
        Paragraph::new(Line::from(auto)).alignment(Alignment::Right),
        area,
    );
}

fn tab_label(i: usize, t: Tab) -> String {
    format!(" {} {} ", i + 1, t.title())
}

/// Which tab sits under screen column `x` on the tab row.
pub fn tab_at(x: u16) -> Option<Tab> {
    let mut start = 1u16;
    for (i, t) in Tab::ALL.iter().enumerate() {
        let w = tab_label(i, *t).len() as u16;
        if x >= start && x < start + w {
            return Some(*t);
        }
        start += w + 1;
    }
    None
}

fn tab_bar(f: &mut Frame, area: Rect, app: &App) {
    let mut spans = vec![Span::raw(" ")];
    for (i, t) in Tab::ALL.iter().enumerate() {
        let label = tab_label(i, *t);
        spans.push(if *t == app.tab {
            Span::styled(
                label,
                Style::new()
                    .fg(Color::Indexed(234))
                    .bg(ACCENT2)
                    .add_modifier(Modifier::BOLD),
            )
        } else {
            dim(label)
        });
        spans.push(Span::raw(" "));
    }
    let [row, rule] = Layout::vertical([Length(1), Length(1)]).areas(area);
    f.render_widget(Paragraph::new(Line::from(spans)), row);
    f.render_widget(
        Paragraph::new(fg("─".repeat(area.width as usize), FAINT)),
        rule,
    );
}

fn footer(f: &mut Frame, area: Rect, app: &App) {
    let hints: &[(&str, &str)] = match app.tab {
        Tab::Dashboard => &[],
        Tab::Chat => &[
            ("enter", "send"),
            ("alt+enter", "newline"),
            ("ctrl+←→", "combo / reset"),
            ("ctrl+n", "new chat"),
            ("tab / esc", "leave"),
        ],
        Tab::Providers => &[
            ("↑↓", "move"),
            ("enter", "connect / pick models"),
            ("t", "test"),
            ("n", "add key"),
            ("o", "get key"),
            ("d", "remove"),
        ],
        Tab::Combos => &[
            ("←→", "focus"),
            ("a", "add models"),
            ("n", "new combo"),
            ("⇧↑↓", "reorder"),
            ("space", "make default"),
            ("d", "delete"),
        ],
        Tab::Logs => &[("↑↓ pgup pgdn", "scroll"), ("g", "top"), ("f", "follow")],
        Tab::Connect => &[("↑↓", "move"), ("enter", "run / copy"), ("c", "copy")],
    };
    let mut spans = vec![Span::raw(" ")];
    let start = if app.pid.is_some() { "stop" } else { "start" };
    let global = [
        ("s", start),
        ("A", "autostart"),
        ("E", "edit config"),
        ("?", "help"),
        ("q", "quit"),
    ];
    let global = if app.tab == Tab::Chat {
        &[][..]
    } else {
        &global[..]
    };
    let room = (area.width as usize).saturating_sub(if app.busy > 0 {
        app.busy_label.len() + 6
    } else {
        0
    });
    let mut used = 1;
    for (k, v) in hints.iter().chain(global.iter()) {
        let w = k.chars().count() + v.chars().count() + 4;
        if used + w > room {
            if used + 8 <= room && app.tab != Tab::Chat {
                spans.push(bold("?", ACCENT));
                spans.push(dim(" more"));
            }
            break;
        }
        used += w;
        spans.push(bold(k.to_string(), ACCENT));
        spans.push(dim(format!(" {v}   ")));
    }
    f.render_widget(Paragraph::new(Line::from(spans)), area);
    if app.busy > 0 {
        let s = Line::from(vec![
            fg(SPIN[(app.tick as usize) % 10], ACCENT),
            dim(format!(" {}… ", app.busy_label)),
        ]);
        f.render_widget(Paragraph::new(s).alignment(Alignment::Right), area);
    }
}

fn toasts(f: &mut Frame, body: Rect, app: &App) {
    let mut y = body.bottom();
    for t in app.toasts.iter().rev() {
        let w = (t.text.chars().count() as u16 + 6)
            .min(body.width.saturating_sub(2))
            .min(90);
        let lines = (t.text.chars().count() as u16 / w.saturating_sub(4).max(1)) + 1;
        let h = lines.min(4) + 2;
        if y < body.y + h {
            break;
        }
        y -= h;
        let r = Rect {
            x: body.right().saturating_sub(w),
            y,
            width: w,
            height: h,
        };
        let (icon, c) = if t.ok { ("✓ ", OK) } else { ("✗ ", ERR) };
        f.render_widget(Clear, r);
        f.render_widget(
            Paragraph::new(Line::from(vec![bold(icon, c), Span::raw(t.text.clone())]))
                .wrap(Wrap { trim: true })
                .block(
                    Block::bordered()
                        .border_type(BorderType::Rounded)
                        .border_style(Style::new().fg(c)),
                ),
            r,
        );
    }
}

// ---- chat -----------------------------------------------------------------

fn chat(f: &mut Frame, area: Rect, app: &App) {
    let compact = area.height < 14;
    let [model, history, status, input] = Layout::vertical([
        Length(1),
        Min(0),
        Length(if compact { 1 } else { 2 }),
        Length(if compact { 3 } else { 5 }),
    ])
    .areas(area);
    f.render_widget(
        Paragraph::new(Line::from(vec![
            bold("Combo: ", ACCENT2),
            Span::raw(app.chat_model().unwrap_or_else(|| "none".into())),
            dim("   ctrl+←→ change combo (starts a new chat)"),
        ])),
        model,
    );
    let mut lines = Vec::new();
    if app.chat.messages.is_empty() {
        lines.push(Line::from(dim(
            "Chat through tako using your configured providers and fallback chains.",
        )));
        lines.push(Line::from(dim(
            "Type a message below. History stays here until you start a new chat or quit.",
        )));
    }
    for message in &app.chat.messages {
        let user = message["role"] == "user";
        lines.push(Line::from(bold(
            if user { "You" } else { "Assistant" },
            if user { ACCENT } else { ACCENT2 },
        )));
        for line in message["content"].as_str().unwrap_or_default().split('\n') {
            lines.push(Line::raw(line.to_string()));
        }
        lines.push(Line::raw(""));
    }
    if !app.chat.messages.is_empty() {
        lines.pop();
    }
    let block = panel("conversation");
    let inner = block.inner(history);
    let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
    let max_scroll = paragraph
        .line_count(inner.width)
        .saturating_sub(inner.height as usize);
    let scroll = max_scroll
        .saturating_sub(app.chat.scroll.min(max_scroll))
        .min(u16::MAX as usize) as u16;
    f.render_widget(paragraph.scroll((scroll, 0)).block(block), history);
    let message = if let Some(error) = &app.chat.error {
        fg(error.clone(), ERR)
    } else if app.chat.pending {
        fg(
            format!(
                "{} Waiting for reply…",
                SPIN[app.tick as usize % SPIN.len()]
            ),
            ACCENT,
        )
    } else if app.pid.is_none() {
        fg("Router stopped. Esc, then s to start tako.", WARN)
    } else {
        dim("Enter sends · Alt+Enter adds a line · PgUp/PgDn scroll · Ctrl+U clears input")
    };
    f.render_widget(Paragraph::new(message).wrap(Wrap { trim: false }), status);
    let block = panel("message");
    let inner = block.inner(input);
    let text = format!("{}▏", app.chat.input);
    let paragraph = Paragraph::new(text).wrap(Wrap { trim: false });
    let scroll = paragraph
        .line_count(inner.width)
        .saturating_sub(inner.height as usize)
        .min(u16::MAX as usize) as u16;
    f.render_widget(paragraph.scroll((scroll, 0)).block(block), input);
}

// ---- dashboard ------------------------------------------------------------

fn onboarding(f: &mut Frame, area: Rect) {
    let steps = [
        (
            "1",
            "Connect a free provider",
            "press 2, pick Z.ai, OpenRouter, Groq, NVIDIA, Gemini…",
        ),
        (
            "2",
            "Pick models",
            "they go into the 'free' combo; tako falls through them in order",
        ),
        (
            "3",
            "Start coding",
            "press 5 to launch Claude Code, or run `tako claude`",
        ),
    ];
    let mut lines = vec![
        Line::from(bold("A free coding agent in three steps", ACCENT2)),
        Line::raw(""),
    ];
    for (n, t, d) in steps {
        lines.push(Line::from(vec![
            badge(n, ACCENT),
            Span::raw("  "),
            bold(format!("{t:<26}"), Color::Reset),
            dim(d),
        ]));
        lines.push(Line::raw(""));
    }
    lines.push(Line::from(dim(
        "Several free tiers chained together give you far more free requests than any one alone.",
    )));
    let r = centered(area, 100, 12);
    f.render_widget(
        Paragraph::new(lines).block(panel("welcome").padding(Padding::uniform(1))),
        r,
    );
}

fn dashboard(f: &mut Frame, area: Rect, app: &App) {
    if app.providers().is_empty() {
        return onboarding(f, area);
    }
    let Some(v) = &app.live else {
        let lines = vec![
            Line::from(bold("tako isn't running", Color::Reset)),
            Line::raw(""),
            Line::from(vec![
                bold("s", ACCENT),
                dim(" start now    "),
                bold("A", ACCENT),
                dim(" start at every login"),
            ]),
        ];
        let r = centered(area, 56, 7);
        f.render_widget(
            Paragraph::new(lines)
                .alignment(Alignment::Center)
                .block(panel("stopped").padding(Padding::uniform(1))),
            r,
        );
        return;
    };

    let stats = v["stats"].as_object().cloned().unwrap_or_default();
    let (mut ok, mut fail, mut ms_sum) = (0u64, 0u64, 0u64);
    for s in stats.values() {
        let o = s["ok"].as_u64().unwrap_or(0);
        ok += o;
        fail += s["fail"].as_u64().unwrap_or(0);
        ms_sum += s["avg_ms"].as_u64().unwrap_or(0) * o;
    }
    let total = ok + fail;
    let minutes: Vec<(u64, u64, u64)> = v["minutes"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|m| {
                    (
                        m[0].as_u64().unwrap_or(0),
                        m[1].as_u64().unwrap_or(0),
                        m[2].as_u64().unwrap_or(0),
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    let now_min = crate::util::now_secs() / 60;
    let hour: u64 = minutes
        .iter()
        .filter(|m| m.0 + 60 > now_min)
        .map(|m| m.1 + m.2)
        .sum();

    let [cards, token_cards, spark, bottom] =
        Layout::vertical([Length(5), Length(5), Length(6), Min(0)]).areas(area);
    let cs: [Rect; 4] = Layout::horizontal([Fill(1); 4]).spacing(1).areas(cards);
    let rate = if total > 0 {
        ok as f64 * 100.0 / total as f64
    } else {
        100.0
    };
    let rate_c = if rate >= 95.0 {
        OK
    } else if rate >= 80.0 {
        WARN
    } else {
        ERR
    };
    card(
        f,
        cs[0],
        "requests",
        total.to_string(),
        format!("{hour} in the last hour"),
        ACCENT,
    );
    card(
        f,
        cs[1],
        "success",
        if total > 0 {
            format!("{rate:.1}%")
        } else {
            "n/a".into()
        },
        format!("{fail} failed or fell back"),
        rate_c,
    );
    card(
        f,
        cs[2],
        "avg latency",
        if let Some(avg) = ms_sum.checked_div(ok) {
            format!("{avg} ms")
        } else {
            "n/a".into()
        },
        "time to first byte".into(),
        ACCENT2,
    );
    card(
        f,
        cs[3],
        "memory",
        mb(v["rss"].as_u64().unwrap_or(0)),
        format!("pid {}", v["pid"]),
        OK,
    );

    let ts: [Rect; 3] = Layout::horizontal([Fill(1); 3])
        .spacing(1)
        .areas(token_cards);
    let tokens = &v["tokens"];
    let estimated = tokens["estimated_requests"].as_u64().unwrap_or(0);
    card(
        f,
        ts[0],
        "tokens used",
        format!(
            "{}{}",
            if estimated > 0 { "~" } else { "" },
            tokens["total_tokens"].as_u64().unwrap_or(0)
        ),
        format!("since start; {estimated} estimated"),
        ACCENT,
    );
    card(
        f,
        ts[1],
        "input / output",
        format!(
            "{} / {}",
            tokens["input_tokens"].as_u64().unwrap_or(0),
            tokens["output_tokens"].as_u64().unwrap_or(0)
        ),
        "completed responses".into(),
        ACCENT2,
    );
    // Display the freshest observation; quotas across targets may overlap.
    let latest = v["token_limits"].as_object().and_then(|limits| {
        limits
            .iter()
            .max_by_key(|(_, q)| q["observed_at"].as_u64().unwrap_or(0))
    });
    let (remaining, detail) = latest.map_or(
        ("unknown".into(), "provider hasn't reported a limit".into()),
        |(key, q)| {
            (
                q["remaining"].to_string(),
                format!(
                    "{key}; {}s ago",
                    crate::util::now_secs().saturating_sub(q["observed_at"].as_u64().unwrap_or(0))
                ),
            )
        },
    );
    card(f, ts[2], "remaining (last report)", remaining, detail, OK);

    // traffic sparkline, one bar per minute
    let block = panel("traffic, last 60 min");
    let inner = block.inner(spark);
    f.render_widget(block, spark);
    // each minute gets the same number of columns so the chart fills the panel
    let per = (inner.width as u64 / 60).max(1);
    let slots = (inner.width as u64 / per).min(60);
    let width = slots * per;
    let data: Vec<u64> = (0..slots)
        .flat_map(|i| {
            let m = now_min + 1 + i - slots;
            let n = minutes.iter().find(|x| x.0 == m).map_or(0, |x| x.1 + x.2);
            std::iter::repeat_n(n, per as usize)
        })
        .collect();
    let [sl, axis] = Layout::vertical([Min(1), Length(1)]).areas(inner);
    let sl = Rect {
        width: width as u16,
        ..sl
    };
    f.render_widget(
        Sparkline::default()
            .data(&data)
            .style(Style::new().fg(ACCENT)),
        sl,
    );
    let axis = Rect {
        width: width as u16,
        ..axis
    };
    f.render_widget(Paragraph::new(dim(format!("{slots} min ago"))), axis);
    f.render_widget(Paragraph::new(dim("now")).alignment(Alignment::Right), axis);

    let [left, right] = Layout::horizontal([Fill(1), Fill(1)])
        .spacing(1)
        .areas(bottom);
    targets_table(f, left, app, v);
    recent(f, right, v);
}

fn card(f: &mut Frame, r: Rect, label: &str, value: String, sub: String, c: Color) {
    let lines = vec![
        Line::from(dim(label.to_uppercase())),
        Line::from(Span::styled(
            value,
            Style::new().fg(c).add_modifier(Modifier::BOLD),
        )),
        Line::from(dim(sub)),
    ];
    f.render_widget(
        Paragraph::new(lines).block(
            Block::bordered()
                .border_type(BorderType::Rounded)
                .border_style(Style::new().fg(FAINT))
                .padding(Padding::horizontal(1)),
        ),
        r,
    );
}

fn cooling_secs(v: &Value, tag: &str) -> Option<u64> {
    let prefix = format!("{tag} (");
    v["cooling"]
        .as_object()?
        .iter()
        .filter(|(k, _)| *k == tag || k.starts_with(&prefix))
        .filter_map(|(_, s)| s.as_u64())
        .min()
}

fn targets_table(f: &mut Frame, area: Rect, app: &App, v: &Value) {
    let mut tags: Vec<String> = app
        .cfg
        .as_ref()
        .map(|c| c.combos.values().flatten().cloned().collect())
        .unwrap_or_default();
    if let Some(s) = v["stats"].as_object() {
        tags.extend(s.keys().cloned());
    }
    tags.sort();
    tags.dedup();
    let rows: Vec<ratatui::widgets::Row> = tags
        .iter()
        .map(|t| {
            let s = &v["stats"][t];
            let ok = s["ok"].as_u64().unwrap_or(0);
            let fail = s["fail"].as_u64().unwrap_or(0);
            let avg = s["avg_ms"].as_u64().unwrap_or(0);
            let state = match cooling_secs(v, t) {
                Some(secs) => fg(format!("◌ cooling {secs}s"), WARN),
                None if ok + fail == 0 => dim("· idle"),
                None => fg("● ready", OK),
            };
            ratatui::widgets::Row::new(vec![
                Cell::from(t.clone()),
                Cell::from(fg(ok.to_string(), if ok > 0 { OK } else { DIM })),
                Cell::from(fg(fail.to_string(), if fail > 0 { ERR } else { DIM })),
                Cell::from(dim(if avg > 0 {
                    format!("{avg} ms")
                } else {
                    "–".into()
                })),
                Cell::from(state),
            ])
        })
        .collect();
    let header = ratatui::widgets::Row::new(["target", "ok", "fail", "avg", "state"])
        .style(Style::new().fg(DIM))
        .bottom_margin(0);
    let t = Table::new(rows, [Fill(1), Length(6), Length(5), Length(8), Length(15)])
        .header(header)
        .block(panel("models"))
        .column_spacing(1);
    f.render_widget(t, area);
}

fn recent(f: &mut Frame, area: Rect, v: &Value) {
    let items: Vec<ListItem> = v["recent"]
        .as_array()
        .map(|a| {
            a.iter()
                .rev()
                .map(|r| {
                    let status = r["status"].as_u64().unwrap_or(0);
                    let c = match status {
                        200..=299 => OK,
                        400..=499 => WARN,
                        _ => ERR,
                    };
                    let st = if status == 0 {
                        "ERR".to_string()
                    } else {
                        status.to_string()
                    };
                    ListItem::new(Line::from(vec![
                        dim(format!("{} ", clock(r["t"].as_u64().unwrap_or(0)))),
                        bold(format!("{st:>3} "), c),
                        Span::raw(r["model"].as_str().unwrap_or("").to_string()),
                        dim(" → "),
                        fg(r["target"].as_str().unwrap_or("").to_string(), ACCENT),
                        dim(format!("  {} ms", r["ms"].as_u64().unwrap_or(0))),
                    ]))
                })
                .collect()
        })
        .unwrap_or_default();
    let list = if items.is_empty() {
        List::new([ListItem::new(dim(
            "no requests yet. Connect a tool from tab 5.",
        ))])
    } else {
        List::new(items)
    };
    f.render_widget(list.block(panel("recent requests")), area);
}

// ---- providers ------------------------------------------------------------

fn providers(f: &mut Frame, area: Rect, app: &App, targets: &mut Vec<ClickTarget>) {
    let [left, right] = Layout::horizontal([Length(46.min(area.width * 2 / 5)), Min(0)])
        .spacing(1)
        .areas(area);
    let rows = app.rows();
    let cfg = app.cfg.as_ref().ok();

    let mut items: Vec<ListItem> = Vec::new();
    let mut map: Vec<Option<usize>> = Vec::new();
    let first_catalog = rows.iter().position(|r| matches!(r, Row::Catalog(_)));
    for (i, r) in rows.iter().enumerate() {
        if i == 0 && matches!(r, Row::Configured(_)) {
            items.push(ListItem::new(dim("CONNECTED")));
            map.push(None);
        }
        if Some(i) == first_catalog {
            if i > 0 {
                items.push(ListItem::new(""));
                map.push(None);
            }
            items.push(ListItem::new(dim("ADD A PROVIDER")));
            map.push(None);
        }
        let line = match r {
            Row::Configured(id) => {
                let p = cfg.and_then(|c| c.providers.get(id));
                let (dot, c) = match p {
                    Some(p) if p.disabled => ("●", ERR),
                    _ => ("●", OK),
                };
                let name = catalog::get(id).map(|e| e.name).unwrap_or("custom");
                let keys = p.map(|p| match p.keys.len() {
                    0 => String::new(),
                    1 => "1 key".into(),
                    n => format!("{n} keys"),
                });
                Line::from(vec![
                    fg(format!("{dot} "), c),
                    bold(format!("{id:<13}"), Color::Reset),
                    dim(format!("{name:<20}")),
                    dim(keys.unwrap_or_default()),
                ])
            }
            Row::Catalog(i) => {
                let e = &catalog::ALL[*i];
                Line::from(vec![
                    fg("+ ", ACCENT),
                    Span::raw(format!("{:<22}", e.name)),
                    tier_badge(e.tier),
                ])
            }
        };
        items.push(ListItem::new(line));
        map.push(Some(i));
    }
    let sel = map.iter().position(|m| *m == Some(app.prov_sel));
    let list = List::new(items)
        .block(panel("providers"))
        .highlight_style(Style::new().bg(Color::Indexed(237)))
        .highlight_symbol("▌");
    let list_inner = panel("providers").inner(left);
    let mut state = ListState::default().with_selected(sel);
    f.render_stateful_widget(list, left, &mut state);
    for (y, row) in map
        .iter()
        .skip(state.offset())
        .take(list_inner.height as usize)
        .enumerate()
    {
        if let Some(i) = row {
            targets.push(ClickTarget {
                area: Rect::new(list_inner.x, list_inner.y + y as u16, list_inner.width, 1),
                action: ClickAction::Provider(*i),
            });
        }
    }

    let mut lines: Vec<Line> = Vec::new();
    let title;
    match rows.get(app.prov_sel) {
        Some(Row::Catalog(i)) => {
            let e = &catalog::ALL[*i];
            title = e.name.to_string();
            lines.push(Line::from(vec![
                bold(e.name, Color::Reset),
                Span::raw("  "),
                tier_badge(e.tier),
            ]));
            lines.push(Line::raw(""));
            lines.push(Line::raw(e.blurb));
            lines.push(Line::raw(""));
            lines.push(Line::from(vec![dim("limits     "), Span::raw(e.limits)]));
            lines.push(Line::from(vec![
                dim("endpoint   "),
                fg(e.base_url, ACCENT),
                dim(format!("  ({})", e.wire.name())),
            ]));
            if !e.models.is_empty() {
                lines.push(Line::from(vec![
                    dim("suggested  "),
                    Span::raw(e.models.join(", ")),
                ]));
            }
            lines.push(Line::raw(""));
            lines.push(Line::from(vec![
                bold("enter", ACCENT),
                dim(if e.needs_key() {
                    " connect (paste a key)   "
                } else {
                    " connect   "
                }),
                if e.needs_key() {
                    bold("o", ACCENT)
                } else {
                    Span::raw("")
                },
                dim(if e.needs_key() {
                    " open the key page"
                } else {
                    ""
                }),
            ]));
        }
        Some(Row::Configured(id)) => {
            title = id.clone();
            let e = catalog::get(id);
            let p = cfg.and_then(|c| c.providers.get(id));
            lines.push(Line::from(vec![
                bold(
                    e.map(|e| e.name).unwrap_or(id.as_str()).to_string(),
                    Color::Reset,
                ),
                Span::raw("  "),
                e.map(|e| tier_badge(e.tier))
                    .unwrap_or_else(|| badge("CUSTOM", DIM)),
            ]));
            lines.push(Line::raw(""));
            if let Some(p) = p {
                lines.push(Line::from(vec![
                    dim("endpoint   "),
                    fg(p.base_url.clone(), ACCENT),
                    dim(format!("  ({})", p.wire.name())),
                ]));
                let keys = if p.disabled {
                    fg("configured env var is not set", ERR)
                } else if p.keys.is_empty() {
                    dim("none needed")
                } else {
                    Span::raw(
                        p.keys
                            .iter()
                            .map(|k| mask(k))
                            .collect::<Vec<_>>()
                            .join(", "),
                    )
                };
                lines.push(Line::from(vec![dim("keys       "), keys]));
                if let Some(m) = p.max_tokens {
                    lines.push(Line::from(vec![
                        dim("max tokens "),
                        Span::raw(m.to_string()),
                    ]));
                }
            }
            if let Some(e) = e {
                lines.push(Line::from(vec![dim("limits     "), Span::raw(e.limits)]));
            }
            lines.push(Line::raw(""));
            lines.push(Line::from(bold("used in", ACCENT2)));
            let prefix = format!("{id}/");
            let mut any = false;
            if let Some(c) = cfg {
                for (name, list) in &c.combos {
                    let ms: Vec<&str> = list
                        .iter()
                        .filter_map(|t| t.strip_prefix(&prefix))
                        .collect();
                    if !ms.is_empty() {
                        any = true;
                        lines.push(Line::from(vec![
                            fg(format!("  {name:<10} "), ACCENT),
                            Span::raw(ms.join(", ")),
                        ]));
                    }
                }
            }
            if !any {
                lines.push(Line::from(dim(
                    "  no combo yet. Press enter to pick models.",
                )));
            }
            if let Some(stats) = app.live.as_ref().and_then(|v| v["stats"].as_object()) {
                let mine: Vec<_> = stats
                    .iter()
                    .filter(|(k, _)| k.starts_with(&prefix))
                    .collect();
                if !mine.is_empty() {
                    lines.push(Line::raw(""));
                    lines.push(Line::from(bold("traffic", ACCENT2)));
                    for (k, s) in mine {
                        lines.push(Line::from(vec![
                            Span::raw(format!("  {k:<40} ")),
                            fg(format!("{} ok ", s["ok"]), OK),
                            fg(
                                format!("{} fail", s["fail"]),
                                if s["fail"].as_u64().unwrap_or(0) > 0 {
                                    ERR
                                } else {
                                    DIM
                                },
                            ),
                        ]));
                    }
                }
            }
            lines.push(Line::raw(""));
            lines.push(Line::from(vec![
                bold("enter", ACCENT),
                dim(" pick models   "),
                bold("t", ACCENT),
                dim(" test   "),
                bold("n", ACCENT),
                dim(" add another key   "),
                bold("d", ACCENT),
                dim(" remove"),
            ]));
        }
        None => title = String::new(),
    }
    let block = panel(&title);
    let inner = block.inner(right);
    f.render_widget(block, right);
    let entry = match rows.get(app.prov_sel) {
        Some(Row::Catalog(i)) => Some(&catalog::ALL[*i]),
        Some(Row::Configured(id)) => catalog::get(id),
        None => None,
    };
    let details = if let Some(e) = entry.filter(|e| e.needs_key()) {
        let [link, details] = Layout::vertical([Length(3), Min(0)]).areas(inner);
        key_link(f, link, e.key_url, "o", targets);
        details
    } else {
        inner
    };
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), details);
}

// ---- combos ---------------------------------------------------------------

fn combos(f: &mut Frame, area: Rect, app: &App) {
    let names = app.combos();
    let cfg = app.cfg.as_ref().ok();
    let default = cfg.and_then(|c| c.default.clone());
    if names.is_empty() {
        let lines = vec![
            Line::from(bold("No combos yet", Color::Reset)),
            Line::raw(""),
            Line::from(dim(
                "A combo is a model name, like 'free', that tries a list of models in order.",
            )),
            Line::from(dim(
                "When one is rate-limited or down, the next one answers.",
            )),
            Line::raw(""),
            Line::from(vec![
                bold("a", ACCENT),
                dim(" pick models   "),
                bold("n", ACCENT),
                dim(" new combo"),
            ]),
        ];
        let r = centered(area, 84, 10);
        f.render_widget(
            Paragraph::new(lines)
                .alignment(Alignment::Center)
                .block(panel("combos").padding(Padding::uniform(1))),
            r,
        );
        return;
    }
    let [left, right] = Layout::horizontal([Length(30), Min(0)])
        .spacing(1)
        .areas(area);
    let items: Vec<ListItem> = names
        .iter()
        .map(|n| {
            let count = app.combo_targets(n).len();
            let star = if default.as_deref() == Some(n) {
                fg("★ ", WARN)
            } else {
                Span::raw("  ")
            };
            ListItem::new(Line::from(vec![
                star,
                bold(format!("{n:<16}"), Color::Reset),
                dim(format!("{count} models")),
            ]))
        })
        .collect();
    let hl = if app.combo_right {
        Style::new().bg(Color::Indexed(236))
    } else {
        Style::new().bg(Color::Indexed(237))
    };
    let list = List::new(items)
        .block(panel("combos"))
        .highlight_style(hl)
        .highlight_symbol("▌");
    f.render_stateful_widget(
        list,
        left,
        &mut ListState::default().with_selected(Some(app.combo_sel)),
    );

    let name = names.get(app.combo_sel).cloned().unwrap_or_default();
    let targets = app.combo_targets(&name);
    let [list_area, info] = Layout::vertical([Min(3), Length(6)]).areas(right);
    let items: Vec<ListItem> = targets
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let free = t
                .split_once('/')
                .and_then(|(p, m)| catalog::get(p).map(|e| e.is_free(m, false)))
                .unwrap_or(false);
            let mut spans = vec![
                dim(format!("{:>2}  ", i + 1)),
                Span::raw(format!("{t:<46} ")),
            ];
            if free {
                spans.push(badge("FREE", OK));
                spans.push(Span::raw(" "));
            }
            if let Some(v) = &app.live {
                let s = &v["stats"][t];
                if let Some(secs) = cooling_secs(v, t) {
                    spans.push(fg(format!(" cooling {secs}s"), WARN));
                } else if s.is_object() {
                    spans.push(dim(format!(" {} ok · {} fail", s["ok"], s["fail"])));
                }
            }
            ListItem::new(Line::from(spans))
        })
        .collect();
    let hl = if app.combo_right {
        Style::new().bg(Color::Indexed(237))
    } else {
        Style::new()
    };
    let title = format!("{name}: tried top to bottom");
    let list = List::new(items)
        .block(panel(&title))
        .highlight_style(hl)
        .highlight_symbol(if app.combo_right { "▌" } else { " " });
    f.render_stateful_widget(
        list,
        list_area,
        &mut ListState::default().with_selected(Some(app.target_sel)),
    );

    let mut lines = vec![];
    match &default {
        Some(d) => lines.push(Line::from(vec![
            fg("★ ", WARN),
            dim("default: "),
            bold(d.clone(), Color::Reset),
            dim(". Requests for unknown models (like Claude Code's claude-*) go here."),
        ])),
        None => lines.push(Line::from(dim(
            "No default combo. Press space to make this one the default.",
        ))),
    }
    if let Some(c) = cfg {
        for (pat, t) in &c.aliases {
            lines.push(Line::from(vec![
                dim("alias   "),
                Span::raw(pat.clone()),
                dim(" → "),
                Span::raw(t.clone()),
            ]));
        }
    }
    lines.push(Line::from(vec![
        dim("use it  "),
        Span::raw(format!("model = \"{name}\"")),
        dim(format!("  at http://{}", app.listen())),
    ]));
    f.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(panel("routing")),
        info,
    );
}

// ---- logs -----------------------------------------------------------------

fn logs(f: &mut Frame, area: Rect, app: &App) {
    let follow = app.log_scroll == 0;
    let title = if follow {
        "log, following".to_string()
    } else {
        format!("log, {} lines up", app.log_scroll)
    };
    let block = panel(&title);
    let inner = block.inner(area);
    f.render_widget(block, area);
    let h = inner.height as usize;
    let end = app.logs.len().saturating_sub(app.log_scroll);
    let start = end.saturating_sub(h);
    let lines: Vec<Line> = app.logs[start..end]
        .iter()
        .map(|l| {
            let (ts, rest) = if l.len() > 20 && l.as_bytes()[4] == b'-' {
                l.split_at(20)
            } else {
                ("", l.as_str())
            };
            let c = if rest.contains("failed") || rest.contains("error") {
                ERR
            } else if rest.contains("ms: ") || rest.starts_with("warning") {
                WARN
            } else if rest.contains(" → ") {
                Color::Reset
            } else {
                ACCENT2
            };
            Line::from(vec![dim(ts.to_string()), fg(rest.to_string(), c)])
        })
        .collect();
    if lines.is_empty() {
        f.render_widget(Paragraph::new(dim("nothing logged yet")), inner);
    } else {
        f.render_widget(Paragraph::new(lines), inner);
    }
    let mut sb = ScrollbarState::new(app.logs.len().saturating_sub(h)).position(start);
    f.render_stateful_widget(
        Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(None)
            .end_symbol(None)
            .track_style(Style::new().fg(FAINT))
            .thumb_style(Style::new().fg(DIM)),
        area.inner(Margin::new(0, 1)),
        &mut sb,
    );
}

// ---- connect --------------------------------------------------------------

pub const CONNECT: [(&str, &str); 5] = [
    ("Claude Code", "launch it through tako"),
    ("OpenCode", "opencode.json provider"),
    ("Aider", "OpenAI-compatible"),
    ("Any OpenAI client", "Cursor, Continue, SDKs"),
    ("Test with curl", "check it's working"),
];

/// (description lines, enter-action label, snippet to copy)
pub fn connect_info(app: &App, i: usize) -> (Vec<String>, &'static str, String) {
    let base = format!("http://{}", app.listen());
    let tok = app.token();
    let model = app.default_combo().unwrap_or_else(|| "free".into());
    match i {
        0 => (
            vec![
                "Press enter to launch Claude Code with every request going through tako,".into(),
                format!("using the '{model}' combo. In a terminal it's just:"),
            ],
            "launch",
            "tako claude".into(),
        ),
        1 => (
            vec!["Add this to opencode.json in your project (or ~/.config/opencode/opencode.json),".into(), "then pick tako in /models.".into()],
            "copy",
            format!(
                "{{\n  \"$schema\": \"https://opencode.ai/config.json\",\n  \"provider\": {{\n    \"tako\": {{\n      \"npm\": \"@ai-sdk/openai-compatible\",\n      \"name\": \"tako\",\n      \"options\": {{ \"baseURL\": \"{base}/v1\", \"apiKey\": \"{tok}\" }},\n      \"models\": {{ \"{model}\": {{ \"name\": \"{model} (tako)\" }} }}\n    }}\n  }}\n}}"
            ),
        ),
        2 => (vec!["Aider talks to any OpenAI-compatible endpoint:".into()], "copy", format!("aider --openai-api-base {base}/v1 --openai-api-key {tok} --model openai/{model}")),
        3 => (
            vec!["Use these settings in any tool that accepts an OpenAI base URL.".into(), "Anthropic-style clients can use the same address without /v1.".into()],
            "copy",
            format!("base URL   {base}/v1\nAPI key    {tok}\nmodel      {model}"),
        ),
        _ => (
            vec!["Sends one request through tako. You should see it on the dashboard.".into()],
            "copy",
            format!("curl {base}/v1/chat/completions \\\n  -H 'authorization: Bearer {tok}' \\\n  -d '{{\"model\":\"{model}\",\"messages\":[{{\"role\":\"user\",\"content\":\"hi\"}}]}}'"),
        ),
    }
}

fn connect(f: &mut Frame, area: Rect, app: &App) {
    let [left, right] = Layout::horizontal([Length(36), Min(0)])
        .spacing(1)
        .areas(area);
    let items: Vec<ListItem> = CONNECT
        .iter()
        .map(|(n, d)| {
            ListItem::new(vec![
                Line::from(bold(*n, Color::Reset)),
                Line::from(dim(*d)),
                Line::raw(""),
            ])
        })
        .collect();
    let list = List::new(items)
        .block(panel("tools"))
        .highlight_style(Style::new().bg(Color::Indexed(237)))
        .highlight_symbol("▌");
    f.render_stateful_widget(
        list,
        left,
        &mut ListState::default().with_selected(Some(app.connect_sel)),
    );

    let (desc, action, snippet) = connect_info(app, app.connect_sel);
    let block = panel(CONNECT[app.connect_sel].0);
    let inner = block.inner(right);
    f.render_widget(block, right);
    let code_h = snippet.lines().count() as u16 + 2;
    let [d, code, keys, warn] = Layout::vertical([
        Length(desc.len() as u16 + 1),
        Length(code_h),
        Length(2),
        Min(0),
    ])
    .areas(inner);
    f.render_widget(
        Paragraph::new(desc.into_iter().map(Line::raw).collect::<Vec<_>>())
            .wrap(Wrap { trim: false }),
        d,
    );
    let code_lines: Vec<Line> = snippet
        .lines()
        .map(|l| Line::from(fg(l.to_string(), ACCENT)))
        .collect();
    f.render_widget(
        Paragraph::new(code_lines)
            .style(Style::new().bg(CODE_BG))
            .block(
                Block::new()
                    .style(Style::new().bg(CODE_BG))
                    .padding(Padding::new(2, 2, 1, 1)),
            ),
        code,
    );
    f.render_widget(
        Paragraph::new(Line::from(vec![
            bold("enter", ACCENT),
            dim(format!(" {action}   ")),
            bold("c", ACCENT),
            dim(" copy"),
        ]))
        .alignment(Alignment::Left),
        keys,
    );
    if app.pid.is_none() {
        f.render_widget(
            Paragraph::new(fg("tako isn't running yet. Press s to start it.", WARN)),
            warn,
        );
    }
}

// ---- modals ---------------------------------------------------------------

fn modal_block(title: &str) -> Block<'static> {
    Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(ACCENT))
        .title(Line::from(vec![
            Span::raw(" "),
            bold(title.to_string(), ACCENT),
            Span::raw(" "),
        ]))
        .padding(Padding::new(2, 2, 1, 0))
}

fn modal(f: &mut Frame, area: Rect, app: &App, m: &Modal, targets: &mut Vec<ClickTarget>) {
    match m {
        Modal::Help => help(f, area),
        Modal::Confirm { msg, .. } => {
            let r = centered(area, (msg.len() as u16 + 8).max(40), 7);
            f.render_widget(Clear, r);
            let lines = vec![
                Line::raw(msg.clone()),
                Line::raw(""),
                Line::from(vec![
                    bold("y", ACCENT),
                    dim(" yes   "),
                    bold("n", ACCENT),
                    dim(" no"),
                ]),
            ];
            f.render_widget(
                Paragraph::new(lines)
                    .wrap(Wrap { trim: false })
                    .block(modal_block("confirm")),
                r,
            );
        }
        Modal::Input { title, hint, value } => {
            let r = centered(area, 70, 10);
            f.render_widget(Clear, r);
            let block = modal_block(title);
            let inner = block.inner(r);
            f.render_widget(block, r);
            let [h, input, keys] = Layout::vertical([Length(2), Length(3), Length(1)]).areas(inner);
            f.render_widget(
                Paragraph::new(dim(hint.clone())).wrap(Wrap { trim: false }),
                h,
            );
            input_box(f, input, value, "");
            f.render_widget(
                Paragraph::new(Line::from(vec![
                    bold("enter", ACCENT),
                    dim(" next: pick models   "),
                    bold("esc", ACCENT),
                    dim(" cancel"),
                ])),
                keys,
            );
        }
        Modal::CloudflareAccount { input, .. } => {
            let r = centered(area, 78, 11);
            f.render_widget(Clear, r);
            let block = modal_block("Cloudflare account ID");
            let inner = block.inner(r);
            f.render_widget(block, r);
            let [hint, field, keys] =
                Layout::vertical([Length(3), Length(3), Length(1)]).areas(inner);
            f.render_widget(Paragraph::new("Copy the 32-character Account ID from your Cloudflare dashboard's account home or Workers AI page.").wrap(Wrap { trim: false }), hint);
            input_box(f, field, input, "paste your account ID");
            f.render_widget(
                Paragraph::new("enter save and pick models   ctrl+u clear   esc cancel"),
                keys,
            );
        }
        Modal::Key {
            entry,
            provider,
            input,
            reveal,
        } => {
            let e = entry.map(|i| &catalog::ALL[i]);
            let name = e.map(|e| e.name).unwrap_or(provider.as_str());
            let new = !app.providers().contains(provider);
            let r = centered(area, 84, 15);
            f.render_widget(Clear, r);
            let block = modal_block(&if new {
                format!("Connect {name}")
            } else {
                format!("Add a key to {name}")
            });
            let inner = block.inner(r);
            f.render_widget(block, r);
            let [info, inp, keys] =
                Layout::vertical([Length(7), Length(3), Length(1)]).areas(inner);
            let mut lines = vec![];
            if let Some(e) = e {
                lines.push(Line::from(vec![
                    tier_badge(e.tier),
                    Span::raw("  "),
                    dim(e.limits),
                ]));
                lines.push(Line::raw(""));
                if e.needs_key() {
                    lines.push(Line::from(vec![
                        bold("1  ", ACCENT2),
                        Span::raw("Create an API key using the link above."),
                    ]));
                    lines.push(Line::from(vec![
                        bold("2  ", ACCENT2),
                        Span::raw(
                            "Paste it below. It's stored only in your config file (mode 600).",
                        ),
                    ]));
                }
            } else {
                lines.push(Line::raw("Paste the extra key. tako rotates between keys and skips ones that hit limits."));
            }
            let info = if let Some(e) = e.filter(|e| e.needs_key()) {
                let [link, info] = Layout::vertical([Length(2), Min(0)]).areas(info);
                key_link(f, link, e.key_url, "ctrl+o", targets);
                info
            } else {
                info
            };
            f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), info);
            let shown = if *reveal {
                input.clone()
            } else {
                "•".repeat(input.chars().count())
            };
            input_box(f, inp, &shown, "paste your key here");
            f.render_widget(
                Paragraph::new(Line::from(vec![
                    bold("enter", ACCENT),
                    dim(" save   "),
                    bold("tab", ACCENT),
                    dim(if *reveal { " hide   " } else { " show   " }),
                    bold("ctrl+o", ACCENT),
                    dim(" open page   "),
                    bold("esc", ACCENT),
                    dim(" cancel"),
                ])),
                keys,
            );
        }
        Modal::Models(p) => picker(f, area, app, p),
    }
}

fn input_box(f: &mut Frame, r: Rect, value: &str, placeholder: &str) {
    let cursor = Span::styled(" ", Style::new().bg(ACCENT));
    let content = if value.is_empty() {
        Line::from(vec![cursor, dim(format!(" {placeholder}"))])
    } else {
        // keep the end of long values visible
        let max = r.width.saturating_sub(4) as usize;
        let n = value.chars().count();
        let shown: String = value.chars().skip(n.saturating_sub(max)).collect();
        Line::from(vec![Span::raw(shown), cursor])
    };
    f.render_widget(
        Paragraph::new(content).block(
            Block::bordered()
                .border_type(BorderType::Rounded)
                .border_style(Style::new().fg(ACCENT2))
                .padding(Padding::horizontal(1)),
        ),
        r,
    );
}

fn picker(f: &mut Frame, area: Rect, app: &App, p: &Picker) {
    let r = centered(area, 96, area.height.saturating_sub(4).max(16));
    f.render_widget(Clear, r);
    let block = modal_block("Pick models");
    let inner = block.inner(r);
    f.render_widget(block, r);
    let [combo, filt, status, list_area, keys] =
        Layout::vertical([Length(2), Length(3), Length(1), Min(3), Length(2)]).areas(inner);

    let existing = app.combo_targets(p.combo_name());
    let new_combo = !app.combos().iter().any(|c| c == p.combo_name());
    f.render_widget(
        Paragraph::new(Line::from(vec![
            dim("add to combo  "),
            fg("‹ ", ACCENT),
            bold(p.combo_name().to_string(), Color::Reset),
            fg(" ›", ACCENT),
            dim(if new_combo { "  (new)" } else { "" }),
            dim("     ←/→ to change"),
        ])),
        combo,
    );
    let free_tag = if p.free_only {
        badge("FREE ONLY", OK)
    } else {
        dim("all models")
    };
    let [fl, fr] = Layout::horizontal([Min(0), Length(24)]).areas(filt);
    input_box(f, fl, &p.filter, "type to filter…");
    f.render_widget(
        Paragraph::new(Line::from(vec![Span::raw("  "), free_tag, dim("  tab")]))
            .block(Block::new().padding(Padding::top(1))),
        fr,
    );

    let picked = p.items.iter().filter(|i| i.picked).count();
    let mut st = vec![];
    if p.pending > 0 {
        st.push(fg(format!("{} ", SPIN[app.tick as usize % 10]), ACCENT));
        st.push(dim(format!(
            "fetching model lists from {}…  ",
            p.providers.join(", ")
        )));
    }
    if !p.errors.is_empty() {
        st.push(fg(format!("{}  ", p.errors.join("; ")), ERR));
    }
    st.push(dim(format!("{} models · {picked} selected", p.items.len())));
    f.render_widget(Paragraph::new(Line::from(st)), status);

    let vis = p.visible();
    let items: Vec<ListItem> = vis
        .iter()
        .map(|&i| {
            let it = &p.items[i];
            let check = if existing.contains(&it.id) {
                fg("[✓] ", DIM)
            } else if it.picked {
                bold("[x] ", OK)
            } else {
                dim("[ ] ")
            };
            let mut spans = vec![check, Span::raw(format!("{:<58} ", it.id))];
            if it.free {
                spans.push(badge("FREE", OK));
                spans.push(Span::raw(" "));
            }
            if it.suggested {
                spans.push(badge("SUGGESTED", ACCENT2));
                spans.push(Span::raw(" "));
            }
            if existing.contains(&it.id) {
                spans.push(dim("already in combo"));
            }
            ListItem::new(Line::from(spans))
        })
        .collect();
    if items.is_empty() && p.pending == 0 {
        let msg = if p.filter.is_empty() {
            "no models found".to_string()
        } else {
            format!(
                "no match. Press enter to add \"{}\" as a custom model id",
                p.filter
            )
        };
        f.render_widget(Paragraph::new(dim(msg)), list_area);
    } else {
        let list = List::new(items)
            .highlight_style(Style::new().bg(Color::Indexed(237)))
            .highlight_symbol("▌");
        f.render_stateful_widget(
            list,
            list_area,
            &mut ListState::default().with_selected(Some(p.sel)),
        );
    }
    f.render_widget(
        Paragraph::new(Line::from(vec![
            bold("space", ACCENT),
            dim(" select   "),
            bold("enter", ACCENT),
            dim(" add to combo   "),
            bold("ctrl+a", ACCENT),
            dim(" all   "),
            bold("tab", ACCENT),
            dim(" free only   "),
            bold("esc", ACCENT),
            dim(" cancel"),
        ]))
        .block(Block::new().padding(Padding::top(1))),
        keys,
    );
}

fn help(f: &mut Frame, area: Rect) {
    let sections: [(&str, &[(&str, &str)]); 6] = [
        (
            "everywhere",
            &[
                ("1-6 / tab", "switch tab"),
                ("s", "start / stop tako"),
                ("R", "restart"),
                ("A", "autostart at login on/off"),
                ("E", "edit config in $EDITOR"),
                ("q", "quit (tako keeps running)"),
            ],
        ),
        (
            "chat",
            &[
                ("enter / alt+enter", "send / newline"),
                ("ctrl+←→", "change combo and reset chat"),
                ("ctrl+n / ctrl+u", "new chat / clear input"),
                ("tab / esc", "leave chat; letters type text"),
            ],
        ),
        (
            "providers",
            &[
                ("enter", "connect, or pick models"),
                ("t", "send a test request"),
                ("n", "add another key (rotated)"),
                ("o", "open the key page"),
                ("d", "remove"),
            ],
        ),
        (
            "combos",
            &[
                ("a", "add models"),
                ("n", "new combo"),
                ("shift+↑↓ / J K", "reorder"),
                ("space", "make default"),
                ("d", "delete"),
            ],
        ),
        (
            "logs",
            &[("↑↓ pgup pgdn", "scroll"), ("g / f", "top / follow")],
        ),
        (
            "model picker",
            &[
                ("type", "filter"),
                ("space", "select"),
                ("tab", "free only"),
                ("←→", "target combo"),
            ],
        ),
    ];
    let mut lines = vec![];
    for (title, keys) in sections {
        lines.push(Line::from(bold(title, ACCENT2)));
        for (k, d) in keys.iter() {
            lines.push(Line::from(vec![
                bold(format!("  {k:<18}"), ACCENT),
                Span::raw(d.to_string()),
            ]));
        }
        lines.push(Line::raw(""));
    }
    lines.push(Line::from(dim(
        "Closing this window doesn't stop tako. It keeps routing in the background.",
    )));
    let r = centered(area, 64, lines.len() as u16 + 3);
    f.render_widget(Clear, r);
    f.render_widget(Paragraph::new(lines).block(modal_block("keys")), r);
}
