# Contributing

Issues and pull requests are welcome. Keep pull requests small and about one thing.

## Layout

| | |
|---|---|
| `sensord/` | The sensor daemon (Rust). |
| `panel/` | The screen's backend (Rust). |
| `device/open-cgs2/ui/` | The screen: `qml/` for the home page and shared items, `components/` for the rest. |
| `device/open-cgs2/*.sh`, `device/oem-start.sh` | Boot and supervision on the device. |
| `adbd-auth/` | The key check preloaded into the firmware's adbd. |
| `installer/` | `cgs2`, the installer. |
| `emulator/` | The screen on a computer against made-up state. |
| `docs/` | Installing, configuration, components and the hardware. |

## Checks

```bash
cargo test --workspace
```

```bash
python -m unittest discover -s tests
```

`sensord` and `panel` build and test on Linux and macOS (WSL on Windows); the installer and the
screen tests run anywhere. The screen tests need PyQt5. `scripts/bundle.sh` cross-builds the device
programs (see [Installing](docs/install.md#building-from-source)).

Try screen changes in the [emulator](docs/components.md#the-emulator) before a device, and on a
device before a pull request when they touch hardware, boot or the installer; say in the pull
request what you checked and where.

## Style

- Match the code around you; prefer fewer lines, and comments only where the code cannot say it.
- Commit subjects are short, capitalized and imperative, without a trailing period; the body says
  why.
- Every push to `main` makes a release named after the date (`v2026.10.09.02`).
