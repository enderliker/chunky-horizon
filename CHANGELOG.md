# Changelog

All notable changes to Chunky for Horizon will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0] - 2026-11-08

### Added
- Complete native Horizon port of Chunky (100% feature parity with Chunky Java 1.4.x).
- Dedicated server console execution support (`native_perform_console_command` and `Sender::Console`) enabling CLI terminal execution without logged-in players.
- Full suite of generation shapes (`circle`, `square`, `diamond`, `star`, `triangle`, `pentagon`, `hexagon`, `ellipse`, `rectangle`, `polygon`).
- Full suite of traversal patterns (`concentric`, `loop`, `spiral`, `strip`, `csv`).
- Region file inspection, trimming, and chunk deletion with `trim [inside|outside] [filters...]`.
- Command tree with 24 Brigadier subcommands registered for Horizon server and client.
- Dynamic tab-completion integration via `native_tab_complete_v2` for subcommands, shapes, patterns, worlds, and trim options.
- Official paginated `/chunky help [page]` formatting with 8 commands per page across 3 pages.
- Native task persistence, rate-limiting, and pause/continue/cancel state management.
- Comprehensive integration and daily-use test suite (95 tests) covering geometry, shapes, patterns, Anvil format, task engine, lifecycle, world reload, and mid-generation updates.

### Fixed
- Fixed `/chunky` with no arguments displaying `[Chunky] chunky help_1` instead of the official paginated help menu.
- Fixed missing client-side Brigadier tab-completion and argument suggestions.
- Fixed 3970-chunk stall in ring traversal iterator when crossing concentric ring boundaries.
- Fixed `/chunky pause` continuing to generate and tick by immediately transitioning state to paused, removing from active task map, releasing tickets, and halting progress notifications.
- Fixed deadlock when requested chunks were already loaded into memory.
- Fixed completed tasks retaining active slot records, preventing subsequent `/chunky start` with new radii or shapes without confirmation prompts.
