mod calendar;
pub mod climate;
mod forecast;
pub mod music;

use crate::{audio, doorbell, group_status, lights, log, Shared, State};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::io::ErrorKind;
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tungstenite::{Message, WebSocket};

pub enum Cmd {
    Press(String),
    Brightness(String, u8),
    Assist,
    Dismiss,
    Audio(Vec<u8>),
    PlayerDone(u64),
    Music(String, String),
    /// A WebSocket request; the reply gets its result, or nothing if it fails.
    Request(Value, Sender<Value>),
    /// A service call from a component, with nothing to wait for.
    Call(Value),
    Climate(String, String),
    /// Speaks the text with the pipeline's TTS.
    Announce(String),
}

type Ws = WebSocket<TcpStream>;
type Res<T> = Result<T, Box<dyn std::error::Error>>;

const LISTEN_MAX: Duration = Duration::from_secs(15);
const NO_SPEECH_MAX: Duration = Duration::from_secs(6);
const DONE_SHOW: Duration = Duration::from_secs(20);
const ERROR_SHOW: Duration = Duration::from_secs(8);
const CONVERSATION_TTL: Duration = Duration::from_secs(300);
const OVERRIDE_TTL: Duration = Duration::from_secs(5);
const PING_EVERY: Duration = Duration::from_secs(30);
const PING_TIMEOUT: Duration = Duration::from_secs(10);
const RECONNECT: Duration = Duration::from_secs(5);
/// The weather entity can be missing for a while after HA starts.
const FORECAST_RETRY: Duration = Duration::from_secs(60);

struct Run {
    id: u64,
    token: u64,
    started: Instant,
    handler: Option<u8>,
    buffered: Vec<Vec<u8>>,
    audio_done: bool,
    speech: bool,
    tts_url: Option<String>,
    playing: bool,
    played: bool,
    ended: bool,
    cont: bool,
}

impl Run {
    fn new(id: u64, token: u64) -> Run {
        Run {
            id,
            token,
            started: Instant::now(),
            handler: None,
            buffered: Vec::new(),
            audio_done: false,
            speech: false,
            tts_url: None,
            playing: false,
            played: false,
            ended: false,
            cont: false,
        }
    }
}

struct Session {
    shared: Arc<Shared>,
    mic: audio::Mic,
    player: audio::Player,
    announcer: audio::Player,
    announce: Option<u64>,
    next_id: u64,
    sub_id: u64,
    calls: HashMap<u64, Vec<String>>,
    run: Option<Run>,
    runs: u64,
    conversation: Option<(String, Instant)>,
    phase_since: Instant,
    ping: Option<(u64, Instant)>,
    last_ping: Instant,
    browse: HashMap<u64, music::Browse>,
    music_calls: HashSet<u64>,
    chained: HashMap<u64, Vec<Value>>,
    pending: Option<music::Pending>,
    requests: HashMap<u64, Sender<Value>>,
    forecasts: HashMap<u64, &'static str>,
    forecast_retry: Option<Instant>,
    calendar_call: Option<u64>,
    calendar_due: Option<Instant>,
}

pub fn run(shared: Arc<Shared>, tx: Sender<Cmd>, rx: Receiver<Cmd>) {
    let audio_tx = tx.clone();
    let mut s = Session {
        mic: audio::Mic::new(shared.cfg.mic.clone(), move |pcm| audio_tx.send(Cmd::Audio(pcm)).is_ok()),
        announcer: audio::Player::new(tx.clone()),
        announce: None,
        player: audio::Player::new(tx),
        shared,
        next_id: 1,
        sub_id: 0,
        calls: HashMap::new(),
        run: None,
        runs: 0,
        conversation: None,
        phase_since: Instant::now(),
        ping: None,
        last_ping: Instant::now(),
        browse: HashMap::new(),
        music_calls: HashSet::new(),
        chained: HashMap::new(),
        pending: None,
        requests: HashMap::new(),
        forecasts: HashMap::new(),
        forecast_retry: None,
        calendar_call: None,
        calendar_due: None,
    };
    if s.shared.cfg.ha.is_empty() {
        while let Ok(cmd) = rx.recv() {
            s.offline(cmd);
            s.timers();
        }
        return;
    }
    let mut failing = false;
    loop {
        match s.connect() {
            Ok(mut ws) => {
                log("connected to Home Assistant");
                failing = false;
                s.shared.update(|st| st.connected = true);
                if let Err(e) = s.serve(&mut ws, &rx) {
                    log(format!("Home Assistant connection lost: {e}"));
                }
            }
            Err(e) if !failing => {
                log(format!("cannot connect to Home Assistant: {e}"));
                failing = true;
            }
            Err(_) => {}
        }
        s.disconnected();
        let until = Instant::now() + RECONNECT;
        while let Some(left) = until.checked_duration_since(Instant::now()) {
            match rx.recv_timeout(left) {
                Ok(cmd) => s.offline(cmd),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return,
            }
            s.timers();
        }
    }
}

fn expect(ws: &mut Ws, kind: &str) -> Res<()> {
    loop {
        if let Message::Text(t) = ws.read()? {
            let v: Value = serde_json::from_str(&t)?;
            if v["type"] == kind {
                return Ok(());
            }
            return Err(format!("expected {kind}, got {}", v["type"]).into());
        }
    }
}

impl Session {
    fn connect(&mut self) -> Res<Ws> {
        let shared = self.shared.clone();
        let cfg = &shared.cfg;
        if cfg.token.is_empty() {
            return Err("no access token".into());
        }
        let addr = cfg.ha.to_socket_addrs()?.next().ok_or("bad address")?;
        let stream = TcpStream::connect_timeout(&addr, Duration::from_secs(5))?;
        stream.set_nodelay(true)?;
        stream.set_read_timeout(Some(Duration::from_secs(10)))?;
        let (mut ws, _) = tungstenite::client(format!("ws://{}/api/websocket", cfg.ha), stream)?;
        expect(&mut ws, "auth_required")?;
        ws.send(Message::Text(json!({"type": "auth", "access_token": cfg.token}).to_string()))?;
        expect(&mut ws, "auth_ok")?;
        crate::detect::check(&mut ws, &cfg.detected)?;
        ws.get_ref().set_read_timeout(Some(Duration::from_millis(20)))?;
        self.next_id = crate::detect::FIRST_ID;
        self.sub_id = self.send(&mut ws, json!({"type": "subscribe_entities", "entity_ids": cfg.entity_ids()}))?;
        self.subscribe_forecasts(&mut ws)?;
        self.ping = None;
        self.last_ping = Instant::now();
        self.browse_root(&mut ws)?;
        Ok(ws)
    }

    fn serve(&mut self, ws: &mut Ws, rx: &Receiver<Cmd>) -> Res<()> {
        loop {
            loop {
                match rx.try_recv() {
                    Ok(cmd) => self.command(ws, cmd)?,
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => return Ok(()),
                }
            }
            match ws.read() {
                Ok(Message::Text(t)) => self.message(ws, &t)?,
                Ok(Message::Close(_)) => return Err("closed by server".into()),
                Ok(_) => {}
                Err(tungstenite::Error::Io(e)) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
                Err(e) => return Err(e.into()),
            }
            self.tick(ws)?;
        }
    }

    fn send(&mut self, ws: &mut Ws, mut msg: Value) -> Res<u64> {
        let id = self.next_id;
        self.next_id += 1;
        msg["id"] = json!(id);
        ws.send(Message::Text(msg.to_string()))?;
        Ok(id)
    }

    fn set_phase(&mut self, phase: &'static str, f: impl FnOnce(&mut State)) {
        self.phase_since = Instant::now();
        self.shared.update(|s| {
            s.phase = phase;
            f(s);
        });
    }

    fn disconnected(&mut self) {
        if self.run.is_some() {
            self.fail("Lost connection to Home Assistant");
        }
        self.calls.clear();
        self.browse.clear();
        self.music_calls.clear();
        self.chained.clear();
        self.forecast_retry = None;
        self.calendar_call = None;
        self.calendar_due = None;
        self.announce = None;
        self.pending = None;
        self.requests.clear();
        self.shared.update(|s| {
            s.connected = false;
            s.overrides.clear();
            s.music.expect = None;
        });
    }

    fn offline(&mut self, cmd: Cmd) {
        match cmd {
            Cmd::Assist => self.fail("Home Assistant is offline"),
            Cmd::Dismiss => self.set_phase("idle", |_| {}),
            Cmd::Announce(text) => log(format!("cannot announce {text:?}: Home Assistant is offline")),
            _ => {}
        }
    }

    fn command(&mut self, ws: &mut Ws, cmd: Cmd) -> Res<()> {
        match cmd {
            Cmd::Press(g) => self.press(ws, &g),
            Cmd::Brightness(g, value) => self.brightness(ws, &g, value),
            Cmd::Assist => match self.shared.read(|s| s.phase) {
                "listening" => self.end_audio(ws),
                "thinking" | "speaking" => self.cancel(ws),
                _ => self.start(ws),
            },
            Cmd::Dismiss => self.cancel(ws),
            Cmd::Audio(chunk) => self.audio(ws, chunk),
            Cmd::PlayerDone(token) => self.player_done(ws, token),
            Cmd::Music(action, arg) => self.music(ws, &action, &arg),
            Cmd::Request(msg, reply) => {
                let id = self.send(ws, msg)?;
                self.requests.insert(id, reply);
                Ok(())
            }
            Cmd::Call(msg) => {
                log(format!("call {}.{}", msg["domain"].as_str().unwrap_or_default(), msg["service"].as_str().unwrap_or_default()));
                self.send(ws, msg)?;
                Ok(())
            }
            Cmd::Climate(action, arg) => self.climate(ws, &action, &arg),
            Cmd::Announce(text) => {
                let mut msg = json!({"type": "assist_pipeline/run", "start_stage": "tts", "end_stage": "tts", "input": {"text": text}});
                if let Some(p) = &self.shared.cfg.pipeline {
                    msg["pipeline"] = json!(p);
                }
                self.announce = Some(self.send(ws, msg)?);
                Ok(())
            }
        }
    }

    fn message(&mut self, ws: &mut Ws, text: &str) -> Res<()> {
        let v: Value = serde_json::from_str(text)?;
        let id = v["id"].as_u64().unwrap_or(0);
        let is_run = self.run.as_ref().is_some_and(|r| r.id == id);
        match v["type"].as_str().unwrap_or("") {
            "pong" => {
                if matches!(self.ping, Some((p, _)) if p == id) {
                    self.ping = None;
                }
            }
            "event" if id == self.sub_id => self.entities(&v["event"]),
            "event" if self.forecasts.contains_key(&id) => self.forecast_event(self.forecasts[&id], &v["event"]),
            "event" if is_run => self.assist_event(ws, &v["event"])?,
            "event" if self.announce == Some(id) => {
                if let Some(url) = v["event"]["data"]["tts_output"]["url"].as_str().filter(|_| v["event"]["type"] == "tts-end") {
                    self.announcer.play(&format!("http://{}{url}", self.shared.cfg.ha), 0);
                    self.announce = None;
                }
            }
            "result" if v["success"] == false => {
                let msg = v["error"]["message"].as_str().unwrap_or("request failed").to_string();
                log(format!("request {id} failed: {msg}"));
                if is_run {
                    self.fail(&msg);
                }
                if let Some(targets) = self.calls.remove(&id) {
                    self.shared.update(|s| targets.iter().for_each(|e| drop(s.overrides.remove(e))));
                }
                self.browse.remove(&id);
                self.requests.remove(&id);
                if self.forecasts.remove(&id).is_some() {
                    self.forecast_retry = Some(Instant::now() + FORECAST_RETRY);
                }
                self.chained.remove(&id);
                if self.music_calls.remove(&id) {
                    self.pending = None;
                    self.shared.update(|s| s.music.expect = None);
                }
            }
            "result" => {
                self.calls.remove(&id);
                self.music_calls.remove(&id);
                if let Some(rest) = self.chained.remove(&id) {
                    self.chain(ws, rest)?;
                }
                if let Some(reply) = self.requests.remove(&id) {
                    let _ = reply.send(v["result"].clone());
                }
                if self.calendar_call == Some(id) {
                    self.calendar_call = None;
                    self.calendar_result(&v["result"]);
                }
                if let Some(kind) = self.browse.remove(&id) {
                    self.browse_result(ws, kind, &v["result"])?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn entities(&self, ev: &Value) {
        let mut turned_on = Vec::new();
        self.shared.update(|s| {
            if let Some(added) = ev["a"].as_object() {
                for (id, e) in added {
                    if let Some(state) = e["s"].as_str() {
                        s.entities.insert(id.clone(), state.to_string());
                    }
                    s.attrs.insert(id.clone(), e["a"].as_object().cloned().unwrap_or_default());
                    if let Some(lc) = e["lc"].as_f64() {
                        s.changed.insert(id.clone(), lc);
                    }
                    if let Some(lu) = e["lu"].as_f64().or(e["lc"].as_f64()) {
                        s.updated.insert(id.clone(), lu);
                    }
                }
            }
            if let Some(changed) = ev["c"].as_object() {
                for (id, e) in changed {
                    let plus = &e["+"];
                    if let Some(state) = plus["s"].as_str() {
                        if let Some(old) = s.entities.insert(id.clone(), state.to_string()) {
                            // An event entity's state is the time of its latest event.
                            let event = id.starts_with("event.") && !["unavailable", "unknown"].contains(&old.as_str()) && old != state;
                            if (state == "on" && old != "on") || event {
                                turned_on.push(id.clone());
                            }
                        }
                    }
                    if let Some(lc) = plus["lc"].as_f64() {
                        s.changed.insert(id.clone(), lc);
                    }
                    if let Some(lu) = plus["lu"].as_f64().or(plus["lc"].as_f64()) {
                        s.updated.insert(id.clone(), lu);
                    }
                    let attrs = s.attrs.entry(id.clone()).or_default();
                    if let Some(a) = plus["a"].as_object() {
                        attrs.extend(a.iter().map(|(k, v)| (k.clone(), v.clone())));
                    }
                    for k in e["-"]["a"].as_array().into_iter().flatten().filter_map(Value::as_str) {
                        attrs.remove(k);
                    }
                }
            }
            if let Some(removed) = ev["r"].as_array() {
                for id in removed.iter().filter_map(Value::as_str) {
                    s.entities.remove(id);
                    s.attrs.remove(id);
                    s.changed.remove(id);
                    s.updated.remove(id);
                }
            }
            let entities = &s.entities;
            s.overrides.retain(|id, (want, _)| entities.get(id).map(String::as_str) != Some(want.as_str()));
            music::settle(&self.shared.cfg, s);
        });
        for id in turned_on {
            doorbell::fire(&self.shared, &id);
        }
    }

    fn tick(&mut self, ws: &mut Ws) -> Res<()> {
        let now = Instant::now();
        match self.ping {
            Some((_, sent)) if now - sent > PING_TIMEOUT => return Err("no pong".into()),
            None if now - self.last_ping > PING_EVERY => {
                let id = self.send(ws, json!({"type": "ping"}))?;
                self.ping = Some((id, now));
                self.last_ping = now;
            }
            _ => {}
        }
        if let Some(run) = &self.run {
            let listened = now - run.started;
            if !run.audio_done && (listened > LISTEN_MAX || (!run.speech && listened > NO_SPEECH_MAX)) {
                self.end_audio(ws)?;
            }
        }
        self.music_tick(ws)?;
        if self.forecast_retry.is_some_and(|t| now >= t) {
            self.forecast_retry = None;
            self.subscribe_forecasts(ws)?;
        }
        self.calendar_tick(ws)?;
        self.timers();
        Ok(())
    }

    fn timers(&mut self) {
        let now = Instant::now();
        let (expired, phase) = self.shared.read(|s| {
            let expect = s.music.expect.as_ref().is_some_and(|(_, _, t)| *t <= now);
            (expect || s.overrides.values().any(|(_, t)| *t <= now), s.phase)
        });
        if expired {
            self.shared.update(|s| {
                s.overrides.retain(|_, (_, t)| *t > now);
                if s.music.expect.as_ref().is_some_and(|(_, _, t)| *t <= now) {
                    s.music.expect = None;
                }
            });
        }
        let shown = now - self.phase_since;
        if (phase == "done" && shown > DONE_SHOW) || (phase == "error" && shown > ERROR_SHOW) {
            self.set_phase("idle", |_| {});
        }
    }

    fn press(&mut self, ws: &mut Ws, gid: &str) -> Res<()> {
        let shared = self.shared.clone();
        let Some(g) = shared.cfg.groups.iter().find(|g| g.id == gid) else { return Ok(()) };
        let (on, targets) = shared.read(|s| group_status(s, g));
        if targets.is_empty() || (!on && g.off_only) {
            return Ok(());
        }
        let (service, want) = if on { ("turn_off", "off") } else { ("turn_on", "on") };
        let msg = json!({"type": "call_service", "domain": "homeassistant", "service": service, "target": {"entity_id": targets}});
        let id = self.send(ws, msg)?;
        log(format!("{}: {service}", g.name));
        let until = Instant::now() + OVERRIDE_TTL;
        shared.update(|s| {
            for e in &targets {
                if s.entities.get(e).map(String::as_str) != Some(want) {
                    s.overrides.insert(e.clone(), (want.to_string(), until));
                }
            }
        });
        self.calls.insert(id, targets);
        Ok(())
    }

    fn brightness(&mut self, ws: &mut Ws, gid: &str, value: u8) -> Res<()> {
        let shared = self.shared.clone();
        let Some(g) = shared.cfg.groups.iter().find(|g| g.id == gid) else { return Ok(()) };
        let Some(msg) = shared.read(|s| lights::command(s, g, value)) else { return Ok(()) };
        self.send(ws, msg)?;
        Ok(())
    }

    fn start(&mut self, ws: &mut Ws) -> Res<()> {
        if !self.shared.cfg.assist || self.shared.read(|s| s.intercom.talking || !s.intercom.incoming.is_empty()) {
            self.set_phase("idle", |_| {});
            return Ok(());
        }
        self.player.stop();
        let shared = self.shared.clone();
        let cfg = &shared.cfg;
        let mut msg = json!({"type": "assist_pipeline/run", "start_stage": "stt", "end_stage": "tts", "input": {"sample_rate": 16000}});
        if let Some(p) = &cfg.pipeline {
            msg["pipeline"] = json!(p);
        }
        if let Some(d) = &cfg.device_id {
            msg["device_id"] = json!(d);
        }
        if let Some((c, at)) = &self.conversation {
            if at.elapsed() < CONVERSATION_TTL {
                msg["conversation_id"] = json!(c);
            }
        }
        let id = self.send(ws, msg)?;
        self.runs += 1;
        self.run = Some(Run::new(id, self.runs));
        self.mic.start();
        self.set_phase("listening", |s| {
            s.heard.clear();
            s.reply.clear();
            s.error.clear();
        });
        Ok(())
    }

    fn audio(&mut self, ws: &mut Ws, chunk: Vec<u8>) -> Res<()> {
        let Some(run) = self.run.as_mut() else { return Ok(()) };
        if run.audio_done {
            return Ok(());
        }
        match run.handler {
            Some(h) => {
                let mut frame = Vec::with_capacity(chunk.len() + 1);
                frame.push(h);
                frame.extend_from_slice(&chunk);
                ws.send(Message::Binary(frame))?;
            }
            None => run.buffered.push(chunk),
        }
        Ok(())
    }

    fn end_audio(&mut self, ws: &mut Ws) -> Res<()> {
        self.mic.stop();
        let Some(run) = self.run.as_mut() else { return Ok(()) };
        if run.audio_done {
            return Ok(());
        }
        run.audio_done = true;
        if let Some(h) = run.handler {
            ws.send(Message::Binary(vec![h]))?;
        }
        if !run.playing {
            self.set_phase("thinking", |_| {});
        }
        Ok(())
    }

    fn assist_event(&mut self, ws: &mut Ws, ev: &Value) -> Res<()> {
        let data = &ev["data"];
        match ev["type"].as_str().unwrap_or("") {
            "run-start" => {
                if let Some(c) = data["conversation_id"].as_str() {
                    self.conversation = Some((c.to_string(), Instant::now()));
                }
                let Some(run) = self.run.as_mut() else { return Ok(()) };
                run.handler = data["runner_data"]["stt_binary_handler_id"].as_u64().map(|h| h as u8);
                run.tts_url = data["tts_output"]["url"].as_str().map(String::from);
                if let Some(h) = run.handler {
                    for chunk in run.buffered.drain(..) {
                        let mut frame = vec![h];
                        frame.extend(chunk);
                        ws.send(Message::Binary(frame))?;
                    }
                    if run.audio_done {
                        ws.send(Message::Binary(vec![h]))?;
                    }
                }
            }
            "stt-vad-start" => {
                if let Some(run) = self.run.as_mut() {
                    run.speech = true;
                }
            }
            "stt-vad-end" => self.end_audio(ws)?,
            "stt-end" => {
                self.end_audio(ws)?;
                let text = data["stt_output"]["text"].as_str().unwrap_or("").trim().to_string();
                self.shared.update(|s| s.heard = text);
            }
            "intent-progress" => {
                if let Some(c) = data["chat_log_delta"]["content"].as_str() {
                    let c = c.to_string();
                    self.shared.update(|s| s.reply.push_str(&c));
                }
                if data["tts_start_streaming"] == true {
                    self.play();
                }
            }
            "intent-end" => {
                let out = &data["intent_output"];
                if let Some(c) = out["conversation_id"].as_str() {
                    self.conversation = Some((c.to_string(), Instant::now()));
                }
                if let Some(run) = self.run.as_mut() {
                    run.cont = out["continue_conversation"] == true;
                }
                let reply = out["response"]["speech"]["plain"]["speech"].as_str().unwrap_or("").to_string();
                if !reply.is_empty() {
                    self.shared.update(|s| s.reply = reply);
                }
            }
            "tts-start" => self.play(),
            "tts-end" => {
                if let (Some(run), Some(url)) = (self.run.as_mut(), data["tts_output"]["url"].as_str()) {
                    run.tts_url = Some(url.to_string());
                }
                self.play();
            }
            "run-end" => {
                let playing = self.run.as_mut().is_some_and(|r| {
                    r.ended = true;
                    r.playing
                });
                if !playing {
                    self.finish(ws)?;
                }
            }
            "error" => {
                let code = data["code"].as_str().unwrap_or("");
                log(format!("assist error {code}: {}", data["message"]));
                let msg = match code {
                    "stt-no-text-recognized" => "Didn't catch that",
                    _ => data["message"].as_str().unwrap_or("Something went wrong"),
                };
                self.fail(msg);
            }
            _ => {}
        }
        Ok(())
    }

    fn play(&mut self) {
        let ha = self.shared.cfg.ha.clone();
        let Some(run) = self.run.as_mut() else { return };
        if run.played {
            return;
        }
        let Some(url) = run.tts_url.clone() else { return };
        run.playing = true;
        run.played = true;
        let token = run.token;
        self.player.play(&format!("http://{ha}{url}"), token);
        self.set_phase("speaking", |_| {});
    }

    fn player_done(&mut self, ws: &mut Ws, token: u64) -> Res<()> {
        let ended = match self.run.as_mut() {
            Some(r) if r.token == token => {
                r.playing = false;
                r.ended
            }
            _ => return Ok(()),
        };
        if ended {
            self.finish(ws)?;
        }
        Ok(())
    }

    fn finish(&mut self, ws: &mut Ws) -> Res<()> {
        if self.run.take().is_some_and(|r| r.cont) {
            return self.start(ws);
        }
        self.set_phase("done", |_| {});
        Ok(())
    }

    fn fail(&mut self, msg: &str) {
        self.mic.stop();
        self.player.stop();
        self.run = None;
        let msg = msg.to_string();
        self.set_phase("error", |s| s.error = msg);
    }

    fn cancel(&mut self, ws: &mut Ws) -> Res<()> {
        self.mic.stop();
        self.player.stop();
        if let Some(run) = self.run.take() {
            if let (false, Some(h)) = (run.audio_done, run.handler) {
                ws.send(Message::Binary(vec![h]))?;
            }
        }
        self.set_phase("idle", |_| {});
        Ok(())
    }
}
