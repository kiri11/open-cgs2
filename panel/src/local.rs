//! This device's own readings from sensord (/run/open-cgs2/sensors), the values Home Assistant
//! gets, so the screen shows them while HA is away. A sensor key set in `sensors` in config.json wins.
use crate::Shared;
use serde_json::{Map, Value};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

const SENSORS: &str = "/run/open-cgs2/sensors";
/// Sensor keys on the screen and their names in sensord's status.
const KEYS: [(&str, &str); 5] = [("temp", "temperature"), ("rh", "humidity"), ("co2", "co2"), ("tvoc", "tvoc"), ("battery", "battery")];

/// °C to °F, to a tenth.
pub fn fahrenheit(c: f64) -> f64 {
    (c * 18.0).round() / 10.0 + 32.0
}

pub fn run(shared: Arc<Shared>) {
    loop {
        let v: Value = std::fs::read_to_string(SENSORS).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
        let mut local = readings(&v);
        if shared.cfg.fahrenheit {
            if let Some(c) = local.get("temp").and_then(Value::as_f64) {
                local.insert("temp".into(), serde_json::json!(fahrenheit(c)));
            }
        }
        if shared.read(|s| s.local != local) {
            shared.update(|s| s.local = local);
        }
        thread::sleep(Duration::from_secs(1));
    }
}

fn readings(status: &Value) -> Map<String, Value> {
    KEYS.iter().map(|(key, name)| (key.to_string(), status[*name].clone())).collect()
}
