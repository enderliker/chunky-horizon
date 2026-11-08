# Chunky for Horizon

Chunky port for [Horizon](https://playhorizon.dev) (Minecraft 26.3 reimplemented in Rust) as a native mod (`.hnmod`).

Pre-generates chunks quickly, safely, and efficiently.

## Features

- **Full Command Parity**: `/chunky` with all sub-commands:
  - `start [world] [shape] [cx cz] [rx [rz]]`
  - `pause [world]`, `continue [world]`, `cancel [world]`
  - `progress`, `quiet <seconds>`, `silent`, `limit <rate>`
  - `center [x z]`, `radius <rx> [rz]`, `corners <x1 z1 x2 z2>`
  - `shape <shape>`, `pattern <pattern>`, `world [name]`
  - `spawn`, `worldborder`, `selection`
  - `trim [world] [shape] [cx cz] [rx [rz]] <inside|outside> [filters...]`
  - `confirm`, `reload`, `debug map`, `help [page]`
- **Shapes**: Square, Circle, Rectangle, Ellipse, Diamond, Pentagon, Hexagon, Triangle, Star.
- **Walk Patterns**: Ring, Concentric, Loop, Spiral, Strip, CSV, World (existing chunks).
- **Trimming**: Fast in-place region file chunk deletion with `inhabited` and `unmodified` filter support.
- **Task Persistence**: Tasks are saved to `<world>/tasks/<world>.properties` in Chunky format.
- **Options > Mods Integration**: Configurable settings in GUI (Silent mode, Continue on restart, Force load existing, Update interval).

## Structure

```
chunky-horizon/
├── Cargo.toml                  workspace member for chunky_core tests
├── compatibility.json          hook API requirements & commit pin
├── tools/                      development verification scripts
├── NOTICE                      attribution notice
└── mixins/chunky/
    ├── horizon-mixin.toml      native mod manifest
    ├── core/                   Rust crate: shapes, patterns, anvil, task, commands
    ├── glue.rs                 game bridge: task execution, server tickets, state
    ├── command.rs              client /chunky command hook
    ├── commands.rs             client command dispatcher hook
    ├── register_commands.rs    server command registration hook
    ├── perform_command.rs      server command execution hook
    ├── tick.rs                 server tick hook (chunk generation pacing)
    ├── chunk_load.rs           server chunk load hook (ticket release)
    ├── list.rs                 options mods entry hook
    ├── settings.rs             options mods settings UI hook
    ├── click.rs                options mods click handler hook
    └── lifecycle.rs            world open/close lifecycle hook
```

## Building and Testing

```bash
# Run unit tests
cargo test

# Compile game with Chunky bundle
python3 /path/to/horizon-mods/native/tools/native.py check .

# Package .hnmod for the registry
python3 /path/to/horizon-mods/native/tools/native.py package .
```
