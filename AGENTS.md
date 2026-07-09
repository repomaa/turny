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
- **Spotify credentials are not hardcoded.** You must provide `client_id` and `client_secret` via `config.toml` or env vars. The app fails at startup with a clear error if they're missing.
- OAuth token persisted in the SQLite database (`turny.db`, `settings` table). Legacy `spotify_token.json` files are automatically migrated to the database on first load and then deleted. Authentication is handled via the web UI at `http://<pi-ip>:8080`; the Spotify OAuth callback is received at `/api/auth/callback`, and an OAuth proxy page on GitHub Pages decodes the `state` parameter to redirect back to the Pi's origin.
- RFID card IDs map to playlist URIs in a SQLite database (`turny.db`, `card_mappings` table); existing `[playlists]` entries in `config.toml` are migrated automatically on first run.

## Architecture notes

- `src/main.rs` declares its own `mod app; mod config; ...` directly rather than reusing `src/lib.rs` — editing a module file affects both the binary and the library re-exports.
- `src/web/` is a full axum web server with REST API and WebSocket support:
  - `src/web/mod.rs` — axum router, embedded SvelteKit frontend via `rust-embed`, WebSocket handler for real-time updates
  - `src/web/api.rs` — HTTP handlers for auth, cards, playlists, player controls (play/pause/next/previous), volume
  - `src/web/db.rs` — SQLite persistence (card mappings, settings, token storage, last card)
  - `src/web/spotify_api.rs` — Spotify Web API client (playlists, currently-playing)
  - `src/web/events.rs` — `WebEvent` and `PlayerCommand` types for real-time WebSocket updates
- `frontend/` — SvelteKit + Tailwind frontend (SPA mode via adapter-static, WebSocket live updates)
- `reference.py` is the original Python implementation, kept only as a behavioral reference (listed in `.gitignore`).
- Modules: `app` (orchestration), `hardware` (gpio.rs + rfid.rs, uses `rppal` and `mfrc522`), `spotify_connect.rs` (librespot + Spotify Connect), `auth` (OAuth), `state`, `audio`, `config`, `web` (axum server: api.rs, db.rs, spotify_api.rs, events.rs, mod.rs with REST + WebSocket + embedded frontend).

## Hardware / runtime constraints

- Target is Raspberry Pi (aarch64-linux or x86_64-linux per flake systems). `rppal` compiles elsewhere but `HardwareManager::new()` fails without `/dev/gpiomem`; a `MockRfidReader` exists for non-Pi testing.
- GPIO pins: button=27, LED=22, MFRC522 RST=25 / SDA=8 (SPI CE0). Enable SPI via `raspi-config`. User must be in the `gpio` and `spi` groups.
- `setup.sh` is an on-device installer: installs system deps, Rust, spotifyd, udev rules, builds release, copies `config.toml`, installs `turny.service`.

## Dependency quirks

- `rustls` is pinned to the `ring` crypto provider to avoid `aws-lc-rs` cross-compilation issues on ARM. `aws-lc-rs` (with `bindgen`) is only pulled in via target-specific deps for non-`x86_64`/non-`aarch64` archs — do not switch the provider casually.
- `reqwest` uses `native-tls` (OpenSSL), so `pkg-config` + OpenSSL dev headers are required even though librespot uses rustls.
- librespot crates are pinned to `0.8` with `default-features = false` and `alsa-backend` + `rustls-tls-native-roots`; audio output goes directly through ALSA. `rodio` is only used for the startup sound in `src/audio.rs`.

## Deployment

- systemd unit `turny.service`: runs as `pi:gpio`, `MemoryMax=100M`, `Restart=always`, `RestartPreventExitStatus=23`.
- NixOS: the flake exposes `flake.modules.nixos.services.turny` (enable + package options) for declarative deployment.
