# Configuration

Everything lives in one file on the device, `/userdata/open-cgs2/config.json`, read by the screen's
backend (`panel`) and the sensor daemon (`sensord`). The installer writes it; `cgs2 config HOST`
asks again, and `cgs2 config HOST config.json` pushes your own. Every key is optional: an empty
`{}` gives the readings, charts, clock, alarms and Settings, with nothing else.

```json
{
  "device": {"name": "Living Room", "area": "Living Room"},
  "units": {"temperature": "C", "clock": "24h"},
  "location": {"latitude": 52.37, "longitude": 4.9},
  "home_assistant": {"url": "http://homeassistant.local:8123", "token": "<long-lived access token>"},
  "mqtt": {"host": "homeassistant.local", "username": "cgs2", "password": "<password>"},
  "calibration": {"humidity_scale": 0.9, "temperature_offset": 0, "humidity_offset": 0},
  "particulate": {"night": "22:00-08:30"},
  "pages": {"left": ["path"], "right": ["weather"], "up": ["thermostat"], "down": []},
  "components": {
    "path": {"enabled": true, "station": "EXP"},
    "calls": true,
    "spotify": {"speakers": [{"name": "Kitchen"}, {"name": "Dining", "entity": "media_player.dining"}]}
  }
}
```

## Device and units

| Key | Default | |
|---|---|---|
| `device.name` | `CGS2` | Shown in Settings, used for the Home Assistant device and its entity IDs (`sensor.<name>_co2`), the intercom and this device's line on charts. |
| `device.area` | | Suggested area for the Home Assistant device. |
| `units.temperature` | `C` | `C` or `F`, for the weather and this device's readings. Home Assistant's values come in Home Assistant's own units. |
| `units.clock` | `24h` | `24h` or `12h`. |
| `location` | Home Assistant's | `latitude` and `longitude` for the weather and the sunrise shading on the forecast. |

## Home Assistant

| Key | |
|---|---|
| `home_assistant.url` | `http://host:8123`. Plain HTTP only; the panel talks to Home Assistant on your network. |
| `home_assistant.token` | A long-lived access token. A non-admin user is enough. |
| `mqtt.host`, `port` (1883), `username`, `password` | The broker Home Assistant's MQTT integration uses. `sensord` publishes the readings there with MQTT discovery: temperature, humidity, CO₂, eTVOC, PM2.5, PM10, illuminance, battery and charging, every `mqtt.interval` seconds (15). `mqtt.discovery_prefix` is `homeassistant` by default. |

Without `mqtt`, Home Assistant gets no readings, but the screen still works with Home Assistant.

## Calibration

| Key | Default | |
|---|---|---|
| `calibration.humidity_scale` | `0.9` | Applied to the raw humidity before it is corrected for the case's heat. Next to other monitors, two units read about 10% too much water vapour without it. |
| `calibration.temperature_offset` | `0` | °C added to the temperature. |
| `calibration.humidity_offset` | `0` | Points added to the humidity. |

Settings → Calibration changes these on the device itself (saved in
`/userdata/open-cgs2/calibration.json`, which wins over config.json). The CO₂ sensor calibrates
itself against the lowest level it sees each week; Settings can also calibrate it to outdoor air.

| Key | Default | |
|---|---|---|
| `particulate.night` | `22:00-08:30` | When the particulate sensor (and its fan) is off. Settings changes it too. |

## Components

Each folder in `/opt/open-cgs2/ui/components` is a component. `components.<name>` is either `true`
or `false`, or an object with `enabled` (`true`, `false` or `"auto"`) and the component's own
settings. Left out, a component takes the default from its manifest:

| Component | Default | Turns on with `auto` when | Settings |
|---|---|---|---|
| `weather` | auto | there is a `location`, or Home Assistant has a weather entity | `entity`: a Home Assistant weather entity instead of Open-Meteo; `source: "home_assistant"` to prefer the one Home Assistant has. |
| `lights` | auto | Home Assistant has lights in areas: a tile per area (up to five) and All off | `groups`: `[{"name", "entities": [...], "off_only"}]` instead. |
| `thermostat` | auto | Home Assistant has a `climate` entity | `entity`; `stale_after`: minutes without an update before the values dim, for thermostats that report rarely. |
| `assist` | auto | Home Assistant is set up | `pipeline`: an Assist pipeline ID instead of the preferred one; `name`: the button's label. |
| `spotify` | auto | Home Assistant has the Spotify integration | `entity`; `speakers`: `[{"name", "spotify": [Connect names], "entity": a player that plays Spotify itself}]`, else the account's Connect devices; `default`: speaker ID; `mixes`: `[{"name", "uri"}]` for Spotify-made playlists, which the integration cannot list. |
| `calendar` | auto | Home Assistant has calendars | `calendars`: entity IDs, else all of them. |
| `calls` | off | | Every timed calendar event lights the screen 10 minutes before it starts and is announced 5 minutes before, through the Assist pipeline's voice. |
| `doorbell` | auto | Home Assistant has a doorbell: an `event` entity of the doorbell class, or a visitor sensor, with a camera on the same device | `name`, `camera`, `ring`, `motion: [{"entity", "label"}]` (most specific first), `relay`: `host:port` of something that shares snapshots between screens. |
| `intercom` | auto | the installer gave it a household key, or `peers` are listed | `name` (defaults to `device.name`), `port` (8098), `key`, `peers: [{"name", "host"}]`. |
| `path` | off | | `station`: a RidePATH code (`GRV`); `source`: the feed URL, for a proxy; `hide_under_minutes`: leave out trains sooner than the walk to the station. |

What the panel finds in Home Assistant is kept in `/userdata/open-cgs2/detected.json` and checked
on every connection; when it changes (Spotify added, a new room with lights), the panel restarts
with the new components. Anything set in config.json wins over what was found.

## Pages

Pages sit around the home page: `left`, `right`, `up` and `down`, each a list going outwards.
Swipe left to reach a page on the right, and down to reach one above. Without `pages`, a
component's page goes where its manifest says (weather right, thermostat up, PATH left).

## Charts

Each reading's chart shows this device and, with Home Assistant, the other open-cgs2 units. To
show other sensors, list the lines per reading (`co2`, `temp`, `rh`, `tvoc`, `pm25`, `pm10`):

```json
"charts": {
  "co2": [
    {"name": "Here", "here": true},
    {"name": "Bedroom", "entity": "sensor.bedroom_co2"}
  ],
  "temp": [
    {"name": "Here", "here": true},
    {"name": "Outside", "entity": "sensor.outdoor_temperature", "outside": true}
  ]
}
```

## Other keys

| Key | Default | |
|---|---|---|
| `sensors` | | Home Assistant entities in place of readings on the screen: `{"outside": "sensor.x", "outside_rh": ..., "sky": ..., "rain_soon": ...}`, or this device's own keys (`co2`, `temp`, `rh`, `tvoc`). |
| `mic` | the CGS2's | `device`, `channels`, `channel` and `gain` of the microphone for Assist and the intercom. |
| `port` | `8099` | The panel's port on 127.0.0.1. |
