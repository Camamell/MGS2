# STAGE.DAT format (MGS2 PS2 NTSC-U, SLUS-201.44 v1.01)

All function addresses refer to `SLUS_201.44` as loaded by Ghidra.

## Disc files

`FUN_00100418` opens the `.DAT` archives via `cdrom0:\MGS2\%s;1` (dev builds
used `pfs0:/mgs2/%s` and `host0:./%s`). The table at `0x17F408` holds
`{name, lsn, device}` per archive; `DAT_0017f40c` is STAGE.DAT's start LSN.

## Cipher (`FUN_00100688`)

XOR over little-endian u32 words; after each word
`key = key * 0x02E90EDD + step`. A trailing partial word is processed whole.

| Use | key | step | source |
|---|---|---|---|
| TOC | `seed` (first u32 of file) | `seed ^ 0xF0F0` | `FUN_00100d80` |
| stage header | `H*0x81 + seed - 0x5876DA27` | `H*0x81 + 0x7A88FB59` | `FUN_001011c0` |
| zlib page | `(v^0x9385) \| (v^0xF6F3)<<16` | `(v^0x9385) * 0x116` | `FUN_00100658` |

`H = str_code(stage_name)`; `v` = first u16 of the stored page. After
decrypting a page, add `0x08F3` to its first u16 to get the zlib header
(`78 9C`); then inflate (zlib 1.1.3, `FUN_00100780`).

`str_code` (`FUN_001408b8`): `h = ((h<<5)|(h>>19)) + c` masked to 24 bits per
byte; returns 1 instead of 0.

## Table of contents (`FUN_00100d80`, `FUN_00100e40`)

```
+0  u32 seed                  plaintext
+4  u32 info                  encrypted; (info >> 16) = header sectors
+8  u32 count                 low 16 bits = number of stages (115)
+C  u32 ?
+10 count x { char name[8]; u32 sector }   same key stream continues
```

## Stage (`FUN_001011c0`, `FUN_001015a0`)

```
u32 n
n x { u32 a; u32 b }          encrypted with the stage cipher; header padded to 0x800
pages...                      each sector-aligned, in entry order
```

Entry list grammar:

- `a = 0x7F010000`: resident page; loaded straight to `0x1F1480`.
- `a = 0x7F000001/2/3`: cached data page (`FUN_00101078` maps to load modes 0/1/2).
- `a = 0x7F000010`: sound data page; skipped by the stage loader (plaintext).
- `b` of a page-start entry = unpacked size.
- If the next entry has `a & 0xFF000000 == 0x7E000000`, the page is zlib
  compressed and `a & 0xFFFFFF` is its stored size.
- File entries: `a = type << 24 | str_code(basename)`, `b` = offset in page.
- `a = 0x7F000000` closes the page; `b` = end offset (size of the last file).

`FUN_00140c18` registers each file in the resource cache and dispatches
on `type` (0..26) through the handler table at `0x1BA250`.

## File types seen (all 115 stages)

| type | letter | provisional ext | notes |
|---|---|---|---|
| 0 | a | — | sound bank data in `0x7F000010` pages (PS-ADPCM + sequences) |
| 1 | b | bpk | resident code+data image, one per stage, named after the stage |
| 2 | c | cv2 | ? |
| 4 | e | e | ? |
| 6 | g | gcx | GCL script, one per stage |
| 7 | h | hzx | collision, one per stage |
| 10 | k | kms | model, magic `71 13 C8 00` |
| 11 | l | lt2 | lighting |
| 12 | m | mar | motion archive, magic `MARa` |
| 14 | o | o | ? |
| 17 | r | rat | ? |
| 18 | s | sar | magic `F6 54 01 00` |
| 19 | t | tri | texture |
| 21 | v | var | magic `12 21 9A 00` |

Extension names other than the letter are guesses until each format's loader
is identified in the handler table.

## Resident code image (type 1)

Each stage has its own build of the gameplay code (~2 MB, ~6000 functions,
~300 source files such as `ab_mind.c`, `c_box.c`, `bomb_chaff.c`), linked to
run at `0x1F1480` against the main ELF.

```
+0  u32 load address (0x1F1480)
+4  u32 end of image data
+8  u32 end of bss
+C  u32 -> script command table: { u32 str_code(name); u32 handler } ..., 0
```

The table maps GCL command / actor names (hashed) to their constructor;
handlers point into both the image and the ELF. `re/scripts/LoadResident.java`
maps an image into the Ghidra program and labels handlers `cmd_<hash>`.
