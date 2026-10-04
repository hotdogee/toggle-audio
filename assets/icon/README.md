# Toggle Audio icon

| File | What it is |
| --- | --- |
| `icon.svg` | Vector source of truth (256 x 256 master geometry, four shapes) |
| `make_icon.py` | Generator: renders every size, writes the PNGs, the `.ico`, the SVG and the preview |
| `toggle-audio.ico` | Windows icon: 16, 20, 24, 32, 48, 64, 128, 256 px, 32-bit. 16-128 are DIB (BGRA + AND mask), 256 is PNG-compressed |
| `icon-256.png`, `icon-48.png` | RGBA exports for the README, the installer and GitHub |
| `preview.png` | Contact sheet: 16/24/32/48/256 on light (#F3F3F3) and dark (#202020), plus 8x/6x/4x zooms of 16/24/32 |

## Regenerate

From the repository root:

```powershell
python -m pip install --user pillow
python assets/icon/make_icon.py
```

The script prints the entries it finds when it re-parses the ICONDIR header of the
`.ico` it just wrote, and exits non-zero if the sizes are not exactly
16, 20, 24, 32, 48, 64, 128, 256. Output is deterministic, so a rerun with no
geometry change leaves the files byte-identical.

## Design

- **Motif.** A white loudspeaker (body plus flared cone) on the left and two
  opposing amber arrows (a "swap" sign) on the right, sitting where sound arcs
  would normally be. The two arrows stand for the two configured outputs that
  each run flips between. There are no arcs: at 16 px a speaker, an arc and two
  arrows don't fit in 14 usable pixels, and the arrows already read as "sound
  goes out this way".
- **Palette.** Two tones plus one accent: background `#1F6FEB` (blue), glyph
  `#FFFFFF`, arrows `#F59E0B` (amber). Amber on blue is a complementary pair, so
  the arrows stay distinct from the speaker even when the icon is reduced to a
  few pixels or seen by someone with a common colour-vision deficiency (the
  arrows are also much lighter than the background).
- **Shape.** Rounded square (radius ~23% of its side) with the Windows 11 flat
  look: no gradient, no shadow, no text. The master leaves 8/256 of transparent
  margin around the tile and about 12-16% padding between the tile edge and the
  glyph.
- **Small sizes are hand-hinted.** 16, 20, 24, 32 and 48 px are not scaled from
  the master. Each has its own pixel-grid geometry in `hinted()` so the edges land
  on whole pixels:
  - 16 and 20 px use stepped, pixel-art arrows (2 px shaft, a 3-column head
    that grows by 1 px per side). Anti-aliased 45 degree heads turned into grey
    blobs at this size. The arrowheads are offset horizontally so the two arrows
    never touch, and the tile fills the whole canvas.
  - 24 px keeps a 1 px transparent margin. The shafts are 3 px, the heads 4 px
    long, and the speaker shifts left 1 px to separate the heads.
  - 32 px: 3 px shafts, no stroke under 2 px. 48 px: 5 px shafts.
- **Rasterising.** The hinted sizes are drawn at 16x and box-filtered down
  (`Image.reduce`), which gives exact area coverage, so grid-aligned edges stay
  sharp. 64, 128 and 256 px are drawn from the master at 4x and downsampled with
  LANCZOS. At small sizes LANCZOS bled every edge into the neighbouring pixel and
  left a dark ring around the white speaker, so it is used only where that
  ringing can't be seen.
- **ICO layout.** Pillow's ICO encoder writes every frame as PNG, or with
  `bitmap_format="bmp"` every frame as BMP, including 256 px and without an
  AND mask. The script therefore writes the ICONDIR itself in the conventional
  layout: 32-bit DIB with an AND mask below 256 px, PNG at 256 px.

## Verification (2026-10-04)

- ICONDIR parse (in `make_icon.py`): 16, 20, 24, 32, 48, 64, 128 (32 bpp DIB), 256 (32 bpp PNG).
- Win32 `LoadImageW(..., IMAGE_ICON, n, n, LR_LOADFROMFILE)` returns an icon of
  exactly n x n for all eight sizes.
- WIC `IconBitmapDecoder` enumerates all 8 frames.
- `[System.Drawing.Icon]::new(path, n, n)` loads 16-128 exactly. For 256 it
  falls back to 128 because System.Drawing ignores PNG-compressed frames when it
  picks a size. That limitation is in System.Drawing, not in the file: Explorer
  and `LoadImage` use the 256 px frame.

## Changing the design

Edit the shapes in `master()` (which also drives `icon.svg`) and the matching
per-size entries in `hinted()`. Then rerun the script and check `preview.png`,
mainly the 16 px and 32 px zooms.
