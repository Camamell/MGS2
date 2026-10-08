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
cargo run --release -p mgs2-extract -- "path/to/mgs2.iso" codec
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
| Z | prone / crawl (lower to the guards' eyes and bullets; fits under low gaps) |
| Space | action: open doors (stage exits load the next stage); at a deck edge, hang over the railing (A/D shimmy, Space climb up, Z let go; the grip gauge runs down); some traps react to it too |
| F (hold, release) | aim (A/D turn) and fire the M9 tranquilizer; aiming at an unaware guard from behind holds him up, and from his front he gives up his dog tags |
| V | punch (three in a row knock a guard out) |
| R | equip / unequip a ration (revives Snake at zero life) |
| K | call the Codec (141.12); Space next line, Esc hang up |
| Enter | continue after a game over |
| Q / E, right mouse drag | turn the camera |
| F1 | collision, plus patrol routes |
| F2 | guard vision cones |
| F3 | stage cameras on/off |

Testing hooks: `--auto-walk <seconds>` holds forward (`--walk-from`),
`--cam-yaw/--cam-dist/--cam-pitch`, `--follow-guard N`, `--trace` (guard
states every second, trap events), `--spawns` (every actor the script
creates), `--snake-at=x,y,z` and `--snake-dir` (4096 = full turn),
`--use-door N --door-at <seconds>`, `--fire-at <seconds>`, `--punch-at
t1,t2,..`, `--screenshot out.png --shot-at <seconds>`.

What works:

- Level geometry plus the script's scenery, plants and glass; item boxes
  you can pick up (inventory kept across stages).
- Guards see, hear running, investigate, raise the alert, chase along the
  navigation graph and shoot (aim, bursts, reloads as in the game's code).
  Darted or punched guards fall asleep and later return to their route;
  guards who find them wake them and raise caution.
- Snake's life follows the difficulty (200/150/100/75/50); he collapses with
  his own death motion at zero.
- Doors and traps run the stage script's own handlers, so walking through a
  door or into an exit area loads the next stage with Snake where the
  script puts him (after his walk-in). Difficulty settings come from the
  title screen's script.
- The stage scripts' own cameras (follow-in-box and fixed, switched by traps)
  frame each area as in the game, with faces culled the way the PS2 does so
  ceilings drop away under overhead views; elsewhere (or with F3) a follow
  camera keeps walls and decks out of the way.

Assumptions (not taken from the game yet): the M9 starts with 15 darts,
sleep and knock-out times, reload pause and fire range.

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
