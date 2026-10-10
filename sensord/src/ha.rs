//! Readings to Home Assistant over MQTT discovery: one device with a sensor per reading, fed from
//! one JSON state topic every `mqtt.interval` seconds (15 by default) and available while the
//! retained status, which the broker sets to "offline" as the last will, says "online". A reading
//! that is missing (the particulate sensor at night) shows as unknown.
use crate::{battery, log, mqtt, read, status, Shared};
use serde_json::{json, Value};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

const KEEPALIVE: u16 = 30;
const VERSION: &str = match option_env!("OPEN_CGS2_VERSION") {
    Some(v) => v,
    None => "dev",
};

/// Entity key, the reading in the state message, name, unit and device class.
const SENSORS: [(&str, &str, &str, Option<&str>, Option<&str>); 8] = [
    ("temperature", "temperature", "Temperature", Some("°C"), Some("temperature")),
    ("humidity", "humidity", "Humidity", Some("%"), Some("humidity")),
    ("co2", "co2", "CO2", Some("ppm"), Some("carbon_dioxide")),
    ("etvoc", "tvoc", "eTVOC", None, None),
    ("pm25", "pm25", "PM2.5", Some("µg/m³"), Some("pm25")),
    ("pm10", "pm10", "PM10", Some("µg/m³"), Some("pm10")),
    ("illuminance", "illuminance", "Illuminance", Some("lx"), Some("illuminance")),
    ("battery", "battery", "Battery", Some("%"), Some("battery")),
];

struct Broker {
    host: String,
    port: u16,
    username: String,
    password: String,
    interval: u64,
    prefix: String,
    /// Wi-Fi MAC address in lower-case hex, which identifies the device.
    id: String,
    name: String,
    area: Option<String>,
}

impl Broker {
    fn load(config: &Value) -> Option<Broker> {
        let m = &config["mqtt"];
        let s = |v: &Value| v.as_str().map(String::from);
        Some(Broker {
            host: s(&m["host"]).filter(|h| !h.is_empty())?,
            port: m["port"].as_u64().unwrap_or(1883) as u16,
            username: s(&m["username"]).unwrap_or_default(),
            password: s(&m["password"]).unwrap_or_default(),
            interval: m["interval"].as_u64().unwrap_or(15).max(2),
            prefix: s(&m["discovery_prefix"]).unwrap_or_else(|| "homeassistant".into()),
            id: read("/sys/class/net/wlan0/address").unwrap_or_default().replace(':', "").to_lowercase(),
            name: s(&config["device"]["name"]).unwrap_or_else(|| "CGS2".into()),
            area: s(&config["device"]["area"]).filter(|a| !a.is_empty()),
        })
    }

    fn topic(&self, leaf: &str) -> String {
        format!("open-cgs2/{}/{leaf}", self.id)
    }

    /// Each entity's discovery topic and config.
    fn discovery(&self) -> Vec<(String, Value)> {
        let slug: String = self.name.to_lowercase().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect();
        let mut device = json!({"identifiers": [format!("open_cgs2_{}", self.id)], "name": self.name, "manufacturer": "Qingping",
            "model": "Air Monitor 2 (CGS2)", "sw_version": VERSION});
        if let Some(a) = &self.area {
            device["suggested_area"] = json!(a);
        }
        let entity = |kind: &str, key: &str, name: &str| {
            let c = json!({"name": name, "unique_id": format!("open_cgs2_{}_{key}", self.id), "state_topic": self.topic("state"),
                "availability_topic": self.topic("status"), "device": device, "default_entity_id": format!("{kind}.{slug}_{key}")});
            (format!("{}/{kind}/open_cgs2_{}_{key}/config", self.prefix, self.id), c)
        };
        let mut out: Vec<(String, Value)> = SENSORS
            .iter()
            .map(|(key, field, name, unit, class)| {
                let (topic, mut c) = entity("sensor", key, name);
                c["value_template"] = json!(format!("{{{{ value_json.{field} }}}}"));
                c["state_class"] = json!("measurement");
                if let Some(u) = unit {
                    c["unit_of_measurement"] = json!(u);
                }
                if let Some(d) = class {
                    c["device_class"] = json!(d);
                }
                (topic, c)
            })
            .collect();
        let (topic, mut charging) = entity("binary_sensor", "charging", "Charging");
        charging["value_template"] = json!("{{ 'ON' if value_json.charging else 'OFF' }}");
        charging["device_class"] = json!("battery_charging");
        out.push((topic, charging));
        out
    }
}

fn session(b: &Broker, shared: &Shared) -> std::io::Result<()> {
    let status_topic = b.topic("status");
    let mut c = mqtt::Client::connect(&mqtt::Options {
        host: &b.host,
        port: b.port,
        client_id: &format!("open-cgs2-{}", b.id),
        username: &b.username,
        password: &b.password,
        will: (&status_topic, "offline"),
        keepalive: KEEPALIVE,
    })?;
    for (topic, config) in b.discovery() {
        c.publish(&topic, config.to_string().as_bytes(), true)?;
    }
    c.publish(&status_topic, b"online", true)?;
    log(format!("mqtt: connected to {}:{}", b.host, b.port));
    let mut next = Instant::now();
    loop {
        c.poll()?;
        if Instant::now() >= next {
            let state = status(&shared.readings.lock().unwrap(), &battery());
            c.publish(&b.topic("state"), state.to_string().as_bytes(), false)?;
            next += Duration::from_secs(b.interval);
            if next < Instant::now() {
                next = Instant::now() + Duration::from_secs(b.interval);
            }
        }
    }
}

pub fn run(shared: Arc<Shared>) {
    let Some(broker) = Broker::load(&shared.config) else {
        return log("no MQTT broker in config.json, not publishing to Home Assistant");
    };
    let mut wait = 5;
    loop {
        let started = Instant::now();
        if let Err(e) = session(&broker, &shared) {
            log(format!("mqtt: {e}"));
        }
        if started.elapsed() > Duration::from_secs(120) {
            wait = 5;
        }
        thread::sleep(Duration::from_secs(wait));
        wait = (wait * 2).min(60);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn describes_every_reading() {
        let config = json!({"mqtt": {"host": "broker"}, "device": {"name": "Living Room", "area": "Living Room"}});
        let mut b = Broker::load(&config).unwrap();
        b.id = "000000000001".into();
        let d = b.discovery();
        assert_eq!(d.len(), 9);
        let (topic, co2) = &d[2];
        assert_eq!(topic, "homeassistant/sensor/open_cgs2_000000000001_co2/config");
        assert_eq!(co2["default_entity_id"], "sensor.living_room_co2");
        assert_eq!(co2["value_template"], "{{ value_json.co2 }}");
        assert_eq!(co2["device"]["suggested_area"], "Living Room");
        assert_eq!(d[8].1["default_entity_id"], "binary_sensor.living_room_charging");
        assert!(Broker::load(&json!({})).is_none());
    }
}
