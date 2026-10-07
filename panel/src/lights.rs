//! Light appearance and brightness commands, derived from HA's subscribed attributes.
use crate::{group_status, Group, State};
use serde_json::{json, Map, Value};

pub struct Appearance {
    pub rgb: [u8; 3],
    pub brightness: u8,
}

fn dimmable(s: &State, id: &str) -> bool {
    id.starts_with("light.")
        && s.attrs.get(id).is_some_and(|a| {
            a.get("supported_color_modes").and_then(Value::as_array).is_some_and(|modes| {
                modes.iter().any(|m| {
                    matches!(
                        m.as_str(),
                        Some("brightness" | "color_temp" | "hs" | "xy" | "rgb" | "rgbw" | "rgbww" | "white")
                    )
                })
            })
        })
}

/// Adjust lit members only; when the whole group is off, a drag can turn it on.
pub fn targets(s: &State, g: &Group) -> Vec<String> {
    if g.off_only {
        return Vec::new();
    }
    let (on, available) = group_status(s, g);
    available.into_iter().filter(|id| dimmable(s, id) && (!on || s.effective(id) == Some("on"))).collect()
}

fn rgb(a: &Map<String, Value>) -> [f64; 3] {
    // HA supplies rgb_color for its HS, XY, RGBW and RGBWW modes too.
    if let Some(v) = a.get("rgb_color").and_then(Value::as_array).filter(|v| v.len() == 3) {
        if let (Some(r), Some(g), Some(b)) = (v[0].as_f64(), v[1].as_f64(), v[2].as_f64()) {
            return [r.clamp(0.0, 255.0), g.clamp(0.0, 255.0), b.clamp(0.0, 255.0)];
        }
    }
    let kelvin = a
        .get("color_temp_kelvin")
        .and_then(Value::as_f64)
        .or_else(|| a.get("color_temp").and_then(Value::as_f64).filter(|m| *m > 0.0).map(|m| 1_000_000.0 / m));
    if let Some(k) = kelvin {
        // Approximate black-body colour for white-mode lights that omit rgb_color.
        let t = k.clamp(1000.0, 40000.0) / 100.0;
        return [
            if t <= 66.0 { 255.0 } else { 329.698727446 * (t - 60.0).powf(-0.1332047592) },
            if t <= 66.0 {
                99.4708025861 * t.ln() - 161.1195681661
            } else {
                288.1221695283 * (t - 60.0).powf(-0.0755148492)
            },
            if t >= 66.0 {
                255.0
            } else if t <= 19.0 {
                0.0
            } else {
                138.5177312231 * (t - 10.0).ln() - 305.0447927307
            },
        ]
        .map(|c| c.clamp(0.0, 255.0));
    }
    [255.0; 3]
}

pub fn appearance(s: &State, g: &Group) -> Appearance {
    let mut count = 0;
    let mut brightness = 0.0;
    let mut color = [0.0; 3];
    for id in &g.entities {
        if !id.starts_with("light.") || s.effective(id) != Some("on") {
            continue;
        }
        let empty = Map::new();
        let a = s.attrs.get(id).unwrap_or(&empty);
        let b = a.get("brightness").and_then(Value::as_f64).unwrap_or(255.0).clamp(1.0, 255.0);
        for (sum, c) in color.iter_mut().zip(rgb(a)) {
            *sum += c * b;
        }
        brightness += b;
        count += 1;
    }
    if count == 0 {
        return Appearance { rgb: [243, 182, 65], brightness: 0 };
    }
    Appearance {
        rgb: color.map(|c| (c / brightness).round() as u8),
        brightness: (brightness / f64::from(count)).round() as u8,
    }
}

pub fn command(s: &State, g: &Group, brightness: u8) -> Option<Value> {
    let targets = targets(s, g);
    if targets.is_empty() || brightness == 0 {
        return None;
    }
    Some(json!({"type": "call_service", "domain": "light", "service": "turn_on",
        "target": {"entity_id": targets}, "service_data": {"brightness": brightness}}))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn group(ids: &[&str]) -> Group {
        Group {
            id: "room".into(),
            name: "Room".into(),
            entities: ids.iter().map(|s| s.to_string()).collect(),
            off_only: false,
        }
    }

    fn light(s: &mut State, id: &str, state: &str, attrs: Value) {
        s.entities.insert(id.into(), state.into());
        s.attrs.insert(id.into(), attrs.as_object().unwrap().clone());
    }

    #[test]
    fn appearance_tracks_only_lit_members_and_attribute_changes() {
        let g = group(&["light.off", "light.on"]);
        let mut s = State::default();
        light(&mut s, "light.off", "off", json!({"rgb_color": [0, 0, 255], "brightness": 255}));
        light(&mut s, "light.on", "on", json!({"rgb_color": [255, 40, 10], "brightness": 64}));
        assert_eq!(appearance(&s, &g).rgb, [255, 40, 10]);
        assert_eq!(appearance(&s, &g).brightness, 64);
        s.attrs.get_mut("light.on").unwrap().insert("brightness".into(), json!(128));
        assert_eq!(appearance(&s, &g).brightness, 128);
        s.entities.insert("light.on".into(), "off".into());
        assert_eq!(appearance(&s, &g).brightness, 0);
    }

    #[test]
    fn mixed_colors_are_weighted_by_brightness() {
        let mut s = State::default();
        light(&mut s, "light.red", "on", json!({"rgb_color": [255, 0, 0], "brightness": 200}));
        light(&mut s, "light.blue", "on", json!({"rgb_color": [0, 0, 255], "brightness": 100}));
        let a = appearance(&s, &group(&["light.red", "light.blue"]));
        assert_eq!(a.rgb, [170, 0, 85]);
        assert_eq!(a.brightness, 150);
    }

    #[test]
    fn white_modes_have_color_and_missing_attributes_are_safe() {
        let warm = rgb(json!({"color_temp_kelvin": 2700}).as_object().unwrap());
        assert!(warm[0] > warm[1] && warm[1] > warm[2]);
        let cool = rgb(json!({"color_temp_kelvin": 6500}).as_object().unwrap());
        assert!(cool[2] > warm[2]);
        let mired = rgb(json!({"color_temp": 370.37}).as_object().unwrap());
        assert!((mired[2] - warm[2]).abs() < 1.0);
        assert_eq!(rgb(&Map::new()), [255.0; 3]);
    }

    #[test]
    fn commands_preserve_color_and_do_not_turn_on_other_members() {
        let mut s = State::default();
        for (id, state) in
            [("light.on", "on"), ("light.off", "off"), ("light.lost", "unavailable"), ("switch.plant", "on")]
        {
            light(&mut s, id, state, json!({"supported_color_modes": ["rgb"], "brightness": 50}));
        }
        light(&mut s, "light.relay", "on", json!({"supported_color_modes": ["onoff"]}));
        let mut g = group(&["light.on", "light.off", "light.lost", "switch.plant", "light.relay"]);
        let cmd = command(&s, &g, 200).unwrap();
        assert_eq!(cmd["target"]["entity_id"], json!(["light.on"]));
        assert_eq!(cmd["service_data"], json!({"brightness": 200}));
        assert_eq!(cmd["domain"], "light");
        assert!(command(&s, &g, 0).is_none());
        g.off_only = true;
        assert!(command(&s, &g, 200).is_none());
    }

    #[test]
    fn off_group_can_be_dimmed_on() {
        let mut s = State::default();
        light(&mut s, "light.off", "off", json!({"supported_color_modes": ["color_temp"]}));
        assert_eq!(targets(&s, &group(&["light.off"])), vec!["light.off"]);
        assert!(targets(&s, &group(&["light.missing"])).is_empty());
    }
}
