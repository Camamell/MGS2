# Sound driver (iop_main.irx)

The IOP module `iop_main.irx` (in `MODULE.IPK`, decrypted per entry with the
key 0x7E8A6B4C + 0xA84E6B2E per word) is the MGS1 `sd` driver grown for the
PS2. Decompile: `re/iop/iop_main.c` (not committed). Port:
`mgs2_formats::sd` (music only).

## Commands from the EE (`FUN_001603f0` -> `FUN_000109cc` / `FUN_0000bd70`)

- `0x0100000n`: play song n of the loaded song data.
- `0xff00010n`: interactive music level n (tracks with level mode 1 fade to
  their level-n volume); `0xff0000ff` and others: sync flags.
- `0x01ffff03..0d`, `0x01fffffd/e/f`: fades and stops.
- `0x02xxxxxx`: sound effects; `0xf0..0xf5`: streams (see VOX/DEMO in
  `formats.md`).
- GCL `command #f2eedc -c <value>` sends a raw command; `command #d9c789`
  returns the current song.

## Timing

A hardware timer fires every 2.5 ms (`FUN_000007fc`: `USec2SysClock(2500)`)
and wakes the driver thread (`FUN_0000f6e0`), which runs the sequencer every
other wake: 200 ticks a second. Each tick a track adds its tempo to a
counter and advances one step when the counter passes 0x100.

## Stage sound pack (page 0x7F000010)

Header: four `u32 start sector, u32 id` (0x800-byte sectors). The driver
loads (`FUN_00000d20`) a wave bank, a second wave bank, sound effect data and
song data, skipping parts whose id matches what is already loaded.

- Wave bank (`FUN_00001928`): big-endian `u32 tone table offset, u32 table
  size`, the table at +0x10 (copied to tone `offset / 16` on), then
  big-endian `u32 SPU address, u32 size` and, 0x10 later, ADPCM data
  DMA'd to SPU 0x20000 + address.
- Song data (`FUN_000016cc`, loaded to 0xa2630): `u32 count`, `u32`
  offsets of each song (1-based); a song is 32 `u32` track offsets (24 bits,
  0 = unused), from the section's start.
- Tone (16 bytes, `FUN_0000ad58`): `u32` SPU address (+0x20000), base note,
  fine tune, attack exponential flag, attack (0x7f - rate), decay (0xf -
  rate), sustain mode (0 linear down, 1 exponential down, 2 linear up, else
  exponential up), sustain (0x7f - rate), sustain level, release
  exponential flag, release (0x1f - rate), pan (x2 = 0..40), volume
  decrement. Tones 0x200 on (drum notes 0x48+: tone note + 0x1b8) come from
  a resident table at 0x99c80.

## Tracks (`FUN_000033a0`, `FUN_000037c8`)

4-byte events, byte 3 the command, bytes 2, 1, 0 its parameters. Under 0x80:
a note (length in steps, gate %, velocity). Controls (table at 0x1a720):

| cmd | meaning |
|---|---|
| d0 / d1 | tempo / tempo slide |
| d2-d4, fd | tone (fd: + 0x100) |
| d5 / d6 | volume / volume slide |
| d7, d8, d9, f1 | ADSR rates and modes |
| da / db | wait for another track's signal (ef) / jump back |
| dc | move the sample start |
| dd / de | pan / pan slide (centre 0x14) |
| df / e0 | transpose / detune |
| e1 / e2 | vibrato / vibrato fade-in |
| e3 | random pitch |
| e4 | pitch sweep to a note (read ahead after a note) |
| e5 / e6 | sweep from an offset / portamento |
| e7-e8, e9-ea | counted loops (with volume and pitch change per pass) |
| eb / ec | song loop point / jump to it (end if unset) |
| ed / ee | first/second ending repeat |
| f2 / f3 | rest / tie |
| f6 / f7 | reverb send on / off |
| f8-fc, cd-cf | interactive levels: volume and fade time per level 1-8; f8 also the mode (1 = game level) |
| ff | end of track |

Pitch: semitone << 8 (+ fine tune), then the tone's base note, through the
interpolated table at 0x1ab7c (0x1000 = 48 kHz). Volume: track volume x
velocity / 256, minus the tone's decrement, through the 41-step pan curve at
0x1a9d4, times the track's level volume.

## Music in the stages (`bgmanage.c`, `cmd_70a68a`)

Its update (`FUN_00230330`) follows the alert phase (`+0x112`): sneaking ->
song 5 + level 1; alert -> SE 0x2a, song 2 + level 2; evasion -> song 3 +
level 3; caution -> song 4; with flag `DAT_0017f48c & 1` song 1 + level 4.
`bgmfader.c` (`cmd_62b1fd`) boxes fade single tracks by area (`-t track
-a x y z w h d "~stage"...`); not ported.

## Sound effects

- Command `pan << 18 | volume << 12 | id` (high byte 0), queued to
  `FUN_00010004`: ids under 0x100 use the resident table (`r_tnk0`'s pack,
  whose effect section starts with an `fe fe` map), ids from 0x100 the stage
  map (section 2's first 0x800 bytes: id - 0x100 -> entry). Entry (16
  bytes): priority, track count (up to 3), kind (nonzero: volume and pan
  from the command through the 65-step curve at 0x1aa78), group, three
  sequence offsets (from 0x1000 after the table). They play on 12 effect
  tracks (0x20-0x2b) with the song engine.
- Wave banks with a table offset of 0x1000 and up are kept in IOP memory
  (0x1ba70) and streamed into SPU 0x9020 + channel * 0x1000 when an effect
  track keys one on (`FUN_000121e0`); the resident bank (offset 0) fills tones
  0..0x93, and its entries past the first address drop become the drum
  tones 0x200+.
- Game calls: `FUN_00114b30(pan, volume, id)` direct, `FUN_00114ce8(id,
  position, mode)` positional (pan and volume from the camera,
  `FUN_00114a10`). Tanker ids seen: usp.c fires 0x52 (weapon 0, the M9),
  1 (USP/SOCOM), 0x30, 0xa7-0xaa by weapon, 4 when empty; wt_door.c 0x6f;
  raiden.c 7 / 8 (footsteps; 0x589 / 0x58a under a flag), 0x19 / 0x1a / 0x25
  when hurt; bgmanage 0x2a on alert; menus 0x14 / 0x15 / 0x17 / 0x22 / 0x23.