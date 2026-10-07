//! Spotify on the account's speakers. Spotify Connect speakers are driven through HA's Spotify
//! player; a speaker with an `entity` (a Sonos) plays Spotify links itself through its own
//! integration, so moving between the two kinds restarts the playlist (or the current track) on
//! the new speaker.
use super::{Res, Session, Ws};
use crate::{log, Config, Music, Speaker, State};
use serde_json::{json, Value};
use std::time::{Duration, Instant};

const PLAYLISTS_TTL: Duration = Duration::from_secs(600);
const RECENT_TTL: Duration = Duration::from_secs(60);
const RECENT_MAX: usize = 24;
/// HA polls Spotify every 30 s, so a new volume or shuffle shows until it reports it.
const REPORT_TTL: Duration = Duration::from_secs(35);
/// A speaker in standby can take about 12 s to start after a transfer.
const PENDING_TTL: Duration = Duration::from_secs(20);
const PENDING_POLL: Duration = Duration::from_secs(3);
const EXPECT_TTL: Duration = Duration::from_secs(5);

pub struct Item {
    name: String,
    uri: String,
}

/// A recently played track and where it sits in its album.
pub struct Recent {
    name: String,
    artist: String,
    uri: String,
    album: String,
    position: u64,
}

#[derive(Default)]
pub struct MusicState {
    selected: String,
    /// Playlist started from the screen and the speaker it went to.
    current: Option<(String, String)>,
    playlists: Vec<Item>,
    recent: Vec<Recent>,
    library_v: u64,
    fetched: Option<Instant>,
    recent_fetched: Option<Instant>,
    /// Spotify config entry as a browse root, e.g. `spotify://<entry id>`.
    entry: Option<String>,
    /// Speaker state shown right after a press, until HA reports it or it expires.
    pub expect: Option<(String, &'static str, Instant)>,
    volume: Option<(f64, Instant)>,
    shuffle: Option<(bool, Instant)>,
}

pub enum Browse {
    Root,
    Playlists,
    Recent,
}

/// A Spotify transfer in flight. HA only polls Spotify every 30 s, so it is refreshed until the
/// new speaker shows up, then `play` (if any) is started there.
pub struct Pending {
    source: String,
    play: Option<Value>,
    until: Instant,
    poll: Instant,
}

fn attr<'a>(s: &'a State, entity: &str, key: &str) -> Option<&'a str> {
    s.attrs.get(entity)?.get(key)?.as_str()
}

fn control<'a>(m: &'a Music, sp: &'a Speaker) -> &'a str {
    sp.entity.as_deref().unwrap_or(&m.spotify)
}

fn listed(s: &State, m: &Music, name: &str) -> bool {
    attr(s, &m.spotify, "source") == Some(name)
        || s.attrs
            .get(&m.spotify)
            .and_then(|a| a.get("source_list")?.as_array())
            .is_some_and(|l| l.iter().any(|x| x.as_str() == Some(name)))
}

/// The speaker's Spotify Connect name as Spotify currently lists it.
fn spotify_name<'a>(s: &State, m: &Music, sp: &'a Speaker) -> Option<&'a str> {
    sp.spotify.iter().find(|n| listed(s, m, n)).or(sp.spotify.first()).map(String::as_str)
}

fn raw_state(s: &State, m: &Music, sp: &Speaker) -> &'static str {
    let norm = |e: &str| match s.entities.get(e).map(String::as_str) {
        Some("playing") => "playing",
        Some("paused") => "paused",
        Some("unavailable") | None => "offline",
        _ => "idle",
    };
    if let Some(e) = &sp.entity {
        return norm(e);
    }
    let Some(name) = spotify_name(s, m, sp) else { return "offline" };
    if attr(s, &m.spotify, "source") == Some(name) && matches!(norm(&m.spotify), "playing" | "paused") {
        return norm(&m.spotify);
    }
    if listed(s, m, name) { "idle" } else { "offline" }
}

fn volume(s: &State, entity: &str) -> Option<f64> {
    s.attrs.get(entity)?.get("volume_level")?.as_f64()
}

fn shuffle(s: &State, entity: &str) -> Option<bool> {
    s.attrs.get(entity)?.get("shuffle")?.as_bool()
}

fn shown_shuffle(s: &State, m: &Music) -> Option<bool> {
    let sp = active(s, m)?;
    s.music.shuffle.map(|(v, _)| v).or_else(|| shuffle(s, control(m, sp)))
}

/// The Spotify track a speaker is playing, as `spotify:track:<id>`.
fn track(s: &State, m: &Music, sp: &Speaker) -> Option<String> {
    match &sp.entity {
        Some(e) => attr(s, e, "media_content_id").and_then(sonos_track),
        None => attr(s, &m.spotify, "media_content_id").filter(|u| u.starts_with("spotify:track:")).map(String::from),
    }
}

fn speaker_state(s: &State, m: &Music, sp: &Speaker) -> &'static str {
    match &s.music.expect {
        Some((id, st, _)) if *id == sp.id => st,
        _ => raw_state(s, m, sp),
    }
}

/// The speaker the transport buttons control: a playing one, else a paused one; the
/// selected speaker wins ties, then the most recently changed.
fn active<'a>(s: &State, m: &'a Music) -> Option<&'a Speaker> {
    let changed = |sp: &Speaker| s.changed.get(control(m, sp)).copied().unwrap_or(0.0);
    ["playing", "paused"].iter().find_map(|want| {
        let found: Vec<&Speaker> = m.speakers.iter().filter(|sp| speaker_state(s, m, sp) == *want).collect();
        found
            .iter()
            .find(|sp| sp.id == s.music.selected)
            .or_else(|| found.iter().max_by(|a, b| changed(a).total_cmp(&changed(b))))
            .copied()
    })
}

pub fn settle(cfg: &Config, s: &mut State) {
    let Some(m) = &cfg.music else { return };
    if let Some((id, want, _)) = &s.music.expect {
        if m.speakers.iter().any(|sp| sp.id == *id && raw_state(s, m, sp) == *want) {
            s.music.expect = None;
        }
    }
    if let Some((want, until)) = s.music.volume {
        let reported = active(s, m).and_then(|sp| volume(s, control(m, sp)));
        if Instant::now() > until || reported.is_some_and(|v| (v - want).abs() < 0.01) {
            s.music.volume = None;
        }
    }
    if let Some((want, until)) = s.music.shuffle {
        let reported = active(s, m).and_then(|sp| shuffle(s, control(m, sp)));
        if Instant::now() > until || reported == Some(want) {
            s.music.shuffle = None;
        }
    }
}

pub fn snapshot(cfg: &Config, s: &State) -> Value {
    let Some(m) = &cfg.music else { return Value::Null };
    let a = active(s, m);
    let speakers: Vec<Value> = m
        .speakers
        .iter()
        .map(|sp| json!({"id": sp.id, "name": sp.name, "state": speaker_state(s, m, sp)}))
        .collect();
    let e = a.map(|sp| control(m, sp)).unwrap_or_default();
    let art = attr(s, e, "entity_picture").map(|p| format!("http://{}{p}", cfg.ha));
    let vol = a.and_then(|_| s.music.volume.map(|(v, _)| v).or_else(|| volume(s, e)));
    json!({
        "active": a.map(|sp| &sp.id),
        "state": a.map_or("idle", |sp| speaker_state(s, m, sp)),
        "selected": target(s, m).map(|sp| &sp.id),
        "title": attr(s, e, "media_title"),
        "artist": attr(s, e, "media_artist"),
        "art": art,
        "volume": vol.map(|v| (v * 100.0).round()),
        "shuffle": shown_shuffle(s, m),
        "track": a.and_then(|sp| track(s, m, sp)),
        "speakers": speakers,
        "library_v": s.music.library_v,
        "current": s.music.current.as_ref().map(|(uri, _)| uri),
    })
}

pub fn library(s: &State, m: Option<&Music>) -> Value {
    let playlists: Vec<Value> = s.music.playlists.iter().map(|p| json!({"name": p.name, "uri": p.uri})).collect();
    let recent: Vec<Value> = s.music.recent.iter().map(|t| json!({"name": t.name, "artist": t.artist, "uri": t.uri})).collect();
    let mixes = m.map(|m| m.mixes.clone()).unwrap_or_default();
    json!({"playlists": playlists, "recent": recent, "mixes": mixes, "recent_supported": m.is_some_and(|m| m.recent)})
}

/// Album and position to start a recently played track from, so its album plays on.
fn in_album(s: &State, uri: &str) -> Option<(String, u64)> {
    s.music.recent.iter().find(|t| t.uri == uri).map(|t| (t.album.clone(), t.position))
}

fn service(domain: &str, service: &str, entity: &str, data: Value) -> Value {
    let mut msg = json!({"type": "call_service", "domain": domain, "service": service, "target": {"entity_id": entity}});
    if !data.is_null() {
        msg["service_data"] = data;
    }
    msg
}

/// Where a new playlist goes: the active speaker, else the last one picked, else the default.
fn target<'a>(s: &State, m: &'a Music) -> Option<&'a Speaker> {
    active(s, m)
        .or_else(|| m.speakers.iter().find(|sp| sp.id == s.music.selected))
        .or_else(|| m.speakers.iter().find(|sp| sp.id == m.default))
}

fn items(children: &[Value]) -> Vec<Item> {
    children
        .iter()
        .filter_map(|c| {
            let uri = c["media_content_id"].as_str()?.rsplit('/').next()?;
            Some(Item { name: clean(c["title"].as_str()?), uri: uri.to_string() })
        })
        .collect()
}

/// Drops emoji and flags, which the device fonts can't draw.
fn clean(name: &str) -> String {
    let s: String = name.chars().filter(|c| (*c as u32) < 0x2600 || ('\u{3000}'..'\u{FE00}').contains(c)).collect();
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `x-sonos-spotify:spotify%3atrack%3a<id>?sid=…` → `spotify:track:<id>`.
fn sonos_track(id: &str) -> Option<String> {
    let rest = id.strip_prefix("x-sonos-spotify:")?;
    let uri = rest.split('?').next()?.replace("%3a", ":").replace("%3A", ":");
    uri.starts_with("spotify:track:").then_some(uri)
}

impl Session {
    fn call(&mut self, ws: &mut Ws, name: &str, entity: &str, data: Value) -> Res<()> {
        self.chain(ws, vec![service("media_player", name, entity, data)])
    }

    /// Sends the calls in order, each once the one before it has succeeded.
    pub(super) fn chain(&mut self, ws: &mut Ws, mut calls: Vec<Value>) -> Res<()> {
        if calls.is_empty() {
            return Ok(());
        }
        let msg = calls.remove(0);
        log(format!("{}: {}", msg["target"]["entity_id"].as_str().unwrap_or_default(), msg["service"].as_str().unwrap_or_default()));
        let id = self.send(ws, msg)?;
        self.music_calls.insert(id);
        if !calls.is_empty() {
            self.chained.insert(id, calls);
        }
        Ok(())
    }

    fn expect(&self, sp: &Speaker, state: &'static str, ttl: Duration) {
        let id = sp.id.clone();
        self.shared.update(|s| s.music.expect = Some((id, state, Instant::now() + ttl)));
    }

    fn transfer(&mut self, ws: &mut Ws, m: &Music, name: &str, play: Option<Value>) -> Res<()> {
        self.call(ws, "select_source", &m.spotify, json!({"source": name}))?;
        let now = Instant::now();
        self.pending = Some(Pending { source: name.to_string(), play, until: now + PENDING_TTL, poll: now + PENDING_POLL });
        Ok(())
    }

    pub(super) fn browse_root(&mut self, ws: &mut Ws) -> Res<()> {
        let shared = self.shared.clone();
        let Some(e) = shared.cfg.music.as_ref().and_then(|m| m.speakers.iter().find_map(|sp| sp.entity.as_ref())) else {
            return Ok(());
        };
        let id = self.send(ws, json!({"type": "media_player/browse_media", "entity_id": e}))?;
        self.browse.insert(id, Browse::Root);
        Ok(())
    }

    fn browse_playlists(&mut self, ws: &mut Ws, entry: &str) -> Res<()> {
        let shared = self.shared.clone();
        let Some(e) = shared.cfg.music.as_ref().and_then(|m| m.speakers.iter().find_map(|sp| sp.entity.as_ref())) else {
            return Ok(());
        };
        let msg = json!({
            "type": "media_player/browse_media",
            "entity_id": e,
            "media_content_id": format!("{entry}/current_user_playlists"),
            "media_content_type": "spotify://current_user_playlists",
        });
        let id = self.send(ws, msg)?;
        self.browse.insert(id, Browse::Playlists);
        Ok(())
    }

    /// `spotify.recently_played` is not in the stock Spotify integration; without it there is no
    /// Recent tab.
    fn fetch_recent(&mut self, ws: &mut Ws) -> Res<()> {
        let shared = self.shared.clone();
        let Some(m) = shared.cfg.music.as_ref().filter(|m| m.recent) else { return Ok(()) };
        let mut msg = service("spotify", "recently_played", &m.spotify, Value::Null);
        msg["return_response"] = json!(true);
        let id = self.send(ws, msg)?;
        self.browse.insert(id, Browse::Recent);
        Ok(())
    }

    pub(super) fn browse_result(&mut self, ws: &mut Ws, kind: Browse, result: &Value) -> Res<()> {
        let children = result["children"].as_array().map(Vec::as_slice).unwrap_or_default();
        match kind {
            Browse::Root => {
                let entry = children.iter().filter_map(|c| c["media_content_id"].as_str()).find(|id| id.starts_with("spotify://"));
                let Some(entry) = entry.map(|e| e.to_lowercase()) else {
                    log("no Spotify library to browse");
                    return Ok(());
                };
                self.shared.update(|s| s.music.entry = Some(entry.clone()));
                self.browse_playlists(ws, &entry)
            }
            Browse::Playlists => {
                let list = items(children);
                log(format!("{} playlists", list.len()));
                self.shared.update(|s| {
                    s.music.playlists = list;
                    s.music.library_v += 1;
                    s.music.fetched = Some(Instant::now());
                });
                Ok(())
            }
            Browse::Recent => {
                let spotify = self.shared.cfg.music.as_ref().map(|m| m.spotify.as_str()).unwrap_or_default();
                let tracks = result["response"][spotify]["tracks"].as_array().map(Vec::as_slice).unwrap_or_default();
                let list: Vec<Recent> = tracks
                    .iter()
                    .filter_map(|t| {
                        Some(Recent {
                            name: clean(t["name"].as_str()?),
                            artist: clean(t["artist"].as_str().unwrap_or_default()),
                            uri: t["uri"].as_str()?.to_string(),
                            album: t["album_uri"].as_str()?.to_string(),
                            position: t["position"].as_u64()?,
                        })
                    })
                    .take(RECENT_MAX)
                    .collect();
                self.shared.update(|s| {
                    s.music.recent = list;
                    s.music.library_v += 1;
                    s.music.recent_fetched = Some(Instant::now());
                });
                Ok(())
            }
        }
    }

    pub(super) fn music(&mut self, ws: &mut Ws, action: &str, arg: &str) -> Res<()> {
        let shared = self.shared.clone();
        let Some(m) = &shared.cfg.music else { return Ok(()) };
        let (a, state) = shared.read(|s| {
            let a = active(s, m);
            (a, a.map_or("idle", |sp| speaker_state(s, m, sp)))
        });
        match (action, a) {
            ("open", _) => {
                let msg = json!({"type": "call_service", "domain": "homeassistant", "service": "update_entity", "target": {"entity_id": m.spotify}});
                self.send(ws, msg)?;
                let old = |t: Option<Instant>, ttl: Duration| t.is_none_or(|t| t.elapsed() > ttl);
                let (entry, playlists, recent) =
                    shared.read(|s| (s.music.entry.clone(), old(s.music.fetched, PLAYLISTS_TTL), old(s.music.recent_fetched, RECENT_TTL)));
                if recent {
                    self.fetch_recent(ws)?;
                }
                match entry {
                    None => self.browse_root(ws),
                    Some(entry) if playlists => self.browse_playlists(ws, &entry),
                    Some(_) => Ok(()),
                }
            }
            ("toggle", Some(sp)) => {
                let (service, next) = if state == "playing" { ("media_pause", "paused") } else { ("media_play", "playing") };
                self.call(ws, service, control(m, sp), Value::Null)?;
                self.expect(sp, next, EXPECT_TTL);
                Ok(())
            }
            ("next", Some(sp)) => self.call(ws, "media_next_track", control(m, sp), Value::Null),
            ("prev", Some(sp)) => self.call(ws, "media_previous_track", control(m, sp), Value::Null),
            ("volume", Some(sp)) => {
                let Ok(v) = arg.parse::<u8>() else { return Ok(()) };
                let v = f64::from(v.min(100)) / 100.0;
                // Spotify and Sonos truncate volume_level × 100, so 0.29 would land on 28%.
                let level = (v + 0.004).min(1.0);
                self.call(ws, "volume_set", control(m, sp), json!({"volume_level": level}))?;
                self.shared.update(|s| s.music.volume = Some((v, Instant::now() + REPORT_TTL)));
                Ok(())
            }
            ("shuffle", Some(sp)) => {
                let on = !shared.read(|s| shown_shuffle(s, m)).unwrap_or(false);
                self.call(ws, "shuffle_set", control(m, sp), json!({"shuffle": on}))?;
                self.shared.update(|s| s.music.shuffle = Some((on, Instant::now() + REPORT_TTL)));
                Ok(())
            }
            ("track", _) => {
                let Some(sp) = shared.read(|s| target(s, m)) else { return Ok(()) };
                self.play_track(ws, m, sp, arg)
            }
            ("speaker", _) => self.switch(ws, m, arg, a, state),
            ("playlist", _) => {
                let Some(sp) = shared.read(|s| target(s, m)) else { return Ok(()) };
                self.play_on(ws, m, sp, arg, "playlist", None)
            }
            _ => Ok(()),
        }
    }

    fn switch(&mut self, ws: &mut Ws, m: &Music, id: &str, a: Option<&Speaker>, state: &str) -> Res<()> {
        let Some(t) = m.speakers.iter().find(|sp| sp.id == id) else { return Ok(()) };
        self.shared.update(|s| s.music.selected = t.id.clone());
        let Some(a) = a.filter(|a| a.id != t.id) else { return Ok(()) };
        let (source, current, item, name) = self.shared.read(|s| {
            let item = track(s, m, a);
            let current = s.music.current.as_ref().filter(|(_, at)| *at == a.id).map(|(uri, _)| uri.clone());
            (attr(s, &m.spotify, "source").map(String::from), current, item, spotify_name(s, m, t))
        });
        if let (None, Some(name)) = (&t.entity, name) {
            if source.is_some_and(|src| a.spotify.contains(&src)) {
                log(format!("moving Spotify from {} to {}", a.name, t.name));
                self.transfer(ws, m, name, None)?;
                self.expect(t, if state == "playing" { "playing" } else { "paused" }, PENDING_TTL);
                self.shared.update(|s| {
                    if let Some(c) = s.music.current.as_mut().filter(|(_, at)| *at == a.id) {
                        c.1 = t.id.clone();
                    }
                });
                return Ok(());
            }
        }
        if current.is_none() && item.is_none() {
            return Ok(());
        }
        if state == "playing" {
            self.call(ws, "media_pause", control(m, a), Value::Null)?;
        }
        match (current, item) {
            (Some(uri), _) => {
                log(format!("restarting on {}: {uri}", t.name));
                self.play_on(ws, m, t, &uri, "playlist", None)
            }
            (None, Some(uri)) => {
                log(format!("restarting on {}: {uri}", t.name));
                self.play_track(ws, m, t, &uri)
            }
            _ => Ok(()),
        }
    }

    /// Plays a track in its album when it is a recent one, so the album continues after it.
    fn play_track(&mut self, ws: &mut Ws, m: &Music, sp: &Speaker, uri: &str) -> Res<()> {
        match self.shared.read(|s| in_album(s, uri)) {
            Some((album, position)) => self.play_on(ws, m, sp, &album, "album", Some((uri, position))),
            None => self.play_on(ws, m, sp, uri, "track", None),
        }
    }

    /// `start` is the track to begin `uri` (an album) at, with its position for the Sonos queue.
    fn play_on(&mut self, ws: &mut Ws, m: &Music, sp: &Speaker, uri: &str, kind: &str, start: Option<(&str, u64)>) -> Res<()> {
        if !uri.starts_with("spotify:") {
            return Ok(());
        }
        let (entry, source, state, name) = self.shared.read(|s| {
            let source = attr(s, &m.spotify, "source").map(String::from);
            (s.music.entry.clone(), source, s.entities.get(&m.spotify).cloned(), spotify_name(s, m, sp))
        });
        match (&sp.entity, name) {
            (Some(e), _) => {
                let Some(entry) = entry else { return Ok(()) };
                let mut data = json!({"media_content_id": format!("{entry}/{uri}"), "media_content_type": format!("spotify://{kind}"), "enqueue": "replace"});
                match start {
                    Some((_, position)) => {
                        data["enqueue"] = json!("add");
                        self.chain(
                            ws,
                            vec![
                                service("media_player", "clear_playlist", e, Value::Null),
                                service("media_player", "play_media", e, data),
                                service("sonos", "play_queue", e, json!({"queue_position": position})),
                            ],
                        )?;
                    }
                    None => self.call(ws, "play_media", e, data)?,
                }
            }
            (None, Some(name)) => {
                let mut data = json!({"media_content_id": uri, "media_content_type": kind});
                if let Some((track, _)) = start {
                    data["extra"] = json!({"offset": track});
                }
                let ready = source.as_deref() == Some(name) && matches!(state.as_deref(), Some("playing" | "paused"));
                if ready {
                    self.call(ws, "play_media", &m.spotify, data)?;
                } else {
                    self.transfer(ws, m, name, Some(data))?;
                }
            }
            _ => return Ok(()),
        }
        let (id, uri) = (sp.id.clone(), uri.to_string());
        self.shared.update(|s| {
            s.music.selected = id.clone();
            s.music.current = (kind == "playlist").then_some((uri, id));
        });
        self.expect(sp, "playing", PENDING_TTL);
        Ok(())
    }

    pub(super) fn music_tick(&mut self, ws: &mut Ws) -> Res<()> {
        let Some(p) = &self.pending else { return Ok(()) };
        let shared = self.shared.clone();
        let Some(m) = &shared.cfg.music else { return Ok(()) };
        let now = Instant::now();
        if now > p.until {
            log(format!("Spotify did not move to {}", p.source));
            self.pending = None;
            return Ok(());
        }
        let ready = shared.read(|s| {
            attr(s, &m.spotify, "source") == Some(p.source.as_str())
                && matches!(s.entities.get(&m.spotify).map(String::as_str), Some("playing" | "paused"))
        });
        if ready {
            if let Some(play) = self.pending.take().and_then(|p| p.play) {
                self.call(ws, "play_media", &m.spotify, play)?;
            }
        } else if now >= p.poll {
            let msg = json!({"type": "call_service", "domain": "homeassistant", "service": "update_entity", "target": {"entity_id": m.spotify}});
            self.send(ws, msg)?;
            if let Some(p) = self.pending.as_mut() {
                p.poll = now + PENDING_POLL;
            }
        }
        Ok(())
    }
}
