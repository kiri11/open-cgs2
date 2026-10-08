//! Hourly and daily forecasts of a weather entity, pushed by HA whenever they update.
use super::{Res, Session, Ws};
use serde_json::{json, Value};

const KINDS: [&str; 2] = ["hourly", "daily"];

/// Seconds since the epoch for an ISO 8601 time such as `2026-10-08T00:00:00+00:00`.
pub(super) fn epoch(s: &str) -> Option<i64> {
    let num = |a: usize, b: usize| s.get(a..b)?.parse::<i64>().ok();
    let (y, m, d) = (num(0, 4)?, num(5, 7)?, num(8, 10)?);
    let secs = num(11, 13)? * 3600 + num(14, 16)? * 60 + num(17, 19)?;
    let zone = s.get(19..)?.trim_start_matches(|c: char| c == '.' || c.is_ascii_digit());
    let offset = match zone.as_bytes().first() {
        None | Some(b'Z') => 0,
        Some(sign) => {
            let o = zone.get(1..3)?.parse::<i64>().ok()? * 3600 + zone.get(4..6)?.parse::<i64>().ok()? * 60;
            if *sign == b'-' { -o } else { o }
        }
    };
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * ((m + 9) % 12) + 2) / 5 + d - 1;
    let days = era * 146097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719468;
    Some(days * 86400 + secs - offset)
}

fn trim(f: &Value) -> Option<Value> {
    Some(json!({
        "t": epoch(f["datetime"].as_str()?)?,
        "condition": f["condition"],
        "temp": f["temperature"],
        "low": f["templow"],
        "rain": f["precipitation"],
        "chance": f["precipitation_probability"],
    }))
}

impl Session {
    pub(super) fn subscribe_forecasts(&mut self, ws: &mut Ws) -> Res<()> {
        self.forecasts.clear();
        let crate::Weather::Entity(entity) = &self.shared.cfg.weather else { return Ok(()) };
        let entity = entity.clone();
        for kind in KINDS {
            let id = self.send(ws, json!({"type": "weather/subscribe_forecast", "entity_id": entity, "forecast_type": kind}))?;
            self.forecasts.insert(id, kind);
        }
        Ok(())
    }

    pub(super) fn forecast_event(&self, kind: &str, ev: &Value) {
        let list: Vec<Value> = ev["forecast"].as_array().into_iter().flatten().filter_map(trim).collect();
        self.shared.update(|s| drop(s.forecast.insert(kind.to_string(), json!(list))));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_iso_times() {
        assert_eq!(epoch("2026-10-07T21:49:45+00:00"), Some(1791409785));
        assert_eq!(epoch("2026-10-07T17:49:45-04:00"), Some(1791409785));
        assert_eq!(epoch("2026-10-07T21:49:45.123456Z"), Some(1791409785));
        assert_eq!(epoch("2000-02-29T00:00:00+00:00"), Some(951782400));
        assert_eq!(epoch("garbage"), None);
    }
}
