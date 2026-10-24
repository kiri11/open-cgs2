//! config.json, with what the panel found in Home Assistant (detected.json) filling in whatever
//! it leaves out, and the components in `ui/components` it turns on and places on the screen.
use crate::{audio, doorbell, intercom, log};
use serde_json::{json, Map, Value};
use std::path::Path;

pub struct Group {
    pub id: String,
    pub name: String,
    pub entities: Vec<String>,
    pub off_only: bool,
}

pub struct Speaker {
    pub id: String,
    pub name: String,
    /// Spotify Connect device names, when Spotify can transfer playback to it. Spotify sometimes
    /// lists a device under its ID instead of its name, so the ID can be given as a second name.
    pub spotify: Vec<String>,
    /// Media player that plays Spotify on its own (Sonos); controls go there instead of Spotify.
    pub entity: Option<String>,
}

pub struct Series {
    pub name: String,
    pub entity: Option<String>,
    /// This device's own reading, from sensord's log, drawn above the others.
    pub here: bool,
    pub outside: bool,
}

pub struct Chart {
    pub key: String,
    pub series: Vec<Series>,
}

pub struct Music {
    pub spotify: String,
    pub speakers: Vec<Speaker>,
    pub default: String,
    /// Spotify-made playlists such as Daily Mixes, which HA's Spotify integration can play but not list.
    pub mixes: Vec<Value>,
    /// Whether HA has `spotify.recently_played`, which the stock Spotify integration lacks.
    pub recent: bool,
}

pub enum Weather {
    None,
    OpenMeteo { latitude: f64, longitude: f64 },
    Entity(String),
}

pub struct Config {
    /// host:port of Home Assistant, empty without one.
    pub ha: String,
    pub token: String,
    pub assist: bool,
    pub pipeline: Option<String>,
    /// This unit's device in HA, so Assist knows which room it is in.
    pub device_id: Option<String>,
    pub sensors: Vec<(String, String)>,
    pub groups: Vec<Group>,
    pub music: Option<Music>,
    pub charts: Vec<Chart>,
    pub weather: Weather,
    /// Calendars whose events in the next 24 hours show on the home screen.
    pub calendars: Vec<String>,
    pub calls: bool,
    pub climate: Option<String>,
    /// Minutes without an update after which the thermostat page dims its values.
    pub climate_stale: Option<u64>,
    pub doorbell: Option<doorbell::Doorbell>,
    pub mic: audio::MicConfig,
    pub intercom: Option<intercom::Intercom>,
    /// Entities that components without a backend of their own follow.
    pub follow: Vec<String>,
    pub fahrenheit: bool,
    pub clock24: bool,
    pub name: String,
    /// config.json's `calibration`, the defaults below what Settings saves.
    pub calibration: Value,
    /// `latitude` and `longitude`: config.json's `location`, else Home Assistant's.
    pub location: Value,
    pub port: u16,
    /// The enabled components, as the screen loads them.
    pub components: Vec<Value>,
    pub pages: Value,
    /// What the panel found in HA when this config was built.
    pub detected: Value,
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array().map_or_else(Vec::new, |a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
}

fn slug(s: &str) -> String {
    s.to_lowercase().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect()
}

/// The `host:port` part of `http://host:port/`.
fn host(url: &str) -> String {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let host = rest.split('/').next().unwrap_or_default();
    if host.is_empty() || host.contains(':') { host.into() } else { format!("{host}:8123") }
}

fn groups(v: &Value) -> Vec<Group> {
    v.as_array().map_or_else(Vec::new, |a| {
        a.iter()
            .map(|g| Group {
                id: g["id"].as_str().map_or_else(|| slug(g["name"].as_str().unwrap_or_default()), String::from),
                name: g["name"].as_str().unwrap_or_default().into(),
                entities: strings(&g["entities"]),
                off_only: g["off_only"].as_bool().unwrap_or(false),
            })
            .collect()
    })
}

fn speakers(v: &Value) -> Vec<Speaker> {
    v.as_array().map_or_else(Vec::new, |a| {
        a.iter()
            .map(|s| {
                let name = s["name"].as_str().unwrap_or_default().to_string();
                Speaker {
                    id: s["id"].as_str().map_or_else(|| slug(&name), String::from),
                    spotify: match &s["spotify"] {
                        Value::String(n) => vec![n.clone()],
                        Value::Null => vec![name.clone()],
                        v => strings(v),
                    },
                    entity: s["entity"].as_str().map(String::from),
                    name,
                }
            })
            .collect()
    })
}

/// Chart keys on the screen and the matching entity keys of other open-cgs2 units.
const CHART_KEYS: [(&str, &str); 6] = [("co2", "co2"), ("temp", "temperature"), ("rh", "humidity"), ("tvoc", "etvoc"), ("pm25", "pm25"), ("pm10", "pm10")];

fn charts(v: &Value, name: &str, detected: &Value) -> Vec<Chart> {
    let series = |a: &Value| {
        a.as_array().map_or_else(Vec::new, |a| {
            a.iter()
                .filter_map(|s| {
                    Some(Series {
                        name: s["name"].as_str()?.into(),
                        entity: s["entity"].as_str().map(String::from),
                        here: s["here"].as_bool().unwrap_or(false),
                        outside: s["outside"].as_bool().unwrap_or(false),
                    })
                })
                .collect()
        })
    };
    CHART_KEYS
        .iter()
        .map(|(key, unit_key)| {
            let configured = series(&v[*key]);
            let series = if configured.is_empty() {
                let mut s = vec![Series { name: name.into(), entity: None, here: true, outside: false }];
                s.extend(detected["units"].as_array().into_iter().flatten().filter_map(|u| {
                    Some(Series { name: u["name"].as_str()?.into(), entity: Some(u["entities"][*unit_key].as_str()?.into()), here: false, outside: false })
                }));
                s
            } else {
                configured
            };
            Chart { key: key.to_string(), series }
        })
        .collect()
}

/// The manifests in `ui/components`, by folder name.
fn manifests(ui: &Path) -> Vec<(String, Value)> {
    let mut out: Vec<(String, Value)> = std::fs::read_dir(ui.join("components"))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let text = std::fs::read_to_string(e.path().join("component.json")).ok()?;
            match serde_json::from_str(&text) {
                Ok(v) => Some((name, v)),
                Err(e) => {
                    log(format!("components/{name}/component.json: {e}"));
                    None
                }
            }
        })
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}


impl Config {
    pub fn load(path: &str, ui: &Path, detected: &Path) -> Result<Config, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
        let v: Value = serde_json::from_str(&text).map_err(|e| format!("{path}: {e}"))?;
        let detected = std::fs::read_to_string(detected).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
        Ok(Config::build(&v, ui, detected))
    }

    pub fn build(v: &Value, ui: &Path, detected: Value) -> Config {
        let ha = v["home_assistant"]["url"].as_str().map(host).unwrap_or_default();
        let has_ha = !ha.is_empty();
        let name = v["device"]["name"].as_str().unwrap_or("CGS2").to_string();
        let location = if v["location"]["latitude"].is_number() { v["location"].clone() } else { detected["location"].clone() };
        let mut enabled: Vec<(String, Value, Value)> = Vec::new();
        for (dir, manifest) in manifests(ui) {
            let opts = v["components"][&dir].clone();
            let setting = match &opts {
                Value::Bool(b) => json!(b),
                o => o.get("enabled").cloned().unwrap_or_else(|| manifest["default"].clone()),
            };
            let on = match setting {
                Value::Bool(b) => b,
                Value::String(s) if s == "auto" => auto(&dir, has_ha, v, &detected, &location),
                _ => false,
            };
            if on {
                enabled.push((dir, manifest, if opts.is_object() { opts } else { json!({}) }));
            }
        }
        let opts = |name: &str| enabled.iter().find(|(d, _, _)| d == name).map(|(_, _, o)| o.clone());
        let d = &detected;
        let groups = opts("lights").map_or_else(Vec::new, |o| groups(if o["groups"].is_array() { &o["groups"] } else { &d["lights"] }));
        let music = opts("spotify").and_then(|o| {
            let spotify = o["entity"].as_str().or(d["spotify"]["entity"].as_str())?.to_string();
            let mut speakers = speakers(&o["speakers"]);
            if speakers.is_empty() {
                speakers = strings(&d["spotify"]["sources"]).into_iter().map(|n| Speaker { id: slug(&n), spotify: vec![n.clone()], entity: None, name: n }).collect();
            }
            let default = o["default"].as_str().map(String::from).or_else(|| speakers.first().map(|s| s.id.clone())).unwrap_or_default();
            Some(Music { spotify, speakers, default, mixes: o["mixes"].as_array().cloned().unwrap_or_default(), recent: d["spotify"]["recent"] == true })
        });
        let calendar = opts("calendar").or(opts("calls"));
        let calendars = calendar.map_or_else(Vec::new, |o| if o["calendars"].is_array() { strings(&o["calendars"]) } else { strings(&d["calendars"]) });
        let weather = match opts("weather") {
            None => Weather::None,
            Some(o) => {
                let (lat, lon) = (location["latitude"].as_f64(), location["longitude"].as_f64());
                match (o["entity"].as_str().or(d["weather"].as_str().filter(|_| o["source"] == "home_assistant")), lat, lon) {
                    (Some(e), _, _) => Weather::Entity(e.into()),
                    (None, Some(latitude), Some(longitude)) => Weather::OpenMeteo { latitude, longitude },
                    _ => d["weather"].as_str().map_or(Weather::None, |e| Weather::Entity(e.into())),
                }
            }
        };
        let thermostat = opts("thermostat");
        let mut follow: Vec<String> = enabled.iter().flat_map(|(_, _, o)| strings(&o["entities"])).collect();
        follow.sort();
        follow.dedup();
        let m = &v["mic"];
        let components = enabled
            .iter()
            .map(|(dir, manifest, o)| {
                let mut c = Map::new();
                c.insert("name".into(), json!(dir));
                for slot in ["page", "status", "button", "side", "tiles", "overlay"] {
                    if let Some(f) = manifest[slot].as_str() {
                        c.insert(slot.into(), json!(f));
                    }
                }
                c.insert("position".into(), manifest["position"].clone());
                c.insert("order".into(), json!(manifest["order"].as_i64().unwrap_or(50)));
                c.insert("layer".into(), json!(manifest["layer"].as_i64().unwrap_or(0)));
                c.insert("config".into(), o.clone());
                Value::Object(c)
            })
            .collect();
        Config {
            token: v["home_assistant"]["token"].as_str().unwrap_or_default().into(),
            assist: opts("assist").is_some() && has_ha,
            pipeline: opts("assist").and_then(|o| o["pipeline"].as_str().map(String::from)),
            device_id: d["device_id"].as_str().map(String::from),
            sensors: v["sensors"].as_object().map_or_else(Vec::new, |o| o.iter().filter_map(|(k, e)| Some((k.clone(), e.as_str()?.to_string()))).collect()),
            groups,
            music,
            charts: charts(&v["charts"], &name, d),
            weather,
            calendars,
            calls: opts("calls").is_some(),
            climate: thermostat.as_ref().and_then(|o| o["entity"].as_str().or(d["climate"].as_str()).map(String::from)),
            climate_stale: thermostat.as_ref().and_then(|o| o["stale_after"].as_u64()),
            doorbell: opts("doorbell").and_then(|o| doorbell::Doorbell::from_json(if o["camera"].is_string() { &o } else { &d["doorbell"] })),
            intercom: opts("intercom").and_then(|o| intercom::Intercom::from_json(&o, &name)),
            mic: audio::MicConfig {
                device: m["device"].as_str().unwrap_or("plug:dsnooper").into(),
                channels: m["channels"].as_u64().unwrap_or(4) as usize,
                channel: m["channel"].as_u64().unwrap_or(2) as usize,
                gain: m["gain"].as_f64().unwrap_or(2.0) as f32,
            },
            follow,
            fahrenheit: v["units"]["temperature"] == "F",
            clock24: v["units"]["clock"] != "12h",
            calibration: v["calibration"].clone(),
            location,
            port: v["port"].as_u64().unwrap_or(8099) as u16,
            components,
            pages: v["pages"].clone(),
            ha,
            name,
            detected,
        }
    }

    pub fn entity_ids(&self) -> Vec<String> {
        let mut ids: Vec<String> = self.sensors.iter().map(|(_, e)| e.clone()).collect();
        ids.extend(self.groups.iter().flat_map(|g| g.entities.iter().cloned()));
        if let Some(m) = &self.music {
            ids.push(m.spotify.clone());
            ids.extend(m.speakers.iter().filter_map(|s| s.entity.clone()));
        }
        ids.extend(self.climate.clone());
        if let Weather::Entity(e) = &self.weather {
            ids.push(e.clone());
        }
        ids.extend(self.doorbell.iter().flat_map(doorbell::Doorbell::entity_ids));
        ids.extend(self.follow.iter().cloned());
        ids.sort();
        ids.dedup();
        ids
    }
}

/// Whether a component set to `auto` turns on.
fn auto(name: &str, has_ha: bool, v: &Value, d: &Value, location: &Value) -> bool {
    let found = |k: &str| !d[k].is_null() && d[k] != json!([]);
    match name {
        "lights" => found("lights"),
        "thermostat" => found("climate"),
        "assist" => has_ha && d["assist"] == true,
        "spotify" => found("spotify"),
        "calendar" => found("calendars"),
        "doorbell" => found("doorbell"),
        "weather" => location["latitude"].is_number() || found("weather"),
        "intercom" => v["components"]["intercom"]["key"].is_string() || v["components"]["intercom"]["peers"].is_array(),
        _ => has_ha,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub fn ui() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../device/open-cgs2/ui")
    }

    #[test]
    fn auto_components_follow_what_ha_has() {
        let detected = json!({
            "lights": [{"id": "kitchen", "name": "Kitchen", "entities": ["light.kitchen"]}],
            "climate": "climate.hall",
            "assist": true,
            "spotify": {"entity": "media_player.spotify", "sources": ["Kitchen speaker"], "recent": false},
            "units": [{"name": "Bedroom", "entities": {"co2": "sensor.bedroom_co2"}}],
        });
        let v = json!({"home_assistant": {"url": "http://ha.local:8123", "token": "t"}, "device": {"name": "Kitchen"},
            "location": {"latitude": 1.5, "longitude": 2.5}});
        let c = Config::build(&v, &ui(), detected);
        let names: Vec<&str> = c.components.iter().filter_map(|c| c["name"].as_str()).collect();
        assert_eq!(names, ["assist", "lights", "spotify", "thermostat", "weather"]);
        assert_eq!(c.ha, "ha.local:8123");
        assert_eq!(c.groups[0].entities, ["light.kitchen"]);
        assert_eq!(c.music.as_ref().unwrap().default, "kitchen_speaker");
        assert_eq!(c.climate.as_deref(), Some("climate.hall"));
        assert!(matches!(c.weather, Weather::OpenMeteo { .. }));
        assert_eq!(c.charts[0].series.len(), 2);
        assert_eq!(c.charts[0].series[1].entity.as_deref(), Some("sensor.bedroom_co2"));
        assert!(c.intercom.is_none() && !c.calls);
    }

    #[test]
    fn without_ha_only_local_components_run() {
        let c = Config::build(&json!({}), &ui(), Value::Null);
        assert!(c.components.is_empty());
        assert!(c.ha.is_empty() && c.clock24 && !c.fahrenheit);
        let c = Config::build(&json!({"components": {"path": true, "weather": {"enabled": false}}, "units": {"clock": "12h"}}), &ui(), Value::Null);
        let names: Vec<&str> = c.components.iter().filter_map(|c| c["name"].as_str()).collect();
        assert_eq!(names, ["path"]);
        assert!(!c.clock24);
    }

    #[test]
    fn weather_uses_home_assistants_location_unless_told_otherwise() {
        let d = json!({"weather": "weather.home", "location": {"latitude": 1.0, "longitude": 2.0}});
        let c = Config::build(&json!({}), &ui(), d.clone());
        assert!(matches!(c.weather, Weather::OpenMeteo { latitude, .. } if latitude == 1.0));
        let c = Config::build(&json!({"components": {"weather": {"source": "home_assistant"}}}), &ui(), d);
        assert!(matches!(c.weather, Weather::Entity(ref e) if e == "weather.home"));
    }

    #[test]
    fn parses_ha_urls() {
        assert_eq!(host("http://192.0.2.1:8123/"), "192.0.2.1:8123");
        assert_eq!(host("homeassistant.local"), "homeassistant.local:8123");
    }
}
