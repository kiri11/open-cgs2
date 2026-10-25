//! Events in the next 24 hours from the calendars in `calendars`, fetched every few minutes.
use super::forecast::epoch;
use super::{Res, Session, Ws};
use crate::history;
use serde_json::{json, Value};
use std::time::{Duration, Instant};

const REFRESH: Duration = Duration::from_secs(300);

/// Seconds since the epoch at the local midnight that starts `date` (`2026-10-22`).
fn midnight(date: &str) -> Option<i64> {
    let t = epoch(&format!("{date}T00:00:00Z"))?;
    Some(t - history::offset(t))
}

/// An event as the screen shows it; `call` when it has `link` in its description or location, or
/// there is no `link`.
fn event(e: &Value, link: Option<&str>) -> Option<Value> {
    let (start, end) = (e["start"].as_str()?, e["end"].as_str()?);
    let all_day = start.len() == 10;
    let at = |s: &str| if all_day { midnight(s) } else { epoch(s) };
    let call = link.is_none_or(|l| ["description", "location"].iter().any(|k| e[k].as_str().is_some_and(|t| t.contains(l))));
    Some(json!({"summary": e["summary"].as_str()?.trim(), "all_day": all_day, "start": at(start)?, "end": at(end)?, "call": call}))
}

impl Session {
    pub(super) fn calendar_tick(&mut self, ws: &mut Ws) -> Res<()> {
        let calendars = &self.shared.cfg.calendars;
        if calendars.is_empty() || self.calendar_due.is_some_and(|t| Instant::now() < t) {
            return Ok(());
        }
        let msg = json!({"type": "call_service", "domain": "calendar", "service": "get_events", "target": {"entity_id": calendars},
            "service_data": {"duration": {"hours": 24}}, "return_response": true});
        self.calendar_due = Some(Instant::now() + REFRESH);
        self.calendar_call = Some(self.send(ws, msg)?);
        Ok(())
    }

    pub(super) fn calendar_result(&self, result: &Value) {
        let mut events: Vec<Value> = result["response"]
            .as_object()
            .into_iter()
            .flat_map(|o| o.values())
            .flat_map(|c| c["events"].as_array().into_iter().flatten())
            .filter_map(|e| event(e, self.shared.cfg.calls_link.as_deref()))
            .collect();
        events.sort_by_key(|e| e["start"].as_i64());
        if self.shared.read(|s| s.calendar != events) {
            self.shared.update(|s| s.calendar = events);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_with_the_link_are_calls() {
        let e = json!({"summary": "Consultation", "start": "2026-10-22T10:00:00-04:00", "end": "2026-10-22T11:00:00-04:00",
            "description": "Join: https://meet.google.com/abc-defg-hij"});
        let call = |e: &Value, link| event(e, link).unwrap()["call"].clone();
        assert_eq!(call(&e, None), true);
        assert_eq!(call(&e, Some("meet.google.com/")), true);
        assert_eq!(call(&json!({"summary": "Lunch", "start": e["start"], "end": e["end"]}), Some("meet.google.com/")), false);
    }
}
