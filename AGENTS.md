# AGENTS.md

Rust Spotify RFID controller for Raspberry Pi. Single Cargo crate (binary + library target), no workspace.

## Development environment

- Nix flake via direnv (`.envrc` → `use flake`). `nix develop` provides alsa-lib, openssl, udev, pkg-config, cargo-watch, cargo-edit.
- Without Nix: `sudo apt install build-essential pkg-config libssl-dev libudev-dev libasound2-dev`.
- `RUST_BACKTRACE=1` is set in the dev shell; `PKG_CONFIG_PATH` points at the Nix alsa/openssl.

## Commands

- Build: `cargo build --release`
- Run (hardware mode, default): `cargo run` — requires Raspberry Pi GPIO at runtime.
- Run CLI subcommands: `cargo run -- <command>` where command is `auth {login|logout|status}`, `config {show|add-card|remove-card}`, `status`, `spotify status`, `cards {list|add|remove}`.
- Tests: `cargo test` — unit tests in `src/lib.rs` and `src/config/mod.rs` run anywhere; `test_hardware_manager_creation` deliberately ignores failure when GPIO is absent.
- Watch: `cargo watch -x run`.
- No linter/formatter config beyond rustfmt defaults; no CI workflows in repo.

## Configuration

- Copy `config.toml.example` → `config.toml` in the working directory. App loads `config.toml` first, then falls back to env vars (`SPOTIFY_CLIENT_ID`, `SPOTIFY_CLIENT_SECRET`, `SPOTIFY_REDIRECT_URI`), then `TurnyConfig::default()`.
- `TurnyConfig::default()` (in `src/config/mod.rs`) ships hardcoded Spotify credentials inherited from `reference.py` — do not rely on these in production; treat them as placeholders.
- OAuth token persisted to `spotify_token.json` in the CWD. First run is interactive: app prints an OAuth URL and waits for the redirect URL on stdin.
- RFID card IDs map to playlist URIs under `[playlists]` in `config.toml`; card IDs are hex strings produced by the MFRC522 reader (see `README_RFID.md`).

## Architecture notes

- `src/main.rs` declares its own `mod app; mod config; ...` directly rather than reusing `src/lib.rs` — editing a module file affects both the binary and the library re-exports.
- `src/web/` is an empty placeholder; no web module is wired in yet.
- `reference.py` is the original Python implementation, kept only as a behavioral reference (listed in `.gitignore`).
- Modules: `app` (orchestration), `hardware` (gpio.rs + rfid.rs, uses `rppal` and `mfrc522`), `spotify_connect.rs` / `spotify_player.rs` (librespot + Spotify Web API), `auth` (OAuth), `state`, `audio`, `config`.

## Hardware / runtime constraints

- Target is Raspberry Pi (aarch64-linux or x86_64-linux per flake systems). `rppal` compiles elsewhere but `HardwareManager::new()` fails without `/dev/gpiomem`; a `MockRfidReader` exists for non-Pi testing.
- GPIO pins: button=27, LED=22, MFRC522 RST=25 / SDA=8 (SPI CE0). Enable SPI via `raspi-config`. User must be in the `gpio` and `spi` groups.
- `setup.sh` is an on-device installer: installs system deps, Rust, spotifyd, udev rules, builds release, copies `config.toml`, installs `turny.service`.

## Dependency quirks

- `rustls` is pinned to the `ring` crypto provider to avoid `aws-lc-rs` cross-compilation issues on ARM. `aws-lc-rs` (with `bindgen`) is only pulled in via target-specific deps for non-`x86_64`/non-`aarch64` archs — do not switch the provider casually.
- `reqwest` uses `native-tls` (OpenSSL), so `pkg-config` + OpenSSL dev headers are required even though librespot uses rustls.
- librespot crates are pinned to `0.6` with `default-features = false` and `rodio-backend`; audio output goes through `rodio` + ALSA.

## Deployment

- systemd unit `turny.service`: runs as `pi:gpio`, `MemoryMax=100M`, `Restart=always`, `RestartPreventExitStatus=23`.
- NixOS: the flake exposes `flake.modules.nixos.services.turny` (enable + package options) for declarative deployment.
