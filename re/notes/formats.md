# Asset formats (Stage 3)

Byte-level layouts live in the doc comments of each `mgs2-formats` module;
this file records how they were found and what is still open.

File handlers are registered by `FUN_00140ae8(letter, handler)` (bit 0x100 on
the letter marks a handler that also runs on unload):

| letter | handler | format | module |
|---|---|---|---|
| b | `0x105698` | resident code image | — |
| c | `0x129b38` | cv2 | — |
| e | `0x129bd8` | evm | — |
| f | `0x11c9e0` | far | — |
| g | `0x153168` | gcx script | `gcx` |
| h | `0x144510` | hzx collision | `hzx` |
| k | `0x129ac0` | kms model | `kms` |
| l | `0x129bb0` | lt2 lighting | — |
| m | `0x11c8f8` | mar motion | — |
| p | `0x156428` | ? | — |
| s | `0x11c990` | sar | — |
| t | `0x129e48` | tri texture set | `tri` |
| z | `0x129c00` | zms (magic `0x1BA13`, table of kms) | — |

## tri (textures): done

- Raw GS VRAM upload: image block then CLUT block, both PSMCT32, 64 wide.
- Textures are PSMT4 / PSMT8 with CSM1 32-bit CLUTs; several textures share
  one CLUT block and select a 16-colour palette with `CSA`.
- Each entry's `CLAMP_1` region clamp gives its rectangle in the atlas.
- Verified visually; all 15,919 tanker textures decode.

## kms (models): done for static geometry and bind pose

- Triangle strips wind clockwise as seen from the front (in our right-handed,
  y-up space): culling Bevy's "front" faces gives the game's look, where
  room ceilings vanish under the overhead stage cameras.

- Triangle strips; restart flag in normal.w bit 15.
- Positions are `i16` millimetres, Y up; UVs are relative to the texture's
  rectangle (0..1, scale 4096).
- Objects form a bone hierarchy; `+0x20` is the offset from the parent.
  Vertices are in their bone's space. Position `w` is the vertex's weight on its
  own bone versus the parent. In bind pose, weight-0 seam vertices land on the
  parent's surface; this was checked numerically on the Gurlukovich soldier
  models.
- Open: object flags (`0x4801` objects do not fit the seam test), mesh flags
  (0x48/0x49/0x4C/0x2F8), the second/third texture slots (lightmaps?), and
  header `+0x04/+0x10/+0x14`.

## hzx (collision): geometry done

- Areas are 3D grids of groups (cells). Element coordinates are `i16`
  relative to the group origin.
- Walls are point lists with a height; floors are 3- or 4-gons with the plane
  normal * 32000 packed into the vertices' `w`.
- Verified by overlaying on the `w00a` level mesh: the walls trace the hull
  and superstructure.
- The fifth per-group list holds trap boxes: 20-byte records `i16 min x y z,
  pad, i16 max x y z, pad, u32 name` relative to the group centre (the pads
  differ between copies of a box in different cells). Scripts name them in
  `trap <box> <actor> -m <mode> -e {...}`; e.g. w00a `r_boat` covers the
  lifeboats on the 2F deck.
- The area's +04/+0C pointers are navigation nodes (see Guards, and
  `hzx.rs`).
- Open: the second wall/floor set (ids +0x400), element flags, header table
  B, and the precomputed shortest-path matrices.

## gcx (GCL scripts): done (disassembly)

- VM spec from the interpreter: statements `0x60` command / `0x70` call, the
  value types, RPN expressions with operators 1..0x17, and typed variables in
  two banks.
- Tanker scripts use only these commands: `eval chara call command if mesg
  trap print return load restart`.
- Gameplay lives in the actor types created by `chara <type> <name> ...`. The
  type hashes match the resident image's command table (`cmd_<hash>` in
  Ghidra).
- `Bs16:0x8` is the difficulty (10..50).
- Name lookup: built-ins, identifiers harvested from the stage, and JoyDict
  file names (git-ignored local copy, shown as `~name`).

## Level placement (from scripts)

Static level geometry is not found by file name. The stage script creates it
with the map-piece actor:

```
chara 0x35a2cf <instance> -k <kms> [x y z] [-k <kms> ...] -h <hzx> <area> -l <lt2>
```

One piece can list several models. The same model can appear in several
pieces with different collision areas (e.g. `w04a1` in w04a). Cutscene stages
(`d0*t`, `d00*p*`) have none.

Open: the viewer orientation may be mirrored (handedness unverified). Check
it against an asymmetric reference such as Snake's holster side once his
model is located (it is not in the w00a files).

## Script actors used by the game

| Actor | Meaning | Options |
|---|---|---|
| `0x35a2cf` | map piece | `-k model [x y z]... -h hzx area -l lt2` |
| `0x7a8ec1` | static scenery (floors, furniture, passages) | `-m model -r rx ry rz -p x y z -s sx sy sz (%) -f flags`; `-f` bit 0x100 = never drawn (helper poles) |
| `0xd1279a` | plant (breakable) | `-m model -b broken -r -p -s` |
| `0xcba568` | breakable glass | `-m model broken pieces -p` |
| `0x9aff54` | item box | `<name> -p x y z -k model -s shadow -l label -f kind (0 item, 1 ammo, 2 grenade) -i id -n count -x {on pickup}` |
| `0xebec24` | deck door (hatch) | `-k door handle -p -r -O proc (opened; $1 = 0) -h x y z x y z (handle line)` |
| `0x55b942` | sliding door | `-m model -d rx ry rz -p -s width -t -b` |
| `0x128946` | player | `-m sna_def -a snake -p x y z -d 0 dir`, name `0x28dce6` |
| `0xf7f777` | soldier | see Guards |
| `0x43f718` | camera (set from traps) | `-c type -p pos -t target ...` (not used yet) |
| `delay` | timer | `-t frames -e {block}` |
| `0xe22388` | fade | `-t frames -p proc` (proc runs when done) |

Stage exits: a door's `-O` proc (or a trap block) calls a proc that sets
Snake's next position (`Au32:0xbc/0x7f4/0xc0`, facing `0x7f8`), the story
marker `Au32:0xc4`, and then `load "<stage>"`, usually behind a `delay` or a
fade. The game runs these with the timers and fades firing immediately.
`Abit:0x7e8.5` is set once Snake's entrance walk-in is over; exit traps
check it so Snake does not leave through the door he came in by.

## trap events (`FUN_0014A448`)

Each frame the trap boxes containing an actor are compared with the
previous frame's: new boxes send `0x670d6b` (enter), kept ones `0x54a56b`
(stay), lost ones `0x61d56b` (leave). A handler's `-m` mode filters them:
an event code itself, `0x14c9` (enter and leave) or `0x1516` (all three).
The block gets `$1` box, `$2` actor, `$3` event, `$4..$6` actor position.
Handler conditions (`FUN_00149FE8`) include `-b` (button), `-d dir range`
(facing) and `-s` (stance; not modelled yet).

## CODEC.DAT (Codec conversations)

- Not encrypted: a run of GCL script records (same bytecode as stage`.gcx`),
  each starting with the stamp `0x3bb29171` and padded to 16 bytes; 451 in
  the NTSC-U disc. `mgs2-extract <iso> codec` writes them as text.
- Stage scripts name a record by id: low 24 bits x 16 = byte offset in
  CODEC.DAT (high byte: flags). `command #4a243a -o <event> <freq> <id>`
  sets the conversation for calling `freq` in that area (on enter/stay, off
  on leave); `-c <freq> <id> <mode>` is an incoming call; `-r` resets.
  14112 = 141.12 (Otacon).
- A record's `main` (`#57c8d1 "codec" ... {...}`) branches on story
  variables and calls procs whose `#01e658 <voice id> {...}` blocks hold
  the lines `#3b91eb <speaker> <?> -m str[n]`. Speakers: `0x4ab569`
  Otacon, `0x28dce6` Snake (the player actor's name). Faces (`face`,
  `#6b11d5` lip sync) and voices (VOX.DAT, the voice id) are not used yet.

## mar (motions): joint rotations and root height done

Full layout in `mgs2-formats/src/mar.rs`. Player code is in the resident
image (w00a addresses `0x3AF000..0x3B5000`).

- Joint rotation keys hold quaternions packed as 4 x 12-bit fields and are
  played back with squad. Every key of every tanker motion decodes to a unit
  quaternion (`cargo test`). A key's duration is the length of the segment
  ending at it.
- Quaternions apply directly as local joint rotations; the conjugate gives
  visibly broken poses.
- Root track (flag 0x20): fp16 values in units of 100 mm; hip heights
  0.8 / 0.42 / 0.14 m (stand / crouch / prone).
- Not decoded: tangents of the root curve, flags 0x01 / 0x04 / 0x10 / 0x40
  tracks, the `.sar` (per-motion events?) and `.var` (motion lists) files.
- Snake (`snake.mar`, in `r_tnk0`) motions identified from root motion:
  0 idle, 5 crouch idle, 7 prone idle, 17 walk (1.25 m/s), 18 run
  (3.05 m/s), 53 crouch walk. `re/scripts/mar_root.py` prints all of them.
- Some `m`-type files are `.mdl` physics models (magic `Phys`), not motions.

## Coordinate fixes found while making Snake walk

- `.kms` header `+0x34` is the model origin. Level geometry is stored centred
  and must be translated; found by correlating mesh and collision heights
  (best match equals the header value exactly in w00a and w04a).
- `.hzx` group centres are stored x, z, y. Confirmed by the area's grid
  dims/origin and by script entry points landing on floors and stairs.
- Orientation is not mirrored: Snake's holster is on his right thigh.
- Player entry points: the procs set `Au32:0xbc / 0x7f4 / 0xc0` (x y z, feet
  on a collision floor) and `0x7f8` (facing, 4096 = 360°); the player actor is
  `chara 0x128946 ... -m sna_def -a snake -p <x y z> -d 0 <dir>`.
- `r_tnk0` is the Tanker resident stage (Snake, weapons, menus, Codec, maps),
  loaded alongside every Tanker stage.

## Guards (Stage 5b)

- Actor `chara 0xf7f777 <id> -r route -n node -d x y z map -s flags -h range
  -i d0 d1 d2 -l .. -f .. -j <dogtag proc + 5 names> -q/-a <procs>`.
  Handler `cmd_f7f777` (0x223590 in w00a's resident image), options parsed by
  `FUN_00222F60`: `-i` defaults 6000, `-l` 3000, `-f` 9, `-h` 12000. Model
  `gbs_def` (0x613791; 0xd83aaf for a variant flag), motions `gbs.mar`.
  Vision set up by `FUN_00259618(vision, 0, 0x200, i[0], h, 6000)`.
- Guards are created by script proc `#ddf4ef`, reached through:
  `main` -> `command ... -s {setup procs}` -> proc `#d2b8e8` -> actor
  `0xddf5ca` (`-w proc`) -> proc `#bdd3aa` -> actor "wccomm" `0x59eda8`
  (`-e proc`) -> `#ddf4ef`. It branches on difficulty (`Bs16:0x8`) and the
  story marker `Au32:0xc4`.
- `Au32:0xc4` is set by the stage that loads this one. `d01t` (after the
  opening) sets `#8744ee` and puts Snake at (6406, 0, 16224) facing 2048.
- Routes are the `.hzx` header table at +08: `{u32 count, u32 offset, 0}`,
  40-byte points `f32 x z y, look_x; f32 look_z look_y; i16 ?, ?, ?, ?,
  flags (4 = look-at point), wait (frames, -1 = forever), direction, ?,
  action, ...`.
- Header table at +10 (0x30-byte records) holds boxes, probably event areas.
- `gbs.mar`: 0 idle, 1 walk (1.33 m/s), 2 run (4.05 m/s), 113 fall prone,
  114 lying (asleep / knocked out).
- `snake.mar`: 13 collapse onto his side (death), 14 fall face down, 176
  knocked onto his back.
- The guard AI port is described in `guard-ai.md`.

## Not started

`lt2`, `cv2`, `evm`, `far`, `o2d`, `sar`, `var`, `zms`, `r*`, and the `anm`
sound-bank pages.
