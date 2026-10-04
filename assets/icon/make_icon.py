"""Generate the Toggle Audio application icon.

Outputs (next to this script):
    icon.svg           hand-authored vector source (same shapes as the 256 px master)
    icon-256.png       256 x 256 RGBA
    icon-48.png        48 x 48 RGBA
    toggle-audio.ico   16, 20, 24, 32, 48, 64, 128, 256 (256 PNG-compressed, rest 32-bit DIB)
    preview.png        contact sheet: 16/24/32/48/256 on light and dark, plus zooms

Usage:
    python assets/icon/make_icon.py

Requires Pillow (python -m pip install --user pillow).
"""

from __future__ import annotations

import io
import struct
import sys
from pathlib import Path

from PIL import Image, ImageDraw

HERE = Path(__file__).resolve().parent
SS = 4  # master sizes (64 px and up): draw at 4x, downsample with LANCZOS
SS_HINTED = 16  # hinted sizes (16-48 px): draw at 16x, box-filter down

BG = "#1F6FEB"  # background (blue)
FG = "#FFFFFF"  # speaker
ACCENT = "#F59E0B"  # toggle arrows (amber)

ICO_SIZES = [16, 20, 24, 32, 48, 64, 128, 256]

# ---------------------------------------------------------------------------
# Geometry
#
# Shapes are ("rrect", (x0, y0, x1, y1), radius, color), ("rect", (x0, y0, x1, y1), color)
# or ("poly", [(x, y), ...], color)
# in a `unit` x `unit` coordinate space that is scaled to the output size.
# The master design uses a 256 grid. Small sizes use hand-hinted pixel grids so
# edges land on whole pixels and nothing thinner than 2 px survives at 32 px.
# ---------------------------------------------------------------------------


def arrow(x_tail, x_tip, y, half_shaft, head_len, half_head):
    """Horizontal arrow polygon. Points right if x_tip > x_tail, left otherwise."""
    d = 1 if x_tip > x_tail else -1
    xb = x_tip - d * head_len  # base of the head
    return [
        (x_tail, y - half_shaft),
        (xb, y - half_shaft),
        (xb, y - half_head),
        (x_tip, y),
        (xb, y + half_head),
        (xb, y + half_shaft),
        (x_tail, y + half_shaft),
    ]


def pixel_arrow(x_tail, x_tip, y0, color, shaft=2, head=3):
    """Pixel-art arrow as whole-pixel rects (16/20 px): a shaft `shaft` px tall
    whose head widens by one pixel per side per column (a stepped 45 degree head)."""
    d = 1 if x_tip > x_tail else -1
    base = x_tip - d * head
    rects = [("rect", (min(x_tail, base), y0, max(x_tail, base), y0 + shaft), color)]
    for i in range(head):
        col = base + d * i if d > 0 else base - 1 - i
        grow = head - 1 - i
        rects.append(("rect", (col, y0 - grow, col + 1, y0 + shaft + grow), color))
    return rects


def speaker(x0, x1, x2, y_mid, body_half, cone_half):
    """Loudspeaker silhouette: body x0..x1, cone flaring from x1 to x2."""
    return [
        (x0, y_mid - body_half),
        (x1, y_mid - body_half),
        (x2, y_mid - cone_half),
        (x2, y_mid + cone_half),
        (x1, y_mid + body_half),
        (x0, y_mid + body_half),
    ]


def master():
    """256-unit master design (used for 64 px and up, and for the SVG)."""
    return 256, [
        ("rrect", (8, 8, 248, 248), 56, BG),
        # speaker: body 36..76, cone to 124, centred at y=128
        ("poly", speaker(36, 76, 124, 128, 30, 80), FG),
        # toggle: two opposing arrows on the right
        ("poly", arrow(140, 222, 92, 14, 38, 36), ACCENT),
        ("poly", arrow(222, 140, 164, 14, 38, 36), ACCENT),
    ]


def hinted(size):
    """Pixel-grid designs for small sizes (unit == size)."""
    if size == 16:
        return 16, [
            ("rrect", (0, 0, 16, 16), 3.5, BG),
            ("poly", speaker(1, 3, 6, 8, 2, 5), FG),
            # heads offset horizontally so the two arrows never touch
            *pixel_arrow(8, 15, 4, ACCENT),
            *pixel_arrow(14, 7, 10, ACCENT),
        ]
    if size == 20:
        return 20, [
            ("rrect", (0, 0, 20, 20), 4.5, BG),
            ("poly", speaker(2, 5, 9, 10, 2, 6), FG),
            *pixel_arrow(11, 19, 5, ACCENT),
            *pixel_arrow(18, 10, 13, ACCENT),
        ]
    if size == 24:
        return 24, [
            ("rrect", (1, 1, 23, 23), 5, BG),
            ("poly", speaker(3, 6, 10, 12, 3, 7), FG),
            ("poly", arrow(13, 21, 8.5, 1.5, 4, 3.5), ACCENT),
            ("poly", arrow(20, 12, 15.5, 1.5, 4, 3.5), ACCENT),
        ]
    if size == 32:
        return 32, [
            ("rrect", (1, 1, 31, 31), 7, BG),
            ("poly", speaker(5, 10, 16, 16, 4, 10), FG),
            ("poly", arrow(18, 28, 10.5, 1.5, 5, 5), ACCENT),
            ("poly", arrow(28, 18, 21.5, 1.5, 5, 5), ACCENT),
        ]
    if size == 48:
        return 48, [
            ("rrect", (2, 2, 46, 46), 10.5, BG),
            ("poly", speaker(7, 14, 23, 24, 6, 15), FG),
            ("poly", arrow(26, 42, 17.5, 2.5, 7, 6.5), ACCENT),
            ("poly", arrow(42, 26, 30.5, 2.5, 7, 6.5), ACCENT),
        ]
    return master()


# ---------------------------------------------------------------------------
# Rendering
# ---------------------------------------------------------------------------


def render(size: int) -> Image.Image:
    unit, shapes = hinted(size)
    ss = SS_HINTED if unit == size else SS
    k = size * ss / unit
    big = Image.new("RGBA", (size * ss, size * ss), (0, 0, 0, 0))
    draw = ImageDraw.Draw(big)
    for shape in shapes:
        if shape[0] == "rrect":
            _, (x0, y0, x1, y1), r, color = shape
            # Pillow rectangles include the end pixel.
            draw.rounded_rectangle(
                (x0 * k, y0 * k, x1 * k - 1, y1 * k - 1), radius=r * k, fill=color
            )
        elif shape[0] == "rect":
            _, (x0, y0, x1, y1), color = shape
            draw.rectangle((x0 * k, y0 * k, x1 * k - 1, y1 * k - 1), fill=color)
        else:
            _, pts, color = shape
            # Pillow fills polygons inclusive of their outline, i.e. one extra
            # supersample on the right/bottom; at 16x that is invisible.
            draw.polygon([(x * k, y * k) for x, y in pts], fill=color)
    if unit == size:
        # Hinted pixel-grid sizes: exact area coverage keeps grid-aligned edges
        # crisp; LANCZOS would bleed them into the neighbouring pixel.
        return big.reduce(ss)
    return big.resize((size, size), Image.Resampling.LANCZOS)


# ---------------------------------------------------------------------------
# SVG (hand-authored from the master geometry)
# ---------------------------------------------------------------------------


def num(v: float) -> str:
    return f"{v:g}"


def write_svg(path: Path) -> None:
    unit, shapes = master()
    lines = [
        f'<svg xmlns="http://www.w3.org/2000/svg" width="{unit}" height="{unit}" '
        f'viewBox="0 0 {unit} {unit}">',
        "  <title>Toggle Audio</title>",
    ]
    for shape in shapes:
        if shape[0] == "rrect":
            _, (x0, y0, x1, y1), r, color = shape
            lines.append(
                f'  <rect x="{num(x0)}" y="{num(y0)}" width="{num(x1 - x0)}" '
                f'height="{num(y1 - y0)}" rx="{num(r)}" fill="{color}"/>'
            )
        else:
            _, pts, color = shape
            p = " ".join(f"{num(x)},{num(y)}" for x, y in pts)
            lines.append(f'  <polygon points="{p}" fill="{color}"/>')
    lines.append("</svg>")
    path.write_text("\n".join(lines) + "\n", encoding="utf-8", newline="\n")


# ---------------------------------------------------------------------------
# ICO writer
#
# Pillow's ICO encoder stores every frame as PNG (default) or every frame as a
# BMP without the AND mask (bitmap_format="bmp"). The conventional layout, which
# every Windows loader handles, is 32-bit DIB + AND mask below 256 px and PNG at
# 256 px, so the container is assembled here.
# ---------------------------------------------------------------------------


def dib_entry(im: Image.Image) -> bytes:
    w, h = im.size
    px = im.convert("RGBA").load()
    header = struct.pack("<IiiHHIIiiII", 40, w, h * 2, 1, 32, 0, 0, 0, 0, 0, 0)
    xor = bytearray()
    and_mask = bytearray()
    row_bytes = ((w + 31) // 32) * 4
    for y in range(h - 1, -1, -1):  # bottom-up rows
        mask_row = bytearray(row_bytes)
        for x in range(w):
            r, g, b, a = px[x, y]
            xor += bytes((b, g, r, a))
            if a == 0:
                mask_row[x // 8] |= 0x80 >> (x % 8)
        and_mask += mask_row
    return header + bytes(xor) + bytes(and_mask)


def png_entry(im: Image.Image) -> bytes:
    buf = io.BytesIO()
    im.save(buf, "PNG", optimize=True)
    return buf.getvalue()


def write_ico(path: Path, images: dict[int, Image.Image]) -> None:
    sizes = sorted(images)
    blobs = [png_entry(images[s]) if s >= 256 else dib_entry(images[s]) for s in sizes]
    out = bytearray(struct.pack("<HHH", 0, 1, len(sizes)))
    offset = 6 + 16 * len(sizes)
    for s, blob in zip(sizes, blobs):
        dim = 0 if s >= 256 else s  # 0 means 256
        out += struct.pack("<BBBBHHII", dim, dim, 0, 0, 1, 32, len(blob), offset)
        offset += len(blob)
    for blob in blobs:
        out += blob
    path.write_bytes(bytes(out))


def read_icondir(path: Path) -> list[tuple[int, int, int, str]]:
    """Parse ICONDIR/ICONDIRENTRY; return (width, height, bpp, 'png'|'dib') per entry."""
    data = path.read_bytes()
    reserved, kind, count = struct.unpack_from("<HHH", data, 0)
    if reserved != 0 or kind != 1:
        raise ValueError("not an ICO file")
    entries = []
    for i in range(count):
        w, h, _, _, _, bpp, size, off = struct.unpack_from("<BBBBHHII", data, 6 + 16 * i)
        if off + size > len(data):
            raise ValueError(f"entry {i} runs past the end of the file")
        kind_ = "png" if data[off : off + 8] == b"\x89PNG\r\n\x1a\n" else "dib"
        entries.append((w or 256, h or 256, bpp, kind_))
    return entries


# ---------------------------------------------------------------------------
# Contact sheet
# ---------------------------------------------------------------------------


def write_preview(path: Path, images: dict[int, Image.Image]) -> None:
    show = [16, 24, 32, 48, 256]
    zoom = {16: 8, 24: 6, 32: 4}  # nearest-neighbour zooms for pixel inspection
    pad = 24
    row1_h = 256 + 2 * pad
    row2_h = 128 + 2 * pad
    panel_w = max(
        sum(show) + pad * (len(show) + 1),
        sum(s * z for s, z in zoom.items()) + pad * (len(zoom) + 1),
    )
    sheet = Image.new("RGB", (panel_w * 2, row1_h + row2_h), "white")
    for p, bg in enumerate(("#F3F3F3", "#202020")):
        x_off = p * panel_w
        sheet.paste(Image.new("RGB", (panel_w, row1_h + row2_h), bg), (x_off, 0))
        x = x_off + pad
        for s in show:
            im = images[s]
            sheet.paste(im, (x, pad + (256 - s) // 2), im)
            x += s + pad
        x = x_off + pad
        for s, z in zoom.items():
            im = images[s].resize((s * z, s * z), Image.Resampling.NEAREST)
            sheet.paste(im, (x, row1_h + pad), im)
            x += s * z + pad
    sheet.save(path)


def main() -> int:
    images = {s: render(s) for s in ICO_SIZES}
    images[256].save(HERE / "icon-256.png", optimize=True)
    images[48].save(HERE / "icon-48.png", optimize=True)
    write_svg(HERE / "icon.svg")
    ico = HERE / "toggle-audio.ico"
    write_ico(ico, images)
    entries = read_icondir(ico)
    found = [w for w, _, _, _ in entries]
    if found != ICO_SIZES:
        print(f"ICO size mismatch: {found}", file=sys.stderr)
        return 1
    for w, h, bpp, kind in entries:
        print(f"  {w}x{h} {bpp}bpp {kind}")
    write_preview(HERE / "preview.png", images)
    print("wrote toggle-audio.ico, icon-256.png, icon-48.png, icon.svg, preview.png")
    return 0


if __name__ == "__main__":
    sys.exit(main())
