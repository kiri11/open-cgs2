//! What Home Assistant has for the components set to `auto`: lights by area, a thermostat, Spotify,
//! calendars, a doorbell, a weather entity, other open-cgs2 units for the charts and this unit's
//! own device. It is checked on every connection; when it differs from what the running config was
//! built from, it is saved to detected.json and the panel exits so the supervisor restarts it
//! with the new components.
use crate::{log, STATE_DIR};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::net::TcpStream;
use tungstenite::{Message, WebSocket};

type Res<T> = Result<T, Box<dyn std::error::Error>>;

/// The session's first request ID: HA wants them increasing, and detection uses the ones before.
pub const FIRST_ID: u64 = 100;
/// Areas with lights get a tile each, up to this many, next to All off.
const LIGHT_GROUPS: usize = 5;

fn request(ws: &mut WebSocket<TcpStream>, id: u64, kind: &str) -> Res<Value> {
    ws.send(Message::Text(json!({"id": id, "type": kind}).to_string()))?;
    loop {
        if let Message::Text(t) = ws.read()? {
            let v: Value = serde_json::from_str(&t)?;
            if v["id"] == id {
                return Ok(v["result"].clone());
            }
        }
    }
}

pub fn check(ws: &mut WebSocket<TcpStream>, cached: &Value) -> Res<()> {
    let kinds = ["config/entity_registry/list", "config/device_registry/list", "config/area_registry/list", "get_states", "get_services", "assist_pipeline/pipeline/list", "get_config"];
    let mut results = Vec::new();
    for (i, kind) in kinds.iter().enumerate() {
        results.push(request(ws, 1 + i as u64, kind)?);
    }
    let mac = std::fs::read_to_string("/sys/class/net/wlan0/address").unwrap_or_default().trim().replace(':', "").to_lowercase();
    let mut found = analyze(&results[0], &results[1], &results[2], &results[3], &results[4], &results[5], &mac);
    let c = &results[6];
    if c["latitude"].is_number() {
        found["location"] = json!({"latitude": c["latitude"], "longitude": c["longitude"]});
    }
    if found != *cached {
        let path = format!("{STATE_DIR}/detected.json");
        std::fs::write(&path, serde_json::to_string_pretty(&found)?)?;
        log("Home Assistant has changed what the components use, restarting");
        std::process::exit(0);
    }
    Ok(())
}

fn title(s: &str) -> String {
    let s = s.replace('_', " ");
    let mut c = s.chars();
    c.next().map_or(String::new(), |f| f.to_uppercase().chain(c).collect())
}

pub fn analyze(entities: &Value, devices: &Value, areas: &Value, states: &Value, services: &Value, pipelines: &Value, mac: &str) -> Value {
    let list = |v: &Value| v.as_array().cloned().unwrap_or_default();
    let (entities, devices, areas, states) = (list(entities), list(devices), list(areas), list(states));
    let device_area: BTreeMap<&str, &str> = devices.iter().filter_map(|d| Some((d["id"].as_str()?, d["area_id"].as_str()?))).collect();
    let area_name: BTreeMap<&str, &str> = areas.iter().filter_map(|a| Some((a["area_id"].as_str()?, a["name"].as_str()?))).collect();
    let usable: Vec<&Value> = entities.iter().filter(|e| e["disabled_by"].is_null() && e["hidden_by"].is_null()).collect();
    let state = |id: &str| states.iter().find(|s| s["entity_id"] == id);
    let ids = |domain: &str| {
        let mut v: Vec<&str> = states.iter().filter_map(|s| s["entity_id"].as_str()).filter(|e| e.starts_with(&format!("{domain}."))).collect();
        v.sort();
        v
    };
    let mut out = Map::new();

    let mut by_area: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for e in usable.iter().filter(|e| e["entity_id"].as_str().is_some_and(|id| id.starts_with("light.")) && e["entity_category"].is_null()) {
        let area = e["area_id"].as_str().or_else(|| device_area.get(e["device_id"].as_str()?).copied());
        if let (Some(area), Some(id)) = (area.filter(|a| area_name.contains_key(a)), e["entity_id"].as_str()) {
            by_area.entry(area).or_default().push(id);
        }
    }
    let mut lights: Vec<Value> = by_area
        .iter()
        .take(LIGHT_GROUPS)
        .map(|(area, ids)| json!({"id": area, "name": area_name[area], "entities": ids}))
        .collect();
    if !lights.is_empty() {
        let all: Vec<&str> = by_area.values().flatten().copied().collect();
        lights.push(json!({"id": "all_off", "name": "All off", "off_only": true, "entities": all}));
    }
    out.insert("lights".into(), json!(lights));

    out.insert("climate".into(), json!(ids("climate").first()));
    out.insert("weather".into(), json!(ids("weather").first()));
    out.insert("calendars".into(), json!(ids("calendar")));
    out.insert("assist".into(), json!(pipelines["pipelines"].as_array().is_some_and(|p| !p.is_empty())));

    let spotify = usable.iter().find(|e| e["platform"] == "spotify" && e["entity_id"].as_str().is_some_and(|id| id.starts_with("media_player.")));
    out.insert(
        "spotify".into(),
        spotify.and_then(|e| e["entity_id"].as_str()).map_or(Value::Null, |id| {
            json!({"entity": id, "sources": state(id).map_or(Value::Null, |s| s["attributes"]["source_list"].clone()),
                "recent": services["spotify"]["recently_played"].is_object()})
        }),
    );

    out.insert("doorbell".into(), doorbell(&usable).unwrap_or_default());

    let mut units: BTreeMap<String, Value> = BTreeMap::new();
    let mut own = None;
    for e in usable.iter().filter(|e| e["platform"] == "mqtt") {
        let Some(rest) = e["unique_id"].as_str().and_then(|u| u.strip_prefix("open_cgs2_")) else { continue };
        let Some((unit_mac, key)) = rest.split_once('_') else { continue };
        if unit_mac == mac {
            own = own.or(e["device_id"].as_str());
            continue;
        }
        let device = devices.iter().find(|d| d["id"] == e["device_id"]);
        let name = device.and_then(|d| d["name_by_user"].as_str().or(d["name"].as_str())).unwrap_or(unit_mac);
        let unit = units.entry(unit_mac.to_string()).or_insert_with(|| json!({"name": name, "entities": {}}));
        unit["entities"][key] = e["entity_id"].clone();
    }
    out.insert("units".into(), json!(units.into_values().collect::<Vec<_>>()));
    out.insert("device_id".into(), json!(own));
    Value::Object(out)
}

/// A doorbell's ring (an `event` entity of the doorbell class, or a visitor sensor), the cameras
/// and motion sensors on the same device, most specific first.
fn doorbell(entities: &[&Value]) -> Option<Value> {
    let ring = entities.iter().find(|e| {
        let id = e["entity_id"].as_str().unwrap_or_default();
        (id.starts_with("event.") && e["original_device_class"] == "doorbell") || (id.starts_with("binary_sensor.") && e["translation_key"] == "visitor")
    })?;
    let device = ring["device_id"].as_str()?;
    let on_device = |prefix: &'static str| entities.iter().filter(move |e| e["device_id"] == device && e["entity_id"].as_str().is_some_and(|id| id.starts_with(prefix)));
    let camera = on_device("camera.").find_map(|e| e["entity_id"].as_str())?;
    const ORDER: [&str; 6] = ["person", "package", "vehicle", "pet", "animal", "motion"];
    let mut motion: Vec<(usize, Value)> = on_device("binary_sensor.")
        .filter_map(|e| {
            let kind = e["translation_key"].as_str().or(e["original_device_class"].as_str())?;
            let rank = ORDER.iter().position(|k| *k == kind || (*k == "motion" && kind == "occupancy"))?;
            Some((rank, json!({"entity": e["entity_id"], "label": title(kind)})))
        })
        .collect();
    motion.sort_by_key(|(r, _)| *r);
    Some(json!({"name": "Doorbell", "camera": camera, "ring": ring["entity_id"], "motion": motion.into_iter().map(|(_, m)| m).collect::<Vec<_>>()}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_what_components_use() {
        let entities = json!([
            {"entity_id": "light.desk", "platform": "hue", "device_id": "d1"},
            {"entity_id": "light.ceiling", "platform": "hue", "area_id": "kitchen"},
            {"entity_id": "light.hidden", "platform": "hue", "area_id": "kitchen", "hidden_by": "user"},
            {"entity_id": "light.nowhere", "platform": "hue"},
            {"entity_id": "media_player.spotify_me", "platform": "spotify"},
            {"entity_id": "binary_sensor.door_visitor", "platform": "reolink", "device_id": "bell", "translation_key": "visitor"},
            {"entity_id": "binary_sensor.door_motion", "platform": "reolink", "device_id": "bell", "original_device_class": "motion"},
            {"entity_id": "binary_sensor.door_person", "platform": "reolink", "device_id": "bell", "translation_key": "person"},
            {"entity_id": "camera.door", "platform": "reolink", "device_id": "bell"},
            {"entity_id": "sensor.other_co2", "platform": "mqtt", "unique_id": "open_cgs2_bbbb_co2", "device_id": "u2"},
            {"entity_id": "sensor.me_co2", "platform": "mqtt", "unique_id": "open_cgs2_aaaa_co2", "device_id": "u1"}
        ]);
        let devices = json!([{"id": "d1", "area_id": "office"}, {"id": "u2", "name": "Bedroom"}, {"id": "u1"}]);
        let areas = json!([{"area_id": "office", "name": "Office"}, {"area_id": "kitchen", "name": "Kitchen"}]);
        let states = json!([
            {"entity_id": "climate.hall", "state": "heat"},
            {"entity_id": "media_player.spotify_me", "state": "idle", "attributes": {"source_list": ["Kitchen"]}},
            {"entity_id": "calendar.work", "state": "off"}
        ]);
        let services = json!({"spotify": {}});
        let d = analyze(&entities, &devices, &areas, &states, &services, &json!({"pipelines": [{}]}), "aaaa");
        assert_eq!(d["lights"][0], json!({"id": "kitchen", "name": "Kitchen", "entities": ["light.ceiling"]}));
        assert_eq!(d["lights"][1]["entities"], json!(["light.desk"]));
        assert_eq!(d["lights"][2]["entities"], json!(["light.ceiling", "light.desk"]));
        assert_eq!(d["climate"], "climate.hall");
        assert_eq!(d["calendars"], json!(["calendar.work"]));
        assert_eq!(d["spotify"], json!({"entity": "media_player.spotify_me", "sources": ["Kitchen"], "recent": false}));
        assert_eq!(d["doorbell"]["ring"], "binary_sensor.door_visitor");
        assert_eq!(d["doorbell"]["motion"][0]["label"], "Person");
        assert_eq!(d["units"], json!([{"name": "Bedroom", "entities": {"co2": "sensor.other_co2"}}]));
        assert_eq!(d["device_id"], "u1");
        assert_eq!(d["assist"], true);
        assert!(d["weather"].is_null());
    }
}
