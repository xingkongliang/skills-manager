"""Compose a macOS-compliant app icon from a bare artwork source.

macOS Big Sur+ no longer auto-masks Dock icons; apps must ship the squircle
themselves. This script wraps the source artwork in Apple's standard
continuous-corner squircle (G2 superellipse, n=5) on a 1024x1024 canvas with
an 824x824 icon body, matching Apple's macOS app icon production template.

Usage:
    python scripts/build_macos_icon.py
    # then regenerate all bundle assets:
    npx tauri icon src-tauri/icons/icon.png

Inputs:
    src-tauri/icons/icon-source.png   bare artwork on a transparent background
                                      (full bleed, square aspect preferred)

Output:
    src-tauri/icons/icon.png          1024x1024 squircle-wrapped icon

Tunables:
    INNER_FRACTION  how much of the squircle the artwork fills (0..1)
    SQUIRCLE_N      superellipse exponent; ~5 matches Apple's continuous corner
    SSAA            mask supersampling factor for anti-aliased edges
"""
from __future__ import annotations

from pathlib import Path

import numpy as np
from PIL import Image, ImageFilter

REPO = Path(__file__).resolve().parent.parent
ICON_DIR = REPO / "src-tauri" / "icons"
SOURCE = ICON_DIR / "icon-source.png"
OUTPUT = ICON_DIR / "icon.png"

CANVAS = 1024
BODY = 824
SQUIRCLE_N = 5.0
INNER_FRACTION = 0.92
SSAA = 8


def make_squircle_mask(size: int, n: float) -> Image.Image:
    """Vectorised superellipse mask, supersampled then downscaled for AA."""
    big = size * SSAA
    half = big / 2.0
    coords = (np.arange(big) - half + 0.5) / half
    xx, yy = np.meshgrid(coords, coords, indexing="xy")
    inside = (np.abs(xx) ** n + np.abs(yy) ** n) <= 1.0
    arr = inside.astype(np.uint8) * 255
    return Image.fromarray(arr, mode="L").resize((size, size), Image.LANCZOS)


def main() -> None:
    if not SOURCE.exists():
        raise SystemExit(f"missing source artwork: {SOURCE}")

    src = Image.open(SOURCE).convert("RGBA")
    bbox = src.getbbox()
    if bbox:
        src = src.crop(bbox)

    canvas = Image.new("RGBA", (CANVAS, CANVAS), (0, 0, 0, 0))
    sq_mask = make_squircle_mask(BODY, SQUIRCLE_N)

    # Soft drop shadow for Dock depth.
    shadow_pad = 60
    shadow_layer = Image.new("RGBA", (BODY + shadow_pad * 2, BODY + shadow_pad * 2), (0, 0, 0, 0))
    shadow_alpha = Image.new("L", shadow_layer.size, 0)
    shadow_alpha.paste(sq_mask, (shadow_pad, shadow_pad))
    shadow_alpha = shadow_alpha.filter(ImageFilter.GaussianBlur(radius=14))
    shadow_alpha = Image.eval(shadow_alpha, lambda v: int(v * 60 / 255))
    shadow_rgba = Image.new("RGBA", shadow_layer.size, (0, 0, 0, 255))
    shadow_rgba.putalpha(shadow_alpha)
    sx = (CANVAS - shadow_layer.size[0]) // 2
    sy = (CANVAS - shadow_layer.size[1]) // 2 + 6
    canvas.alpha_composite(shadow_rgba, (sx, sy))

    # White squircle body.
    body = Image.new("RGBA", (BODY, BODY), (255, 255, 255, 255))
    body.putalpha(sq_mask)
    qx = (CANVAS - BODY) // 2
    qy = (CANVAS - BODY) // 2
    canvas.alpha_composite(body, (qx, qy))

    # Artwork inside the squircle, clipped to the mask.
    target = int(BODY * INNER_FRACTION)
    sw, sh = src.size
    scale = min(target / sw, target / sh)
    new_size = (max(1, int(sw * scale)), max(1, int(sh * scale)))
    art = src.resize(new_size, Image.LANCZOS)

    art_layer = Image.new("RGBA", (BODY, BODY), (0, 0, 0, 0))
    cx = (BODY - new_size[0]) // 2
    cy = (BODY - new_size[1]) // 2
    art_layer.alpha_composite(art, (cx, cy))

    art_alpha = np.array(art_layer.split()[3])
    mask_arr = np.array(sq_mask)
    art_layer.putalpha(Image.fromarray(np.minimum(art_alpha, mask_arr).astype(np.uint8), mode="L"))
    canvas.alpha_composite(art_layer, (qx, qy))

    canvas.save(OUTPUT, format="PNG", optimize=True)
    print(f"wrote {OUTPUT}  size={canvas.size}")
    print("next: npx tauri icon src-tauri/icons/icon.png")


if __name__ == "__main__":
    main()
