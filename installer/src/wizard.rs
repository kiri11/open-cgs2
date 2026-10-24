//! The step-by-step setup: find a device, then install, change settings, read logs or uninstall.
use crate::device::{self, Device};
use crate::{Bundle, Options};
use dialoguer::{Confirm, Input, MultiSelect, Password, Select};
use serde_json::{json, Value};
use std::path::PathBuf;

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

fn ask(prompt: &str, default: &str) -> Result<String, String> {
    Input::<String>::new().with_prompt(prompt).default(default.into()).allow_empty(true).interact_text().map_err(err)
}

fn yes(prompt: &str, default: bool) -> Result<bool, String> {
    Confirm::new().with_prompt(prompt).default(default).interact().map_err(err)
}

/// The household key shared by every unit this PC installs, so their intercoms find each other.
fn household_key() -> Result<String, String> {
    let dir = std::env::var_os("APPDATA")
        .map(|d| PathBuf::from(d).join("open-cgs2"))
        .or_else(|| std::env::home_dir().map(|h| h.join(".config/open-cgs2")))
        .ok_or("no home directory")?;
    let path = dir.join("household.key");
    if let Ok(k) = std::fs::read_to_string(&path) {
        return Ok(k.trim().to_string());
    }
    let mut bytes = [0u8; 16];
    rsa::rand_core::RngCore::fill_bytes(&mut rsa::rand_core::OsRng, &mut bytes);
    let key: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    std::fs::create_dir_all(&dir).map_err(err)?;
    std::fs::write(&path, &key).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(key)
}

fn pick_device() -> Result<Device, String> {
    println!("Looking for devices with ADB on this network…");
    let mut found = device::scan();
    found.push("Another address…".into());
    let i = Select::new().with_prompt("Which CGS2?").items(&found).default(0).interact().map_err(err)?;
    let host = if i == found.len() - 1 { ask("IP address (shown in Settings → Wi-Fi on the device)", "")? } else { found[i].clone() };
    Device::connect(&host)
}

/// Asks for the settings, starting from `current`.
pub fn config(current: &Value, components: &[(String, Value)]) -> Result<Value, String> {
    let mut c = current.clone();
    let s = |v: &Value| v.as_str().unwrap_or_default().to_string();
    c["device"]["name"] = json!(ask("Name of this device (its room)", c["device"]["name"].as_str().unwrap_or("CGS2"))?);
    let area = ask("Home Assistant area (optional)", &s(&c["device"]["area"]))?;
    c["device"]["area"] = if area.is_empty() { Value::Null } else { json!(area) };

    let url = ask("Home Assistant URL, such as http://homeassistant.local:8123 (empty for none)", &s(&c["home_assistant"]["url"]))?;
    if url.is_empty() {
        c["home_assistant"] = Value::Null;
        c["mqtt"] = Value::Null;
    } else {
        c["home_assistant"]["url"] = json!(url);
        if c["home_assistant"]["token"].is_null() || !yes("Keep the current access token?", true)? {
            println!("Make a long-lived access token in Home Assistant (your profile → Security), ideally for a non-admin user.");
            c["home_assistant"]["token"] = json!(Password::new().with_prompt("Access token").interact().map_err(err)?);
        }
        let broker = ask("MQTT broker for the readings (empty to skip)", &s(&c["mqtt"]["host"]))?;
        if broker.is_empty() {
            c["mqtt"] = Value::Null;
        } else {
            c["mqtt"]["host"] = json!(broker);
            c["mqtt"]["username"] = json!(ask("MQTT username", &s(&c["mqtt"]["username"]))?);
            if c["mqtt"]["password"].is_null() || !yes("Keep the current MQTT password?", true)? {
                c["mqtt"]["password"] = json!(Password::new().with_prompt("MQTT password").allow_empty_password(true).interact().map_err(err)?);
            }
        }
    }

    let units = ["Celsius", "Fahrenheit"];
    let f = Select::new().with_prompt("Temperature").items(&units).default((c["units"]["temperature"] == "F") as usize).interact().map_err(err)?;
    c["units"]["temperature"] = json!(if f == 1 { "F" } else { "C" });
    let clocks = ["24-hour", "12-hour"];
    let h = Select::new().with_prompt("Clock").items(&clocks).default((c["units"]["clock"] == "12h") as usize).interact().map_err(err)?;
    c["units"]["clock"] = json!(if h == 1 { "12h" } else { "24h" });

    let hint = if c["home_assistant"].is_null() { "empty for no weather" } else { "empty to use Home Assistant's" };
    let lat = ask(&format!("Latitude for the weather ({hint})"), &c["location"]["latitude"].as_f64().map_or(String::new(), |v| v.to_string()))?;
    if lat.is_empty() {
        c["location"] = Value::Null;
    } else {
        let lon = ask("Longitude", &c["location"]["longitude"].as_f64().map_or(String::new(), |v| v.to_string()))?;
        let num = |t: &str| t.trim().parse::<f64>().map_err(|_| format!("not a number: {t}"));
        c["location"] = json!({"latitude": num(&lat)?, "longitude": num(&lon)?});
    }

    let labels: Vec<String> = components
        .iter()
        .map(|(name, m)| {
            let how = match &m["default"] {
                Value::String(a) if a == "auto" => " (when Home Assistant has it)",
                _ => "",
            };
            format!("{}{how}", m["title"].as_str().unwrap_or(name))
        })
        .collect();
    let on = |name: &str, m: &Value| match &c["components"][name] {
        Value::Bool(b) => *b,
        o => !matches!(o.get("enabled").unwrap_or(&m["default"]), Value::Bool(false)),
    };
    let defaults: Vec<bool> = components.iter().map(|(n, m)| on(n, m)).collect();
    let chosen = MultiSelect::new().with_prompt("Components (space to toggle, enter to accept)").items(&labels).defaults(&defaults).interact().map_err(err)?;
    for (i, (name, m)) in components.iter().enumerate() {
        let enabled = chosen.contains(&i);
        let setting = if enabled && m["default"] == "auto" { json!("auto") } else { json!(enabled) };
        if !c["components"][name].is_object() {
            c["components"][name] = json!({});
        }
        c["components"][name]["enabled"] = setting;
    }
    if chosen.iter().any(|i| components[*i].0 == "path") {
        c["components"]["path"]["station"] = json!(ask("PATH station code (GRV, EXP, HOB, JSQ, NWK, WTC, 33S, …)", c["components"]["path"]["station"].as_str().unwrap_or("GRV"))?);
    }
    c["components"]["intercom"]["key"] = json!(household_key()?);
    Ok(c)
}

pub fn run() -> Result<(), String> {
    println!("open-cgs2 setup {}\n", crate::VERSION);
    println!("On the device: Settings → About → tap Device Name 7 times → Developer options → turn on ADB.\n");
    let mut d = pick_device()?;
    crate::status(&mut d)?;
    let actions = ["Install or reinstall", "Update the device files", "Change settings", "Show logs", "Uninstall"];
    match Select::new().with_prompt("What now?").items(&actions).default(0).interact().map_err(err)? {
        0 => {
            let bundle = Bundle::load(None)?;
            let current = crate::device_config(&mut d);
            let config = if current.as_object().is_some_and(|o| !o.is_empty()) && yes("Keep the device's current settings?", true)? {
                None
            } else {
                Some(config(&current, &bundle.components())?)
            };
            let read_only = crate::Layout::detect(&mut d)?.read_only;
            let lan_only = !read_only && yes("LAN only (no internet for the device; weather and PATH then need a proxy)?", false)?;
            let open_adb = read_only || !yes("Let ADB accept only this PC from now on?", true)?;
            let ssh = std::env::home_dir().map(|h| h.join(".ssh")).and_then(|dir| {
                ["id_ecdsa.pub", "id_rsa.pub"].iter().map(|f| dir.join(f)).find(|p| p.exists())
            });
            let ssh_key = match ssh {
                Some(p) if yes(&format!("Allow SSH logins with {}?", p.display()), true)? => std::fs::read_to_string(p).ok(),
                _ => None,
            };
            crate::install(&mut d, &bundle, &Options { config, lan_only, open_adb, ssh_key })?;
            if yes("Restart the device now?", true)? {
                let _ = d.sh("(sleep 1; reboot) >/dev/null 2>&1 &");
                println!("Restarting; open-cgs2 starts in about a minute.");
            }
            Ok(())
        }
        1 => crate::update(&mut d, &Bundle::load(None)?),
        2 => {
            let current = crate::device_config(&mut d);
            let config = config(&current, &Bundle::load(None)?.components())?;
            crate::push_config(&mut d, &config)
        }
        3 => crate::logs(&mut d, "panel"),
        _ => {
            if yes("Remove open-cgs2 and go back to the stock firmware?", false)? {
                let purge = yes("Also delete its settings and sensor history?", false)?;
                crate::uninstall(&mut d, purge)?;
                if yes("Restart the device now?", true)? {
                    let _ = d.sh("(sleep 1; reboot) >/dev/null 2>&1 &");
                }
            }
            Ok(())
        }
    }
}
