# Chunky Commands Reference

This document provides a reference for all `/chunky` commands available in Chunky for Horizon.

## General Syntax

Commands can be run from the client chat or from the server console:

```
/chunky <subcommand> [arguments...]
```

## Subcommands

### `/chunky` or `/chunky help [page]`
Displays paginated command help and usage examples.

### `/chunky start [world] [shape] [cx cz] [rx [rz]]`
Starts pre-generating chunks according to the configured selection or inline arguments:
- `world`: Dimension identifier (e.g. `minecraft:overworld`, `minecraft:the_nether`, `minecraft:the_end`).
- `shape`: Shape kind (e.g. `square`, `circle`, `rectangle`, `ellipse`, `diamond`, etc.).
- `cx cz`: Center coordinates in block coordinates.
- `rx [rz]`: Radius in blocks along X (and optionally Z).

### `/chunky pause [world]`
Pauses the active pre-generation task for the specified world (or active world), releasing all held chunk tickets.

### `/chunky continue [world]`
Resumes a paused pre-generation task for the specified world, loading saved progress from disk if necessary.

### `/chunky cancel [world]`
Cancels the generation task for the specified world and clears any in-flight chunk requests.

### `/chunky progress`
Displays the current progress of all active tasks, including percentage, chunks processed, ETA, and processing rate.

### `/chunky center [x z]`
Sets the selection center to the given block coordinates, or to the player's current position if coordinates are omitted.

### `/chunky radius <rx> [rz]`
Sets or modifies the selection radii:
- `<number>`: Sets radius to absolute value.
- `+<number>`: Increases radius by value.
- `-<number>`: Decreases radius by value.

### `/chunky corners <x1 z1 x2 z2>`
Sets the selection from two corner block coordinates, calculating the center and radii automatically.

### `/chunky shape <name>`
Sets the selection shape. Supported shapes:
- `square`
- `circle`
- `rectangle`
- `ellipse`
- `diamond`
- `pentagon`
- `hexagon`
- `triangle`
- `star`

### `/chunky pattern <name>`
Sets the chunk generation traversal pattern. Supported patterns:
- `ring` (default outwards concentric spiral)
- `concentric`
- `loop`
- `spiral`
- `strip`
- `csv`
- `world`

### `/chunky world [name]`
Sets the active world/dimension for subsequent selection commands.

### `/chunky spawn`
Sets the selection center to the world's spawn location.

### `/chunky worldborder`
Sets the selection center and radii to fit the current world border.

### `/chunky selection`
Displays a summary of the current selection parameters.

### `/chunky quiet <seconds>`
Sets the progress update interval in seconds. Set to `0` to silence periodic updates while keeping completion notices.

### `/chunky silent`
Toggles silent mode, muting all output except errors.

### `/chunky limit <rate>`
Sets a chunk generation rate limit (chunks per second). Set to `0` to disable rate limiting.

### `/chunky trim [world] [shape] [cx cz] [rx [rz]] <inside|outside> [filters...]`
Prepares a world trimming task to delete chunks inside or outside the selection from disk (`.mca` files):
- `<inside|outside>`: Specifies whether to trim chunks within or outside the shape.
- `inhabited=<ticks>`: Optional filter deleting only chunks where players spent less than (or range of) inhabited ticks.
- `unmodified=<duration>`: Optional filter deleting chunks not modified within the given time duration.
Requires `/chunky confirm` to proceed.

### `/chunky confirm`
Confirms a pending destructive action (e.g. trimming). Must be executed within 60 seconds of the prompt.

### `/chunky reload`
Reloads `config/chunky/config.json`. Only allowed when no generation tasks are running.

### `/chunky debug map`
Generates a status report counting chunks in the selection across regions (Full, Incomplete, Invalid, Not generated).
