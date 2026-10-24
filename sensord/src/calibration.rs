//! Corrections applied to the readings: `calibration` in config.json, overridden by what the
//! screen's Settings save in calibration.json, which is read again whenever it changes.
use crate::{log, STATE_DIR};
use serde_json::Value;
use std::fs;
use std::time::SystemTime;

/// Next to an Aranet4 and a thermostat, two units' SHT4x read about 10% more water vapour than
/// either, so the stock humidity read 7 to 9 points high.
const HUMIDITY_SCALE: f32 = 0.9;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Values {
    /// Applied to the raw humidity, before it is moved to the corrected temperature.
    pub humidity_scale: f32,
    /// Degrees Celsius.
    pub temperature_offset: f32,
    /// Percentage points.
    pub humidity_offset: f32,
}

pub struct Calibration {
    pub values: Values,
    base: Value,
    modified: Option<SystemTime>,
}

fn path() -> String {
    format!("{STATE_DIR}/calibration.json")
}

fn values(base: &Value, saved: &Value) -> Values {
    let get = |k: &str, default: f32, range: (f32, f32)| {
        saved[k].as_f64().or(base[k].as_f64()).map_or(default, |v| (v as f32).clamp(range.0, range.1))
    };
    Values {
        humidity_scale: get("humidity_scale", HUMIDITY_SCALE, (0.5, 1.5)),
        temperature_offset: get("temperature_offset", 0.0, (-10.0, 10.0)),
        humidity_offset: get("humidity_offset", 0.0, (-20.0, 20.0)),
    }
}

impl Calibration {
    pub fn load(config: &Value) -> Calibration {
        let base = config["calibration"].clone();
        let mut c = Calibration { values: values(&base, &Value::Null), base, modified: None };
        c.reload();
        c
    }

    pub fn reload(&mut self) {
        let modified = fs::metadata(path()).and_then(|m| m.modified()).ok();
        if modified == self.modified {
            return;
        }
        self.modified = modified;
        let saved = fs::read_to_string(path()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
        let v = values(&self.base, &saved);
        if v != self.values {
            log(format!("calibration: {v:?}"));
            self.values = v;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn saved_values_win_over_config() {
        assert_eq!(values(&Value::Null, &Value::Null), Values { humidity_scale: 0.9, temperature_offset: 0.0, humidity_offset: 0.0 });
        let base = json!({"humidity_scale": 0.88, "temperature_offset": -0.5});
        let v = values(&base, &json!({"temperature_offset": 1.0, "humidity_offset": 50}));
        assert_eq!(v, Values { humidity_scale: 0.88, temperature_offset: 1.0, humidity_offset: 20.0 });
    }
}
