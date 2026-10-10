# open-cgs2

Open replacement software for the Qingping Air Monitor 2 (CGS2). It replaces the stock app with
its own sensor daemon and a screen with lights, Spotify, weather, a thermostat, PATH trains and
Home Assistant Assist.

> Unofficial. Not affiliated with or endorsed by Qingping. Installing it may void your warranty; use
> it at your own risk.

## What runs on the device

- `sensord` reads the sensors directly over I2C, corrects temperature and humidity for the case's
  own heat as the stock app does, keeps the VOC baseline across restarts, owns the particulate
  sensor and the hardware watchdog, and publishes the readings to Home Assistant with MQTT
  discovery.
- `panel` is the screen's backend: Home Assistant, Assist, the backlight and the sensor history.
- The screen is QML on the device's own Qt, with `qmlscene -platform eglfs`.

The device boots the stock firmware again after three failed boots, or when
`/userdata/open-cgs2/stock` exists.

## License

[MIT](LICENSE)
