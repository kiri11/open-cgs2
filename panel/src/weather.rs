//! Weather from Open-Meteo (free, no key): the current outside temperature, humidity and
//! condition, a rain note for the next two hours and the hourly and daily forecasts, in the
//! shape the weather page takes from a Home Assistant weather entity.
use crate::{fetch, log, Shared};
use serde_json::{json, Map, Value};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

const REFRESH: Duration = Duration::from_secs(900);
const RETRY: Duration = Duration::from_secs(300);

fn url(latitude: f64, longitude: f64, fahrenheit: bool) -> String {
    format!(
        "https://api.open-meteo.com/v1/forecast?latitude={latitude}&longitude={longitude}\
&current=temperature_2m,relative_humidity_2m,weather_code,is_day\
&hourly=temperature_2m,precipitation_probability,precipitation,weather_code,is_day\
&daily=weather_code,temperature_2m_max,temperature_2m_min,precipitation_sum,precipitation_probability_max\
&minutely_15=precipitation&forecast_days=8&forecast_minutely_15=8&timezone=auto&timeformat=unixtime\
&temperature_unit={}",
        if fahrenheit { "fahrenheit" } else { "celsius" }
    )
}

/// Home Assistant's condition for a WMO weather code.
fn condition(code: i64, day: bool) -> &'static str {
    match code {
        0 if day => "sunny",
        0 => "clear-night",
        1 | 2 => "partlycloudy",
        3 => "cloudy",
        45 | 48 => "fog",
        51..=55 | 61 | 63 | 80 | 81 => "rainy",
        65 | 82 => "pouring",
        56 | 57 | 66 | 67 => "snowy-rainy",
        71..=77 | 85 | 86 => "snowy",
        95..=99 => "lightning-rainy",
        _ => "cloudy",
    }
}

/// Element `i` of `block[key]`.
fn at<'a>(block: &'a Value, key: &str, i: usize) -> &'a Value {
    &block[key][i]
}

/// A sentence about rain in the next two hours, from 15-minute precipitation.
fn rain_note(times: &[i64], mm: &[f64], now: i64) -> String {
    let wet: Vec<bool> = times.iter().zip(mm).filter(|(t, _)| **t + 900 > now).map(|(_, m)| *m >= 0.1).take(8).collect();
    let minutes = |slots: usize| slots * 15;
    match (wet.first(), wet.iter().position(|w| *w), wet.iter().position(|w| !*w)) {
        (None, _, _) | (_, None, _) => "No rain for the next two hours".into(),
        (Some(true), _, None) => "Rain for the next two hours".into(),
        (Some(true), _, Some(dry)) => format!("Rain stopping in {} min", minutes(dry)),
        (_, Some(start), _) => format!("Rain starting in {} min", minutes(start).max(5)),
    }
}

pub fn parse(v: &Value, now: i64) -> Option<(Map<String, Value>, Map<String, Value>)> {
    let (c, h, d, m) = (&v["current"], &v["hourly"], &v["daily"], &v["minutely_15"]);
    let num = |x: &Value| x.as_f64();
    let hourly: Vec<Value> = (0..h["time"].as_array()?.len())
        .filter_map(|i| {
            let t = at(h, "time", i).as_i64()?;
            (t + 3600 > now).then(|| {
                json!({"t": t, "condition": condition(at(h, "weather_code", i).as_i64().unwrap_or(3), at(h, "is_day", i) == 1),
                    "temp": num(at(h, "temperature_2m", i)), "rain": num(at(h, "precipitation", i)),
                    "chance": num(at(h, "precipitation_probability", i))})
            })
        })
        .take(168)
        .collect();
    let daily: Vec<Value> = (0..d["time"].as_array()?.len())
        .map(|i| {
            json!({"t": at(d, "time", i), "condition": condition(at(d, "weather_code", i).as_i64().unwrap_or(3), true),
                "temp": num(at(d, "temperature_2m_max", i)), "low": num(at(d, "temperature_2m_min", i)),
                "rain": num(at(d, "precipitation_sum", i)), "chance": num(at(d, "precipitation_probability_max", i))})
        })
        .collect();
    let times: Vec<i64> = m["time"].as_array().into_iter().flatten().filter_map(Value::as_i64).collect();
    let mm: Vec<f64> = m["precipitation"].as_array().into_iter().flatten().map(|x| x.as_f64().unwrap_or(0.0)).collect();
    let mut forecast = Map::new();
    forecast.insert("hourly".into(), json!(hourly));
    forecast.insert("daily".into(), json!(daily));
    let mut now_values = Map::new();
    now_values.insert("outside".into(), c["temperature_2m"].clone());
    now_values.insert("outside_rh".into(), c["relative_humidity_2m"].clone());
    now_values.insert("sky".into(), json!(condition(c["weather_code"].as_i64().unwrap_or(3), c["is_day"] == 1)));
    now_values.insert("rain_soon".into(), json!(rain_note(&times, &mm, now)));
    Some((forecast, now_values))
}

pub fn run(shared: Arc<Shared>, latitude: f64, longitude: f64) {
    let url = url(latitude, longitude, shared.cfg.fahrenheit);
    let mut failing = false;
    loop {
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64);
        let parsed = fetch::get(&url).and_then(|b| serde_json::from_slice::<Value>(&b).map_err(|e| e.to_string()));
        match parsed.as_ref().ok().and_then(|v| parse(v, now)) {
            Some((forecast, current)) => {
                if failing {
                    log("weather: Open-Meteo answers again");
                }
                failing = false;
                shared.update(|s| {
                    s.forecast = forecast;
                    s.weather = current;
                });
                thread::sleep(REFRESH);
            }
            None => {
                if !failing {
                    log(format!("weather: {}", parsed.err().unwrap_or_else(|| "unexpected answer".into())));
                }
                failing = true;
                thread::sleep(RETRY);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_open_meteo() {
        let v = json!({
            "current": {"temperature_2m": 12.5, "relative_humidity_2m": 80, "weather_code": 61, "is_day": 1},
            "hourly": {"time": [0, 3600, 7200], "temperature_2m": [10, 11, 12], "precipitation_probability": [0, 50, 80],
                "precipitation": [0, 0.4, 2.1], "weather_code": [0, 61, 65], "is_day": [0, 1, 1]},
            "daily": {"time": [0], "weather_code": [95], "temperature_2m_max": [14], "temperature_2m_min": [6],
                "precipitation_sum": [3.2], "precipitation_probability_max": [90]},
            "minutely_15": {"time": [3600, 4500, 5400, 6300], "precipitation": [0, 0, 0.3, 0.5]}
        });
        let (f, now) = parse(&v, 3700).unwrap();
        assert_eq!(f["hourly"].as_array().unwrap().len(), 2);
        assert_eq!(f["hourly"][0]["condition"], "rainy");
        assert_eq!(f["daily"][0], json!({"t": 0, "condition": "lightning-rainy", "temp": 14.0, "low": 6.0, "rain": 3.2, "chance": 90.0}));
        assert_eq!(now["sky"], "rainy");
        assert_eq!(now["rain_soon"], "Rain starting in 30 min");
        assert_eq!(rain_note(&[0, 900], &[1.0, 1.0], 0), "Rain for the next two hours");
        assert_eq!(rain_note(&[0, 900], &[1.0, 0.0], 0), "Rain stopping in 15 min");
        assert_eq!(rain_note(&[], &[], 0), "No rain for the next two hours");
        assert_eq!(condition(0, false), "clear-night");
    }
}
