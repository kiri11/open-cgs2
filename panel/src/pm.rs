//! The particulate sensor as sensord reports it (/run/open-cgs2/pm/status), and the switches on
//! the screen, sent to sensord as lines in /run/open-cgs2/pm/cmd.
use crate::{log, Shared};
use serde_json::{json, Value};
use std::io::Write;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

const STATUS: &str = "/run/open-cgs2/pm/status";
const CMD: &str = "/run/open-cgs2/pm/cmd";
/// How long a switch shows its new position before sensord's status must agree.
const PENDING: Duration = Duration::from_secs(3);

static LAST_SET: Mutex<Option<Instant>> = Mutex::new(None);

pub fn run(shared: Arc<Shared>) {
    let mut last = None;
    loop {
        let text = std::fs::read_to_string(STATUS).ok();
        let pending = LAST_SET.lock().unwrap().is_some_and(|t| t.elapsed() < PENDING);
        if text != last || !pending {
            let v = text.as_deref().and_then(|t| serde_json::from_str(t).ok()).unwrap_or(Value::Null);
            if shared.read(|s| s.pm != v) {
                shared.update(|s| s.pm = v);
            }
            last = text;
        }
        thread::sleep(Duration::from_secs(1));
    }
}

fn send(line: &str) -> bool {
    let sent = std::fs::OpenOptions::new().create(true).append(true).open(CMD).and_then(|mut f| f.write_all(format!("{line}\n").as_bytes()));
    if let Err(e) = sent {
        log(format!("{CMD}: {e}"));
        return false;
    }
    *LAST_SET.lock().unwrap() = Some(Instant::now());
    true
}

/// The night window, "HH:MM-HH:MM".
pub fn window(shared: &Shared, from: &str, until: &str) -> bool {
    let valid = |t: &str| t.split_once(':').is_some_and(|(h, m)| h.parse::<u32>().is_ok_and(|h| h < 24) && m.parse::<u32>().is_ok_and(|m| m < 60));
    if !valid(from) || !valid(until) || !send(&format!("window {from}-{until}")) {
        return false;
    }
    shared.update(|s| {
        if let Some(o) = s.pm.as_object_mut() {
            o.insert("from".into(), json!(from));
            o.insert("until".into(), json!(until));
        }
    });
    true
}

/// `what` is "power" or "night". The screen shows the change at once; sensord confirms it.
pub fn set(shared: &Shared, what: &str, on: bool) -> bool {
    if !send(&format!("{what} {}", if on { "on" } else { "off" })) {
        return false;
    }
    shared.update(|s| {
        if let Some(o) = s.pm.as_object_mut() {
            if what == "power" {
                o.insert("enabled".into(), json!(on));
                o.insert("state".into(), json!(if on { "warming" } else { "off" }));
            } else {
                o.insert("night".into(), json!(on));
            }
        }
    });
    true
}
