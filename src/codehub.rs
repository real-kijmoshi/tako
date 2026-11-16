//! Outbound-connected PC runners. The router queues work; it never opens a repo or runs code.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Default, Serialize, Deserialize)]
struct State {
    runners: BTreeMap<String, Value>,
    jobs: BTreeMap<String, Value>,
}
pub struct Hub {
    path: PathBuf,
    state: Mutex<State>,
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
fn hash(s: &str) -> String {
    Sha256::digest(s.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
fn text<'a>(v: &'a Value, key: &str, max: usize) -> Result<&'a str, String> {
    v[key]
        .as_str()
        .filter(|s| !s.trim().is_empty() && s.len() <= max)
        .ok_or_else(|| format!("Invalid {key}"))
}
fn public_runner(v: &Value) -> Value {
    let mut v = v.clone();
    v.as_object_mut().unwrap().remove("secret_hash");
    v["online"] = json!(now().saturating_sub(v["seen"].as_u64().unwrap_or(0)) < 45);
    v
}
fn summary(v: &Value) -> Value {
    let mut v = v.clone();
    for k in ["events", "diff", "review", "tests"] {
        v.as_object_mut().unwrap().remove(k);
    }
    v
}
impl Hub {
    pub fn open(path: PathBuf) -> Result<Self, String> {
        let state = match fs::read(&path) {
            Ok(b) => serde_json::from_slice(&b).map_err(|_| "Invalid Tako Code state file")?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => State::default(),
            Err(e) => return Err(e.to_string()),
        };
        Ok(Self {
            path,
            state: Mutex::new(state),
        })
    }
    fn save(&self, s: &State) -> Result<(), String> {
        let tmp = self.path.with_extension("json.tmp");
        let mut options = fs::OpenOptions::new();
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = options
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&tmp)
            .map_err(|e| e.to_string())?;
        f.set_permissions(fs::Permissions::from_mode(0o600))
            .map_err(|e| e.to_string())?;
        f.write_all(&serde_json::to_vec(s).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        fs::rename(tmp, &self.path).map_err(|e| e.to_string())
    }
    pub fn overview(&self) -> Result<Value, String> {
        let s = self.state.lock().map_err(|_| "Code state unavailable")?;
        let mut jobs: Vec<_> = s.jobs.values().map(summary).collect();
        jobs.sort_by_key(|v| std::cmp::Reverse(v["created"].as_u64()));
        Ok(
            json!({"runners": s.runners.values().map(public_runner).collect::<Vec<_>>(), "jobs":jobs, "protocol":1}),
        )
    }
    pub fn admin(&self, action: &str, v: &Value) -> Result<Value, String> {
        let mut s = self.state.lock().map_err(|_| "Code state unavailable")?;
        match action {
            "job" => Ok(s
                .jobs
                .get(text(v, "id", 64)?)
                .ok_or("Job not found")?
                .clone()),
            "create" => {
                if s.jobs.len() >= 200 {
                    return Err("Archive old local jobs before queuing more than 200 jobs".into());
                }
                let runner = text(v, "runner", 64)?;
                let r = s
                    .runners
                    .get(runner)
                    .ok_or("Connect your PC runner first")?;
                if now().saturating_sub(r["seen"].as_u64().unwrap_or(0)) >= 45 {
                    return Err("PC runner is offline".into());
                }
                let project = text(v, "project", 64)?;
                if !r["projects"]
                    .as_array()
                    .is_some_and(|a| a.iter().any(|p| p["id"] == project))
                {
                    return Err("Project was not registered locally on this PC".into());
                }
                let engine = text(v, "engine", 24)?;
                if !r["engines"]
                    .as_array()
                    .is_some_and(|a| a.iter().any(|e| e == engine))
                {
                    return Err("Engine is not installed on this PC".into());
                }
                let workers = v["workers"]
                    .as_u64()
                    .filter(|n| (1..=6).contains(n))
                    .ok_or("Choose 1–6 agents")?;
                let task = text(v, "task", 16000)?;
                let id = crate::auth::secret()[..16].to_string();
                let job = json!({"id":id,"runner":runner,"project":project,"engine":engine,"workers_requested":workers,"task":task,"phase":"queued","created":now(),"updated":now(),"events":[],"workers":[],"cancel":false,"free_only":true});
                s.jobs.insert(id.clone(), job.clone());
                self.save(&s)?;
                Ok(job)
            }
            "cancel" => {
                let j = s.jobs.get_mut(text(v, "id", 64)?).ok_or("Job not found")?;
                if matches!(
                    j["phase"].as_str(),
                    Some("ready" | "failed" | "cancelled" | "needs_attention" | "applied")
                ) {
                    return Err("Job is already finished".into());
                }
                j["cancel"] = json!(true);
                if j["phase"] == "queued" {
                    j["phase"] = json!("cancelled");
                }
                j["updated"] = json!(now());
                self.save(&s)?;
                Ok(json!({"ok":true}))
            }
            _ => Err("Unknown Code action".into()),
        }
    }
    pub fn runner(&self, action: &str, v: &Value) -> Result<Value, String> {
        let mut s = self.state.lock().map_err(|_| "Code state unavailable")?;
        let id = text(v, "runner", 64)?.to_string();
        if action == "register" && !s.runners.contains_key(&id) {
            if s.runners.len() >= 50 {
                return Err("Runner limit reached".into());
            }
            let secret = crate::auth::secret();
            s.runners.insert(id.clone(), json!({"id":id,"name":text(v,"name",100)?,"secret_hash":hash(&secret),"seen":now(),"projects":[],"engines":[]}));
            self.save(&s)?;
            return Ok(json!({"runner":id,"runner_token":secret,"protocol":1}));
        }
        let r = s.runners.get_mut(&id).ok_or("Register this PC first")?;
        if r["secret_hash"].as_str() != Some(hash(text(v, "runner_token", 128)?).as_str()) {
            return Err("Invalid runner credential".into());
        }
        r["seen"] = json!(now());
        if action == "register" || action == "poll" {
            let projects = v["projects"]
                .as_array()
                .ok_or("Expected registered projects")?;
            if projects.len() > 50 {
                return Err("Too many projects".into());
            }
            let mut clean = Vec::new();
            for p in projects {
                clean.push(json!({"id":text(p,"id",64)?,"name":text(p,"name",100)?}));
            }
            let engines: Vec<_> = v["engines"]
                .as_array()
                .ok_or("Expected engines")?
                .iter()
                .filter_map(Value::as_str)
                .filter(|e| matches!(*e, "builtin" | "opencode" | "claude" | "codex"))
                .collect();
            r["projects"] = json!(clean);
            r["engines"] = json!(engines);
            r["name"] = json!(text(v, "name", 100)?);
        }
        // Snapshot what this runner may use before borrowing the job list.
        let (projects, engines) = (r["projects"].clone(), r["engines"].clone());
        let result = match action {
            "register" => json!({"ok":true,"protocol":1}),
            // A job typed in the PC's terminal UI: created already leased to that PC so
            // the web UI shows it alongside jobs queued from the browser.
            "submit" => {
                if s.jobs.len() >= 200 {
                    return Err("Archive old local jobs before queuing more than 200 jobs".into());
                }
                let busy = s.jobs.values().any(|j| {
                    j["runner"] == id
                        && matches!(
                            j["phase"].as_str(),
                            Some("planning" | "working" | "integrating" | "reviewing")
                        )
                });
                if busy {
                    return Err("This PC is already running a swarm".into());
                }
                let project = text(v, "project", 64)?;
                if !projects
                    .as_array()
                    .is_some_and(|a| a.iter().any(|p| p["id"] == project))
                {
                    return Err("Project was not registered locally on this PC".into());
                }
                let engine = text(v, "engine", 24)?;
                if !engines
                    .as_array()
                    .is_some_and(|a| a.iter().any(|e| e == engine))
                {
                    return Err("Engine is not installed on this PC".into());
                }
                let workers = v["workers"]
                    .as_u64()
                    .filter(|n| (1..=6).contains(n))
                    .ok_or("Choose 1–6 agents")?;
                let task = text(v, "task", 16000)?;
                let job_id = crate::auth::secret()[..16].to_string();
                let job = json!({"id":job_id,"runner":id,"project":project,"engine":engine,"workers_requested":workers,"task":task,"phase":"planning","created":now(),"updated":now(),"events":[],"workers":[],"cancel":false,"free_only":true,"source":"terminal"});
                s.jobs.insert(job_id, job.clone());
                json!({"job":job})
            }
            "recover" => {
                for job in s.jobs.values_mut().filter(|j| j["runner"] == id) {
                    if matches!(
                        job["phase"].as_str(),
                        Some("planning" | "working" | "integrating" | "reviewing")
                    ) {
                        job["phase"] = json!("failed");
                        job["error"] = json!("PC runner restarted; local worktrees are preserved");
                        job["updated"] = json!(now());
                    }
                }
                json!({"ok":true})
            }
            "poll" => {
                let active = v["busy"] == true
                    || s.jobs.values().any(|j| {
                        j["runner"] == id
                            && matches!(
                                j["phase"].as_str(),
                                Some("planning" | "working" | "integrating" | "reviewing")
                            )
                    });
                let job = if !active {
                    let next = s
                        .jobs
                        .values()
                        .filter(|j| {
                            j["runner"] == id && j["phase"] == "queued" && j["cancel"] != true
                        })
                        .min_by_key(|j| j["created"].as_u64())
                        .and_then(|j| j["id"].as_str())
                        .map(String::from);
                    next.and_then(|key| s.jobs.get_mut(&key)).map(|j| {
                        j["phase"] = json!("planning");
                        j["updated"] = json!(now());
                        j.clone()
                    })
                } else {
                    None
                };
                let cancel: Vec<_> = s
                    .jobs
                    .values()
                    .filter(|j| j["runner"] == id && j["cancel"] == true)
                    .filter_map(|j| j["id"].as_str())
                    .collect();
                json!({"job":job,"cancel":cancel,"protocol":1})
            }
            "event" => {
                let job = s
                    .jobs
                    .get_mut(text(v, "job", 64)?)
                    .filter(|j| j["runner"] == id)
                    .ok_or("Job is not assigned to this runner")?;
                let event = v
                    .get("event")
                    .filter(|v| v.is_object())
                    .ok_or("Expected event")?;
                if let Some(phase) = event["phase"].as_str() {
                    if !matches!(
                        phase,
                        "planning"
                            | "working"
                            | "integrating"
                            | "reviewing"
                            | "ready"
                            | "needs_attention"
                            | "failed"
                            | "cancelled"
                            | "applied"
                    ) {
                        return Err("Invalid phase".into());
                    }
                    job["phase"] = json!(phase);
                }
                for field in [
                    "workers",
                    "branch",
                    "base",
                    "diff",
                    "diff_truncated",
                    "review",
                    "tests",
                    "error",
                    "apply_command",
                ] {
                    if let Some(value) = event.get(field) {
                        if value.to_string().len() > 250_000 {
                            return Err("Code event field too large".into());
                        }
                        job[field] = value.clone();
                    }
                }
                let events = job["events"].as_array_mut().ok_or("Invalid job history")?;
                events.push(json!({"time":now(),"worker":event["worker"],"message":event["message"].as_str().unwrap_or("").chars().take(2000).collect::<String>(),"phase":event["phase"]}));
                if events.len() > 300 {
                    events.drain(..events.len() - 300);
                }
                job["updated"] = json!(now());
                json!({"ok":true,"cancel":job["cancel"]})
            }
            _ => return Err("Unknown runner action".into()),
        };
        self.save(&s)?;
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn credentials_leases_recovery_and_persistence() {
        let folder = std::env::temp_dir().join(format!("tako-code-{}", crate::auth::secret()));
        fs::create_dir_all(&folder).unwrap();
        let path = folder.join("state.json");
        let hub = Hub::open(path.clone()).unwrap();
        let first = hub
            .runner("register", &json!({"runner":"pc1","name":"My PC"}))
            .unwrap();
        let mut pc = json!({"runner":"pc1","runner_token":first["runner_token"],"name":"My PC","projects":[{"id":"repo1","name":"App","path":"secret-local-path"}],"engines":["builtin","codex","rogue"]});
        hub.runner("register", &pc).unwrap();
        let overview = hub.overview().unwrap();
        assert!(!overview.to_string().contains("secret_hash"));
        assert!(!overview.to_string().contains("secret-local-path"));
        assert_eq!(
            overview["runners"][0]["engines"],
            json!(["builtin", "codex"])
        );
        let mut bad = pc.clone();
        bad["runner_token"] = json!("incorrect");
        assert!(hub.runner("poll", &bad).is_err());
        let mut task = json!({"runner":"pc1","project":"repo1","engine":"rogue","workers":2,"task":"Implement a feature"});
        assert!(hub.admin("create", &task).is_err());
        task["engine"] = json!("builtin");
        let job = hub.admin("create", &task).unwrap();
        let next = hub.admin("create", &task).unwrap();
        pc["busy"] = json!(true);
        assert!(hub.runner("poll", &pc).unwrap()["job"].is_null());
        pc["busy"] = json!(false);
        let leased = hub.runner("poll", &pc).unwrap()["job"].clone();
        assert_eq!(leased["phase"], "planning");
        assert!(hub.runner("poll", &pc).unwrap()["job"].is_null());
        let other = hub
            .runner("register", &json!({"runner":"pc2","name":"Other PC"}))
            .unwrap();
        assert!(hub.runner("event", &json!({"runner":"pc2","runner_token":other["runner_token"],"job":leased["id"],"event":{"phase":"ready"}})).is_err());
        hub.runner("recover", &pc).unwrap();
        assert_eq!(
            hub.admin("job", &json!({"id":leased["id"]})).unwrap()["phase"],
            "failed"
        );
        let queued = if leased["id"] == job["id"] { next } else { job };
        hub.admin("cancel", &json!({"id":queued["id"]})).unwrap();
        assert_eq!(
            hub.admin("job", &json!({"id":queued["id"]})).unwrap()["phase"],
            "cancelled"
        );
        assert!(hub.runner("poll", &pc).unwrap()["job"].is_null());
        let reopened = Hub::open(path.clone()).unwrap();
        assert_eq!(
            reopened.overview().unwrap()["jobs"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        assert_eq!(
            fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn terminal_submit_is_leased_scoped_and_exclusive() {
        let folder = std::env::temp_dir().join(format!("tako-code-{}", crate::auth::secret()));
        fs::create_dir_all(&folder).unwrap();
        let hub = Hub::open(folder.join("state.json")).unwrap();
        let first = hub
            .runner("register", &json!({"runner":"pc1","name":"PC"}))
            .unwrap();
        let pc = json!({"runner":"pc1","runner_token":first["runner_token"],"name":"PC","projects":[{"id":"repo1","name":"App"}],"engines":["builtin","claude"]});
        hub.runner("register", &pc).unwrap();
        let submit = |extra: Value| {
            let mut v = pc.clone();
            v.as_object_mut()
                .unwrap()
                .extend(extra.as_object().unwrap().clone());
            hub.runner("submit", &v)
        };
        assert!(
            submit(json!({"project":"other","engine":"claude","workers":3,"task":"x"})).is_err()
        );
        assert!(
            submit(json!({"project":"repo1","engine":"codex","workers":3,"task":"x"})).is_err()
        );
        assert!(
            submit(json!({"project":"repo1","engine":"claude","workers":9,"task":"x"})).is_err()
        );
        let mut forged = pc.clone();
        forged["runner_token"] = json!("bad");
        forged["project"] = json!("repo1");
        assert!(hub.runner("submit", &forged).is_err());
        let job =
            submit(json!({"project":"repo1","engine":"claude","workers":3,"task":"Add tests"}))
                .unwrap()["job"]
                .clone();
        assert_eq!(job["phase"], "planning");
        assert_eq!(job["source"], "terminal");
        assert!(hub.runner("poll", &pc).unwrap()["job"].is_null());
        assert!(
            submit(json!({"project":"repo1","engine":"claude","workers":1,"task":"y"})).is_err()
        );
        assert_eq!(hub.overview().unwrap()["jobs"][0]["id"], job["id"]);
        fs::remove_dir_all(folder).unwrap();
    }
}
