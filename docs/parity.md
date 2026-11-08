# Chunky Parity for Horizon

This document details the parity between the original Java edition of Chunky (1.4.45) and the native port for Horizon.

## Commands and Sub-commands

| Command | Status | Notes |
|---|---|---|
| `/chunky` | 100% Parity | Shows interactive help |
| `/chunky help [page]` | 100% Parity | Exact pagination matching upstream Chunky messages |
| `/chunky start [world] [shape] [center] [radii]` | 100% Parity | Positional arguments in Chunky's exact order |
| `/chunky pause [world]` | 100% Parity | Pauses the task and releases held tickets |
| `/chunky continue [world]` | 100% Parity | Resumes task from file or memory |
| `/chunky cancel [world]` | 100% Parity | Cancels task and frees in-flight chunk tickets |
| `/chunky progress` | 100% Parity | Reports percentage, chunks, ETA, and generation rate |
| `/chunky center [x z]` | 100% Parity | Manual coordinates or player position |
| `/chunky radius <rx> [rz]` | 100% Parity | Absolute or incremental (+/-) radii |
| `/chunky corners <x1 z1 x2 z2>` | 100% Parity | Computes center, radii, and fitting shape |
| `/chunky shape <name>` | 100% Parity | 9 supported shapes |
| `/chunky pattern <name>` | 100% Parity | 7 supported walk patterns |
| `/chunky world [name]` | 100% Parity | Selects active world or dimension |
| `/chunky spawn` | 100% Parity | Reads the world spawn point |
| `/chunky worldborder` | 100% Parity | Reads world border bounds via `native_world_border` |
| `/chunky selection` | 100% Parity | Displays current selection parameters |
| `/chunky quiet <seconds>` | 100% Parity | Configures update interval |
| `/chunky silent` | 100% Parity | Toggles silent mode |
| `/chunky limit <rate>` | 100% Parity | Chunk generation rate limiter with 100 ms slots |
| `/chunky trim ...` | 100% Parity | Trims `.mca` region files with `inhabited` and `unmodified` filters |
| `/chunky confirm` | 100% Parity | Safe confirmation for destructive actions with 60s timeout |
| `/chunky reload` | 100% Parity | Reloads configuration from `config/chunky/config.json` |
| `/chunky debug map` | 100% Parity | Scans and tallies chunk statuses across regions |

## Supported Shapes

1. **Square**
2. **Circle**
3. **Rectangle**
4. **Ellipse**
5. **Diamond**
6. **Pentagon**
7. **Hexagon**
8. **Triangle**
9. **Star**

## Supported Walk Patterns

1. **Ring** (concentric spiral outwards from center)
2. **Concentric**
3. **Loop** (walks X then Z)
4. **Spiral**
5. **Strip** (bands of 64 chunks)
6. **CSV** (reads X, Z coordinate lines)
7. **World** (existing chunks from region files)
