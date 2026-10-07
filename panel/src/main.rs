//! Backend for the CGS2 screen: keeps a Home Assistant WebSocket session, runs push-to-talk
//! Assist, and serves a JSON snapshot to the QML front end on 127.0.0.1 (long-poll).
mod audio;
mod call;
mod clock;
mod config;
mod detect;
mod doorbell;
mod fetch;
mod ha;
mod history;
mod intercom;
mod lights;
mod local;
mod pm;
mod screen;
mod settings;
mod weather;

pub use config::{Chart, Config, Group, Music, Series, Speaker, Weather};

use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{mpsc, Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tiny_http::{Header, Method, Response, Server};

pub fn log(msg: impl std::fmt::Display) {
    let t = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs()) % 86400;
    eprintln!("{:02}:{:02}:{:02} {msg}", t / 3600, t / 60 % 60, t % 60);
}

#[derive(Default)]
pub struct State {
    pub version: u64,
    pub connected: bool,
    pub entities: HashMap<String, String>,
    pub attrs: HashMap<String, serde_json::Map<String, Value>>,
    pub changed: HashMap<String, f64>,
    /// When each entity last changed state or attributes, in seconds since the epoch.
    pub updated: HashMap<String, f64>,
    pub music: ha::music::MusicState,
    /// Expected states after a button press, shown until HA reports them or they expire.
    pub overrides: HashMap<String, (String, Instant)>,
    /// Whether the backlight is on.
    pub screen: bool,
    /// The power key was held: the screen asks whether to power off.
    pub power_menu: bool,
    pub phase: &'static str,
    pub heard: String,
    pub reply: String,
    pub error: String,
    /// Trimmed `hourly` and `daily` forecasts.
    pub forecast: serde_json::Map<String, Value>,
    /// Current outside readings from the weather source: `outside`, `outside_rh`, `sky`, `rain_soon`.
    pub weather: serde_json::Map<String, Value>,
    /// Events of the calendars in the next 24 hours, soonest first.
    pub calendar: Vec<Value>,
    /// sensord's particulate sensor status.
    pub pm: Value,
    /// This device's own readings from sensord, by sensor key.
    pub local: serde_json::Map<String, Value>,
    pub clock: clock::Clock,
    pub intercom: intercom::Status,
    pub doorbell: doorbell::Status,
    pub call: call::Status,
}

impl State {
    fn effective(&self, id: &str) -> Option<&str> {
        self.overrides.get(id).map(|(s, _)| s.as_str()).or_else(|| self.entities.get(id).map(String::as_str))
    }
}

/// Whether any entity in the group is on, and the entities that are currently reachable.
pub fn group_status(s: &State, g: &Group) -> (bool, Vec<String>) {
    let on = g.entities.iter().any(|e| s.effective(e) == Some("on"));
    let available = g
        .entities
        .iter()
        .filter(|e| matches!(s.effective(e), Some(x) if x != "unavailable" && x != "unknown"))
        .cloned()
        .collect();
    (on, available)
}

pub struct Shared {
    pub cfg: Config,
    state: Mutex<State>,
    changed: Condvar,
}

impl Shared {
    pub fn read<R>(&self, f: impl FnOnce(&State) -> R) -> R {
        f(&self.state.lock().unwrap())
    }

    pub fn update(&self, f: impl FnOnce(&mut State)) {
        let mut s = self.state.lock().unwrap();
        f(&mut s);
        s.version += 1;
        self.changed.notify_all();
    }

    fn snapshot(&self, s: &State) -> Value {
        let mut sensors = s.local.clone();
        sensors.extend(s.weather.clone());
        if let Weather::Entity(e) = &self.cfg.weather {
            let attr = |k: &str| s.attrs.get(e).and_then(|a| a.get(k)).cloned().unwrap_or_default();
            sensors.insert("outside".into(), attr("temperature"));
            sensors.insert("outside_rh".into(), attr("humidity"));
            sensors.insert("sky".into(), json!(s.entities.get(e)));
        }
        sensors.extend(self.cfg.sensors.iter().map(|(k, e)| (k.clone(), json!(s.entities.get(e)))));
        let entities: serde_json::Map<String, Value> = self
            .cfg
            .follow
            .iter()
            .map(|e| (e.clone(), json!({"state": s.entities.get(e), "attributes": s.attrs.get(e)})))
            .collect();
        let groups: Vec<Value> = self
            .cfg
            .groups
            .iter()
            .map(|g| {
                let (on, available) = group_status(s, g);
                let light = lights::appearance(s, g);
                json!({"id": g.id, "name": g.name, "on": on, "available": !available.is_empty(), "off_only": g.off_only,
                    "rgb": light.rgb, "brightness": light.brightness, "dimmable": !lights::targets(s, g).is_empty()})
            })
            .collect();
        json!({
            "v": s.version,
            "name": self.cfg.name,
            "units": {"temperature": if self.cfg.fahrenheit { "F" } else { "C" }, "clock": if self.cfg.clock24 { "24h" } else { "12h" }},
            "components": self.cfg.components,
            "pages": self.cfg.pages,
            "location": self.cfg.location,
            "entities": entities,
            "ha": s.connected,
            "screen": s.screen,
            "power_menu": s.power_menu,
            "sensors": sensors,
            "groups": groups,
            "assist": {"phase": s.phase, "heard": s.heard, "reply": s.reply, "error": s.error},
            "music": ha::music::snapshot(&self.cfg, s),
            "climate": ha::climate::snapshot(&self.cfg, s),
            "calendar": s.calendar,
            "pm": s.pm,
            "clock": clock::snapshot(&s.clock),
            "intercom": intercom::snapshot(&s.intercom),
            "doorbell": doorbell::snapshot(&self.cfg, s),
            "call": call::snapshot(&s.call),
        })
    }
}

fn json_response(body: String) -> Response<std::io::Cursor<Vec<u8>>> {
    Response::from_string(body).with_header(Header::from_bytes("Content-Type", "application/json").unwrap())
}

fn handle(mut req: tiny_http::Request, shared: &Shared, tx: &mpsc::Sender<ha::Cmd>, intercom: Option<&intercom::Service>) {
    let url = req.url().to_string();
    let method = req.method().clone();
    let (path, query) = url.split_once('?').unwrap_or((url.as_str(), ""));
    let cmd = match (&method, path) {
        (Method::Get, "/state") => {
            let since: u64 = query.strip_prefix("v=").and_then(|v| v.parse().ok()).unwrap_or(0);
            let s = shared.state.lock().unwrap();
            let (s, _) = shared.changed.wait_timeout_while(s, Duration::from_secs(25), |s| s.version == since).unwrap();
            let body = shared.snapshot(&s).to_string();
            drop(s);
            let _ = req.respond(json_response(body));
            return;
        }
        (Method::Get, "/library") => {
            let body = shared.read(|s| ha::music::library(s, shared.cfg.music.as_ref())).to_string();
            let _ = req.respond(json_response(body));
            return;
        }
        (Method::Get, "/forecast") => {
            let body = shared.read(|s| Value::Object(s.forecast.clone())).to_string();
            let _ = req.respond(json_response(body));
            return;
        }
        (Method::Get, p) if p.starts_with("/history/") => {
            let range = query.strip_prefix("range=").unwrap_or("3h");
            let _ = match history::chart(shared, tx, &p["/history/".len()..], range) {
                Some(v) => req.respond(json_response(v.to_string())),
                None => req.respond(Response::empty(404)),
            };
            return;
        }
        (Method::Get, "/settings") => {
            let mut v = settings::info(&shared.cfg);
            v["screen"] = screen::prefs().json();
            let _ = req.respond(json_response(v.to_string()));
            return;
        }
        (Method::Get, "/fetch") => {
            let url = query.strip_prefix("url=").map(fetch::decode).unwrap_or_default();
            let _ = match fetch::get(&url) {
                Ok(body) => req.respond(Response::from_data(body)),
                Err(e) => req.respond(json_response(json!({"error": e}).to_string()).with_status_code(502)),
            };
            return;
        }
        (Method::Post, "/calibration") => {
            let mut body = String::new();
            let _ = std::io::Read::read_to_string(req.as_reader(), &mut body);
            let ok = serde_json::from_str(&body).is_ok_and(|v| settings::set_calibration(&v));
            let _ = req.respond(if ok { json_response("{}".into()) } else { json_response("{}".into()).with_status_code(400) });
            return;
        }
        (Method::Post, p) if p.starts_with("/ha/call/") => {
            let mut body = String::new();
            let _ = std::io::Read::read_to_string(req.as_reader(), &mut body);
            let v: Value = serde_json::from_str(&body).unwrap_or_default();
            let Some((domain, service)) = p["/ha/call/".len()..].split_once('/') else {
                let _ = req.respond(Response::empty(400));
                return;
            };
            let mut msg = json!({"type": "call_service", "domain": domain, "service": service, "service_data": v["data"]});
            if !v["target"].is_null() {
                msg["target"] = v["target"].clone();
            }
            ha::Cmd::Call(msg)
        }
        (Method::Get, "/clock/tones") => {
            let _ = req.respond(json_response(clock::tones().to_string()));
            return;
        }
        (Method::Post, p) if p.starts_with("/clock/") => {
            let mut body = String::new();
            let _ = std::io::Read::read_to_string(req.as_reader(), &mut body);
            let v: Value = serde_json::from_str(&body).unwrap_or_default();
            let ok = clock::command(shared, &p["/clock/".len()..], &v);
            let _ = req.respond(if ok { json_response("{}".into()) } else { json_response("{}".into()).with_status_code(400) });
            return;
        }
        (Method::Get, "/wifi/scan") => {
            let _ = req.respond(json_response(settings::scan().to_string()));
            return;
        }
        (Method::Post, "/wifi/connect") | (Method::Post, "/screen/prefs") => {
            let mut body = String::new();
            let _ = std::io::Read::read_to_string(req.as_reader(), &mut body);
            let v: Value = serde_json::from_str(&body).unwrap_or_default();
            let res = if path == "/screen/prefs" {
                screen::set_prefs(&v);
                Ok(())
            } else {
                settings::connect(v["ssid"].as_str().unwrap_or_default(), v["password"].as_str().unwrap_or_default())
            };
            let _ = match res {
                Ok(()) => req.respond(json_response("{}".into())),
                Err(e) => req.respond(json_response(json!({"error": e}).to_string()).with_status_code(400)),
            };
            return;
        }
        (Method::Post, p) if p.starts_with("/co2/calibrate/") => {
            let ok = p["/co2/calibrate/".len()..].parse().is_ok_and(settings::calibrate);
            let _ = req.respond(if ok { json_response("{}".into()) } else { json_response("{}".into()).with_status_code(400) });
            return;
        }
        (Method::Post, "/stock") => {
            let _ = req.respond(if settings::stock_once() { json_response("{}".into()) } else { json_response("{}".into()).with_status_code(500) });
            return;
        }
        (Method::Post, "/doorbell/dismiss") => {
            doorbell::dismiss(shared);
            let _ = req.respond(json_response("{}".into()));
            return;
        }
        (Method::Post, "/call/dismiss") => {
            call::dismiss(shared);
            let _ = req.respond(json_response("{}".into()));
            return;
        }
        (Method::Post, "/screen/wake") => {
            screen::set(shared, true, "tap");
            let _ = req.respond(json_response("{}".into()));
            return;
        }
        (Method::Post, p) if p.starts_with("/power/") => {
            let _ = if screen::power(shared, &p["/power/".len()..]) { req.respond(json_response("{}".into())) } else { req.respond(Response::empty(400)) };
            return;
        }
        (Method::Post, p) if p.starts_with("/pm/") => {
            let ok = match p["/pm/".len()..].split_once('/') {
                Some((what @ ("power" | "night"), state @ ("on" | "off"))) => pm::set(shared, what, state == "on"),
                Some(("window", w)) => w.split_once('-').is_some_and(|(from, until)| pm::window(shared, from, until)),
                _ => false,
            };
            let _ = if ok { req.respond(json_response("{}".into())) } else { req.respond(Response::empty(400)) };
            return;
        }
        (Method::Post, path @ ("/intercom/talk" | "/intercom/release")) => {
            let Some(intercom) = intercom else {
                let _ = req.respond(Response::empty(404));
                return;
            };
            if path == "/intercom/talk" { intercom.talk(); } else { intercom.release(); }
            let _ = req.respond(json_response("{}".into()));
            return;
        }
        (Method::Post, "/assist") => ha::Cmd::Assist,
        (Method::Post, "/dismiss") => ha::Cmd::Dismiss,
        (Method::Post, p) if p.starts_with("/brightness/") => {
            let parsed = p["/brightness/".len()..].split_once('/').and_then(|(group, value)| {
                Some((group, value.parse::<u8>().ok().filter(|v| *v > 0)?))
            });
            let Some((group, value)) = parsed else {
                let _ = req.respond(Response::empty(400));
                return;
            };
            if !shared.cfg.groups.iter().any(|g| g.id == group && !g.off_only) {
                let _ = req.respond(Response::empty(404));
                return;
            }
            ha::Cmd::Brightness(group.to_string(), value)
        }
        (Method::Post, p) if p.starts_with("/press/") => {
            let group = &p["/press/".len()..];
            ha::Cmd::Press(group.to_string())
        }
        (Method::Post, p) if p.starts_with("/music/") => {
            let (action, arg) = p["/music/".len()..].split_once('/').unwrap_or((&p["/music/".len()..], ""));
            ha::Cmd::Music(action.to_string(), arg.to_string())
        }
        (Method::Post, p) if p.starts_with("/climate/") => {
            let (action, arg) = p["/climate/".len()..].split_once('/').unwrap_or((&p["/climate/".len()..], ""));
            ha::Cmd::Climate(action.to_string(), arg.to_string())
        }
        _ => {
            let _ = req.respond(Response::empty(404));
            return;
        }
    };
    let _ = tx.send(cmd);
    let _ = req.respond(json_response("{}".into()));
}

pub const STATE_DIR: &str = "/userdata/open-cgs2";

fn main() {
    let path = std::env::args().nth(1).unwrap_or_else(|| format!("{STATE_DIR}/config.json"));
    let ui = std::env::var_os("OPEN_CGS2_UI").map(std::path::PathBuf::from).unwrap_or_else(|| {
        std::env::current_exe().ok().and_then(|p| Some(p.parent()?.join("ui"))).unwrap_or_default()
    });
    let cfg = Config::load(&path, &ui, std::path::Path::new(&format!("{STATE_DIR}/detected.json"))).unwrap_or_else(|e| {
        log(e);
        std::process::exit(1)
    });
    let port = cfg.port;
    let state = State { version: 1, screen: screen::lit().unwrap_or(true), phase: "idle", clock: clock::load(), ..State::default() };
    let shared = Arc::new(Shared { cfg, state: Mutex::new(state), changed: Condvar::new() });
    let (tx, rx) = mpsc::channel();
    {
        let (shared, tx) = (shared.clone(), tx.clone());
        thread::spawn(move || ha::run(shared, tx, rx));
    }
    {
        let shared = shared.clone();
        thread::spawn(move || screen::run(shared));
    }
    {
        let shared = shared.clone();
        thread::spawn(move || pm::run(shared));
    }
    {
        let shared = shared.clone();
        thread::spawn(move || local::run(shared));
    }
    {
        let shared = shared.clone();
        thread::spawn(move || clock::run(shared));
    }
    {
        let shared = shared.clone();
        thread::spawn(move || doorbell::run(shared));
    }
    {
        let (shared, tx) = (shared.clone(), tx.clone());
        thread::spawn(move || call::run(shared, tx));
    }
    if let Weather::OpenMeteo { latitude, longitude } = shared.cfg.weather {
        let shared = shared.clone();
        thread::spawn(move || weather::run(shared, latitude, longitude));
    }
    let intercom = shared.cfg.intercom.as_ref().map(|_| intercom::Service::start(shared.clone(), tx.clone()));
    let server = Server::http(("127.0.0.1", port)).unwrap_or_else(|e| {
        log(format!("cannot listen on port {port}: {e}"));
        std::process::exit(1)
    });
    log(format!("listening on 127.0.0.1:{port}"));
    for req in server.incoming_requests() {
        let (shared, tx) = (shared.clone(), tx.clone());
        let intercom = intercom.clone();
        thread::spawn(move || handle(req, &shared, &tx, intercom.as_deref()));
    }
}
