"""Generate tray icons for macOS menu bar and Windows/Linux notification area.

macOS variant (tray-icon-{N}.png): the bare S silhouette stamped as solid
white on a transparent background. macOS menu bars are dark, so a bright
white glyph is visible without template inversion.

Windows/Linux variant (tray-icon-color-{N}.png): the full colored app icon
resampled to tray sizes. Windows taskbars don't auto-tint, and a single-tone
silhouette is invisible against either light or dark taskbars, so we use the
branded artwork which has its own contrast in both themes.

Inputs:
    src-tauri/icons/icon-source.png   bare artwork on transparent background
    src-tauri/icons/icon.png          full colored app icon (rounded square)

Outputs:
    src-tauri/icons/tray/tray-icon-{16,20,24,32}.png         (macOS)
    src-tauri/icons/tray/tray-icon-color-{16,20,24,32}.png   (Windows/Linux)
"""
from __future__ import annotations

from pathlib import Path

import numpy as np
from PIL import Image

REPO = Path(__file__).resolve().parent.parent
SOURCE = REPO / "src-tauri" / "icons" / "icon-source.png"
COLOR_SOURCE = REPO / "src-tauri" / "icons" / "icon.png"
TRAY_DIR = REPO / "src-tauri" / "icons" / "tray"
SIZES = (16, 20, 24, 32)
INSET = 0.0         # fraction of canvas left blank around the glyph
GLYPH_RGB = (255, 255, 255)  # tray template; macOS auto-tints for menu bar
SSAA = 4            # render oversized then downscale for clean small sizes
ALPHA_THRESHOLD = 8 # alpha levels below this become fully transparent


def silhouette(src: Image.Image) -> Image.Image:
    """Convert source artwork to a solid-black RGBA silhouette."""
    bbox = src.getbbox()
    if bbox:
        src = src.crop(bbox)
    alpha = np.array(src.split()[3])
    alpha = np.where(alpha < ALPHA_THRESHOLD, 0, alpha).astype(np.uint8)
    h, w = alpha.shape
    rgba = np.zeros((h, w, 4), dtype=np.uint8)
    rgba[..., 0] = GLYPH_RGB[0]
    rgba[..., 1] = GLYPH_RGB[1]
    rgba[..., 2] = GLYPH_RGB[2]
    rgba[..., 3] = alpha
    return Image.fromarray(rgba, mode="RGBA")


def render_at(silh: Image.Image, size: int) -> Image.Image:
    big = size * SSAA
    canvas = Image.new("RGBA", (big, big), (0, 0, 0, 0))
    target = int(big * (1.0 - 2 * INSET))
    sw, sh = silh.size
    scale = min(target / sw, target / sh)
    new_size = (max(1, int(sw * scale)), max(1, int(sh * scale)))
    scaled = silh.resize(new_size, Image.LANCZOS)
    cx = (big - new_size[0]) // 2
    cy = (big - new_size[1]) // 2
    canvas.alpha_composite(scaled, (cx, cy))
    return canvas.resize((size, size), Image.LANCZOS)


def render_color(src: Image.Image, size: int) -> Image.Image:
    """Downscale the full-color app icon to a tray size with SSAA."""
    big = size * SSAA
    scaled = src.resize((big, big), Image.LANCZOS)
    return scaled.resize((size, size), Image.LANCZOS)


def main() -> None:
    if not SOURCE.exists():
        raise SystemExit(f"missing source: {SOURCE}")
    if not COLOR_SOURCE.exists():
        raise SystemExit(f"missing color source: {COLOR_SOURCE}")
    src = Image.open(SOURCE).convert("RGBA")
    color_src = Image.open(COLOR_SOURCE).convert("RGBA")
    silh = silhouette(src)
    TRAY_DIR.mkdir(parents=True, exist_ok=True)
    for size in SIZES:
        out = TRAY_DIR / f"tray-icon-{size}.png"
        render_at(silh, size).save(out, format="PNG", optimize=True)
        print(f"wrote {out}  size={size}")
        color_out = TRAY_DIR / f"tray-icon-color-{size}.png"
        render_color(color_src, size).save(color_out, format="PNG", optimize=True)
        print(f"wrote {color_out}  size={size}")


if __name__ == "__main__":
    main()
