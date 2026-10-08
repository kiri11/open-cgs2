//! Thermostat: its state for the screen, and mode, target and fan changes from it.
use super::{Res, Session, Ws};
use crate::{log, Config, State};
use serde_json::{json, Value};

const ATTRS: [&str; 11] = [
    "hvac_modes",
    "hvac_action",
    "min_temp",
    "max_temp",
    "current_temperature",
    "current_humidity",
    "temperature",
    "target_temp_low",
    "target_temp_high",
    "preset_mode",
    "fan_mode",
];

pub fn snapshot(cfg: &Config, s: &State) -> Value {
    let Some(entity) = &cfg.climate else { return Value::Null };
    let attrs = s.attrs.get(entity);
    let mut out: serde_json::Map<String, Value> =
        ATTRS.iter().filter_map(|k| Some((k.to_string(), attrs?.get(*k)?.clone()))).collect();
    out.insert("mode".into(), json!(s.entities.get(entity)));
    out.insert("updated".into(), json!(s.updated.get(entity)));
    Value::Object(out)
}

fn temp(s: &str) -> Option<f64> {
    s.parse::<f64>().ok().filter(|t| (5.0..=95.0).contains(t))
}

/// Service and data for `action`/`arg`, such as `mode`/`heat`, `temp`/`21.5` or `range`/`19/26`, in
/// the thermostat's own units (5 to 95 covers °C and °F).
fn request(action: &str, arg: &str) -> Option<(&'static str, Value)> {
    match action {
        "mode" if ["off", "heat", "cool", "heat_cool"].contains(&arg) => Some(("set_hvac_mode", json!({"hvac_mode": arg}))),
        "temp" => Some(("set_temperature", json!({"temperature": temp(arg)?}))),
        "range" => {
            let (low, high) = arg.split_once('/')?;
            let (low, high) = (temp(low)?, temp(high)?);
            (low < high).then(|| ("set_temperature", json!({"target_temp_low": low, "target_temp_high": high})))
        }
        "fan" if ["on", "off"].contains(&arg) => Some(("set_fan_mode", json!({"fan_mode": arg}))),
        _ => None,
    }
}

impl Session {
    pub(super) fn climate(&mut self, ws: &mut Ws, action: &str, arg: &str) -> Res<()> {
        let Some(entity) = self.shared.cfg.climate.clone() else { return Ok(()) };
        let Some((service, data)) = request(action, arg) else { return Ok(()) };
        log(format!("thermostat: {service} {data}"));
        self.send(ws, json!({"type": "call_service", "domain": "climate", "service": service, "service_data": data, "target": {"entity_id": entity}}))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checks_requests() {
        assert_eq!(request("range", "19/26").unwrap().1, json!({"target_temp_low": 19.0, "target_temp_high": 26.0}));
        assert!(request("range", "26/19").is_none());
        assert!(request("temp", "120").is_none());
        assert!(request("mode", "dry").is_none());
        assert_eq!(request("temp", "21.5").unwrap().0, "set_temperature");
    }
}
