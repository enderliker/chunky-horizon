# Changelog

All notable changes to Chunky for Horizon will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [1.0.3] - 2026-11-12

### Fixed
- Fixed multi-strip chunk selection in `pattern.rs`: corrected strip X-offset calculation (`strip_x = current / (WIDTH * size_z) * WIDTH`), resolving overlapping strips and ensuring 100% unique chunk coverage without gaps or duplicates for selections exceeding 64 chunks in width (e.g. 141×141 selections, non-square rectangles, and ellipses).
- Fixed ticket cleanup during world shutdown in `glue.rs`: captured in-flight held chunks before calling `task.stop(false)`, ensuring all force-load tickets are properly released to the game engine when the world closes.
- Reconciled licensing and attribution: updated `core/Cargo.toml` to `GPL-3.0-or-later`, updated `NOTICE` to accurately attribute original Chunky (pop4959 and contributors) under GPL-3.0-or-later at pinned commit `c4274bd539d1d79a15fe1e73afb2eface64d625f`, packaged `NOTICE` into the distributed `.hnmod` bundle (`mixins/chunky/data/NOTICE`), and added an Attribution & License section to `README.md`.

## [1.0.2] - 2026-11-09

### Fixed
- Fixed client command interception preventing server-thread execution in single-player and LAN multiplayer: `command.rs` now skips client-side claiming when connected to an integrated single-player or remote/LAN server (`game.remote.is_some()`), allowing Horizon to forward `/chunky` commands via chat packets to the authoritative server thread where `native_perform_command` handles them and `native_server_tick` drives generation.
- Added client/server command routing test coverage in `daily_use.rs` verifying command forwarding parity across integrated single-player, LAN chat, and dedicated console.

## [1.0.1] - 2026-11-08

### Added
- Dedicated server console execution support (`native_perform_console_command` and `Sender::Console`) enabling headless CLI terminal execution without logged-in players.
- Resilient `/chunky` and `chunky` command prefix handling on server console and multiplayer chat.
- Daily-use integration test suite (88 tests) covering generation boundaries, pause/resume parity, world lifecycle, mid-generation edits, and task slot freeing.

### Fixed
- Fixed 3970-chunk stall in ring traversal iterator when crossing concentric ring boundaries.
- Fixed `/chunky pause` continuing to generate and tick by immediately transitioning state to paused, removing from active task map, releasing tickets, and halting progress notifications.
- Fixed deadlock when requested chunks were already loaded into memory.
- Fixed completed tasks retaining active slot records, preventing subsequent `/chunky start` with new radii or shapes without confirmation prompts.
- Cleaned up finished and cancelled task files to prevent stale task states from blocking subsequent pre-generation tasks.

## [0.1.0] - 2026-11-08

### Added
- Complete native Horizon port of Chunky (100% feature parity with Chunky Java 1.4.x).
- Full suite of generation shapes (`circle`, `square`, `diamond`, `star`, `triangle`, `pentagon`, `hexagon`, `ellipse`, `rectangle`, `polygon`).
- Full suite of traversal patterns (`concentric`, `loop`, `spiral`, `strip`, `csv`).
- Region file inspection, trimming, and chunk deletion with `trim [inside|outside] [filters...]`.
- Command tree with 24 Brigadier subcommands registered for Horizon server and client.
- Dynamic tab-completion integration via `native_tab_complete_v2` for subcommands, shapes, patterns, worlds, and trim options.
- Official paginated `/chunky help [page]` formatting with 8 commands per page across 3 pages.
- Native task persistence, rate-limiting, and pause/continue/cancel state management.
- Comprehensive unit test suite with 86 tests covering geometry, shapes, patterns, Anvil format, task engine, and command parsers.

### Fixed
- Fixed `/chunky` with no arguments displaying `[Chunky] chunky help_1` instead of the official paginated help menu.
- Fixed missing client-side Brigadier tab-completion and argument suggestions.
