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
- Open: the second wall/floor set (ids +0x400), element flags, header tables
  A/B, the fifth per-group list, and the area's other pointers (navigation?).

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
- `gbs.mar`: 0 idle, 1 walk (1.33 m/s), 2 run (4.05 m/s).
- Still a stand-in: the guards' decision logic, the meaning of the vision
  parameters beyond `i[0]` and the 0x200 angle, hearing, and navigation
  (guards move in straight lines, so they get stuck on walls when chasing).

## Not started

`mar` (motion archives; needed to animate characters), `lt2`, `cv2`, `evm`,
`far`, `o2d`, `sar`, `var`, `zms`, `r*`, and the `anm` sound-bank pages.
