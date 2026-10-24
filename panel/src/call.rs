//! Calls: when the `calls` component is on, every timed calendar event lights the screen and
//! shows itself 10 minutes before it starts; if nobody has closed it 5 minutes before, the panel
//! says so out loud.
use crate::{ha::Cmd, log, screen, Shared};
use serde_json::{json, Value};
use std::sync::mpsc::Sender;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const SHOW: i64 = 600;
const ANNOUNCE: i64 = 300;

#[derive(Clone, PartialEq)]
struct Call {
    summary: String,
    start: i64,
}

#[derive(Default, Clone)]
pub struct Status {
    open: Option<Call>,
    announced: bool,
    /// Calls already shown, so a closed one stays closed.
    shown: Vec<Call>,
}

impl Status {
    pub fn open(&self) -> bool {
        self.open.is_some()
    }
}

/// What to do now: the call to show, and whether to announce it.
fn step(st: &mut Status, events: &[Value], now: i64) -> (bool, Option<String>) {
    let calls: Vec<Call> = events
        .iter()
        .filter(|e| e["all_day"] == false)
        .filter_map(|e| Some(Call { summary: e["summary"].as_str()?.into(), start: e["start"].as_i64()? }))
        .collect();
    st.shown.retain(|c| c.start > now);
    if st.open.as_ref().is_some_and(|c| c.start <= now || !calls.contains(c)) {
        st.open = None;
    }
    let mut opened = false;
    if st.open.is_none() {
        if let Some(c) = calls.into_iter().find(|c| c.start - SHOW <= now && now < c.start && !st.shown.contains(c)) {
            st.shown.push(c.clone());
            st.open = Some(c);
            st.announced = false;
            opened = true;
        }
    }
    match &st.open {
        Some(c) if !st.announced && c.start - ANNOUNCE <= now => {
            st.announced = true;
            (opened, Some(format!("{} in {} minutes", c.summary, ((c.start - now + 30) / 60).max(1))))
        }
        _ => (opened, None),
    }
}

pub fn dismiss(shared: &Shared) {
    shared.update(|s| s.call.open = None);
}

pub fn snapshot(st: &Status) -> Value {
    st.open.as_ref().map_or(Value::Null, |c| json!({"summary": c.summary, "start": c.start}))
}

pub fn run(shared: Arc<Shared>, tx: Sender<Cmd>) {
    if !shared.cfg.calls {
        return;
    }
    loop {
        thread::sleep(Duration::from_secs(1));
        let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64);
        let (mut probe, events) = shared.read(|s| (s.call.clone(), s.calendar.clone()));
        let was = probe.open.clone();
        if step(&mut probe, &events, now) == (false, None) && probe.open == was {
            continue;
        }
        let mut changed = (false, None);
        shared.update(|s| changed = step(&mut s.call, &s.calendar, now));
        let (opened, announce) = changed;
        if opened || announce.is_some() {
            screen::set(&shared, true, "call");
        }
        if let Some(text) = announce {
            log(format!("call: {text}"));
            let _ = tx.send(Cmd::Announce(text));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shows_once_announces_once_and_closes_at_the_start() {
        let events = [
            json!({"summary": "Day", "all_day": true, "start": 1000}),
            json!({"summary": "Consultation", "all_day": false, "start": 1000}),
        ];
        let mut st = Status::default();
        assert_eq!(step(&mut st, &events, 1000 - SHOW - 1), (false, None));
        assert_eq!(step(&mut st, &events, 1000 - SHOW), (true, None));
        assert_eq!(snapshot(&st)["summary"], "Consultation");
        assert_eq!(step(&mut st, &events, 1000 - ANNOUNCE), (false, Some("Consultation in 5 minutes".into())));
        assert_eq!(step(&mut st, &events, 1000 - 1), (false, None));
        assert_eq!(step(&mut st, &events, 1000), (false, None));
        assert!(!st.open());

        let mut st = Status::default();
        step(&mut st, &events, 1000 - SHOW);
        st.open = None;
        assert_eq!(step(&mut st, &events, 1000 - ANNOUNCE), (false, None));
        assert!(!st.open());
    }
}
