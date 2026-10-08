# mgs2-tanker

A reverse-engineering project to rebuild the Tanker chapter of
*Metal Gear Solid 2: Sons of Liberty* (PS2, NTSC-U, SLUS-201.44) in Rust on
the Bevy engine.

No game data lives in this repository. Every tool reads from your own disc
image at run time.

## Layout

| Path | Contents |
|---|---|
| `crates/mgs2-formats` | Library: ISO 9660 reader, STAGE.DAT cipher and archive reader |
| `crates/mgs2-extract` | CLI to list and extract stages |
| `crates/mgs2-bevy` | Shared Bevy code: stage loading, meshes/materials, skinning, fly camera |
| `crates/mgs2-viewer` | Stage and model viewer |
| `crates/tanker` | The game |
| `re/notes` | Format documentation with references to game functions |
| `re/scripts` | Ghidra scripts and Python prototypes |
| `re/disc`, `re/ghidra`, `extracted` | Local only (git-ignored): copied executable, Ghidra project, extracted data |

## Extracting

```bash
cargo run --release -p mgs2-extract -- "path/to/mgs2.iso" stages
cargo run --release -p mgs2-extract -- "path/to/mgs2.iso" list w00a
cargo run --release -p mgs2-extract -- "path/to/mgs2.iso" extract --tanker -o extracted
cargo run --release -p mgs2-extract -- extracted textures -o extracted-png
cargo run --release -p mgs2-extract -- extracted models --png-dir extracted-png -o extracted-obj
cargo run --release -p mgs2-extract -- extracted scripts -o extracted-gcl
```

`cargo test` also parses every extracted file when `extracted/` exists.

Optional: put a [JoyDict](https://github.com/Joy-Division/JoyDict) checkout at
`re/third_party/JoyDict-master` (git-ignored) to resolve more hashes in
script output. Dictionary-only matches are shown as `~name`.

## Game (work in progress)

```bash
cargo run --release -p tanker -- --iso "path/to/mgs2.iso" w00a
```

The stage script runs through a GCL interpreter, which creates the level
geometry and the guards for the chosen difficulty (`--difficulty 10..50`,
default 30 = Normal). Snake arrives the way the previous stage's script sends
him (`--from d01t` by default for w00a: the deck after the opening). The
Tanker resident data (`r_tnk0`: Snake's model, motions, weapons) is loaded
too.

| Input | Action |
|---|---|
| W A S D | run (camera-relative) |
| Shift | walk |
| C | crouch |
| Space | action: open doors (stage exits load the next stage) |
| Enter | continue after a game over |
| Q / E, right mouse drag | turn the camera |
| F1 | collision, plus patrol routes |
| F2 | guard vision cones |

Testing hooks: `--auto-walk <seconds>` holds forward, `--cam-yaw/--cam-dist/
--cam-pitch`, `--follow-guard N`, `--trace` (guard states every second),
`--spawns` (every actor the script creates), `--snake-at=x,y,z` (mm),
`--use-door N --door-at <seconds>` (open a door without walking there),
`--screenshot out.png --shot-at <seconds>`.

Guards see, investigate, raise the alert, chase along the navigation graph
and shoot (aim, bursts, reloads as in the game's code); Snake's life follows
the difficulty (200/150/100/75/50). Doors run the stage script's own exit
procs, so walking through a deck door loads the next stage with Snake where
the script puts him. Difficulty settings come from the title screen's script.

## Viewer

```bash
cargo run --release -p mgs2-viewer -- --iso "path/to/mgs2.iso" w00a
```

Set `MGS2_ISO` to skip `--iso`. Level geometry is placed the way the game
does it, from the stage script's map-piece actors.

| Input | Action |
|---|---|
| Right mouse + move | look |
| W A S D / Q E | fly / down, up (Shift = fast) |
| `[` `]` | previous / next Tanker stage |
| C / V | collision overlay (walls red, floors green) / second collision set |
| L | toggle lighting (default unlit, closest to the PS2 look) |
| B, then N / P | model browser: every model in the stage |
| F / G | overview / jump to the main floor |

Other options: `--collision`, `--lit`, `--browse <index>`,
`--cam=x,y,z,tx,ty,tz` (mm), `--screenshot out.png` (render, save, exit) and
`--check` (load every Tanker stage and print a summary).

## Ghidra setup

- Ghidra 12.1.4 with the
  [ghidra-emotionengine-reloaded](https://github.com/chaoticgd/ghidra-emotionengine-reloaded)
  extension (language `r5900:LE:32:default`), running on JDK 21.
- `re/ghidra/mgs2` contains `SLUS_201.44`, with the `w00a` resident gameplay
  code mapped at `0x1F1480` (`re/scripts/LoadResident.java`).
- `re/scripts/DumpAll.java` writes every decompiled function to
  `re/ghidra/decomp/all.c`. `re/scripts/fn.py` prints functions from it by
  name or address.

## Status

1. Tooling: done
2. STAGE.DAT unpacking: done
3. Asset formats: textures, models, collision and scripts done; motions
   (`mar`) and minor formats open (see `re/notes/formats.md`)
4. Bevy viewer: done (static levels, collision overlay, model browser)
5. Gameplay: in progress
   - done: motion decoding (joint rotations, root height), Snake spawned
     from script entry points, run/walk/crouch with collision, stairs, GCL
     interpreter for stage setup, guards with routes, guard AI ported from
     the game (perception, suspicion meter, discovery, alert, evasion,
     caution, shooting), Snake's life and game over, door transitions
     between stages, an obstacle-aware camera
   - next: items/weapons (pickups, M9, holding up guards), traps
     (script area triggers), reinforcements and radio reports, cutscenes
     and Codec
