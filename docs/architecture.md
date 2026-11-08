# Chunky for Horizon Architecture

This document outlines the architecture of Chunky for Horizon, describing how the port maps upstream Chunky components into Horizon's native modding ecosystem.

## Overview

Unlike standard Java mod platforms (Fabric, Forge, NeoForge, Spigot/Paper) which run in a JVM with multithreaded chunk generation futures, Horizon is a native Rust engine. In Horizon:
- The game is ticked at 20 ticks per second.
- Chunk loading, generation, and ticking on the server are managed through chunk ticket mechanics (`native_force_chunk_in` / `native_force_chunk`).
- Bundles are injected using `horizon-mixins` into Horizon's source tree before compilation.

```
+-------------------------------------------------------------+
|                      Horizon Game Engine                     |
|                                                             |
|  [native_command]       [native_server_tick]  [native_mods]  |
|         |                        |                  |       |
+---------|------------------------|------------------|-------+
          |                        |                  |
          v                        v                  v
+-------------------------------------------------------------+
|                     mixins/chunky/ (Glue)                   |
|                                                             |
|   - command.rs / perform_command.rs                         |
|   - tick.rs (paces generation, checks rate limiter)         |
|   - chunk_load.rs (receives loaded chunks, releases ticket) |
|   - list.rs / settings.rs / click.rs (Options > Mods UI)    |
|   - glue.rs (thread-safe State, task coordination)          |
+-------------------------------------------------------------+
                               |
                               v
+-------------------------------------------------------------+
|                  chunky_core (Standalone Crate)             |
|                                                             |
|   - selection.rs: Selections, bounding boxes               |
|   - shape.rs: 9 mathematical shapes                         |
|   - pattern.rs: 7 chunk walk patterns (Ring, Hilbert, etc.) |
|   - task.rs: GenerationTask, RateLimiter, Progress          |
|   - anvil.rs: RegionFile MCA parser (Zlib, Flate2, LZ4)     |
|   - trim.rs: Inhabited & Unmodified trimming logic          |
|   - command.rs: Brigadier-compatible command parser         |
|   - messages.rs: Formatting & translation strings           |
|   - config.rs: config.json persistence                      |
+-------------------------------------------------------------+
```

## Component Breakdown

### 1. `chunky_core`
A standalone, pure Rust library crate located in `mixins/chunky/core`. It depends only on crates already locked in Horizon's dependencies (`serde`, `serde_json`, `flate2`, `lz4_flex`, `twox-hash`). It has zero engine dependencies, enabling full unit test execution via `cargo test`.

### 2. Chunk Generation Pacing (`Task::next_to_load` & Server Tick)
- Chunky's original threading model parked worker threads on a rate limiter semaphore.
- In Horizon, generation is driven non-blockingly inside `native_server_tick`:
  - Up to 16 chunk decisions are evaluated per tick via `task.next_to_load(now, cache)`.
  - Chunks already fully generated on disk are counted instantly without acquiring permits or triggering tickets.
  - Chunks requiring generation request an authoritative ticket via `game.native_force_chunk_in("chunky:task/<world>", dimension, [x, z], true)`.
  - When Horizon finishes generating/loading the chunk, `native_chunk_load` is triggered.
  - The glue intercepts `native_chunk_load`, records the chunk as completed in `task.loaded()`, and releases the ticket (`native_force_chunk_in(..., false)`).

### 3. File System & Task Persistence
- Task progress files are saved in `<world_dir>/tasks/<world>.properties`, fully compatible with upstream Chunky task files.
- Region trimming operates directly on Anvil `.mca` files under `<world_dir>/dimensions/<ns>/<dim>/region` (and entity/poi directories).
