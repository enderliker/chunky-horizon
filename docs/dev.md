# Development Environment

Instructions for building, checking, and packaging Chunky for Horizon.

## Toolchain

| Tool | Purpose |
|---|---|
| Rust / Cargo | Rust toolchain (stable, 2024 edition supported) |
| `horizon-mods` | Native mod build tools (`native/tools/native.py`) |
| `cargo-horizon-mod` | Registry validation CLI (`cargo install --git https://github.com/horizon-rust/horizon cargo-horizon-mod`) |
| Horizon client | Runtime for manual / in-game verification |

## Linux Build Dependencies (Ubuntu / WSL)

Horizon client links system libraries:

```sh
sudo apt update
sudo apt install -y build-essential pkg-config libasound2-dev libudev-dev \
  libxkbcommon-dev libwayland-dev libx11-dev libxcursor-dev libxi-dev libxrandr-dev
```

## Common Commands

```sh
# Run unit tests on chunky_core (independent of Horizon)
cargo test

# Code style and lint checks
cargo fmt --check
rustfmt --edition 2024 --check mixins/chunky/*.rs
cargo clippy --all-targets -- -D warnings

# Compile the game with Chunky against the pinned Horizon revision
python3 /path/to/horizon-mods/native/tools/native.py check "$PWD"

# Check against latest Nightly
python3 /path/to/horizon-mods/native/tools/native.py check "$PWD" --horizon latest

# Package the .hnmod bundle
python3 /path/to/horizon-mods/native/tools/native.py package "$PWD"

# Run complete development check (fmt, clippy, test, check, package)
bash tools/dev-check.sh
```
