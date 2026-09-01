#!/usr/bin/env python3
from __future__ import annotations

from pathlib import Path

import numpy as np
from PIL import Image


WEB_ROOT = Path(__file__).resolve().parent.parent
REPO_ROOT = WEB_ROOT.parent
ASSET_ROOTS = (
    WEB_ROOT / "public" / "assets",
    REPO_ROOT / "docs-site" / "docs" / "public" / "assets",
)
ICON_SIZE = 1024
CONTENT_BOX = (240, 200, 784, 824)
MAX_CENTER_OFFSET = 2.0
MINIMUM_COLOR_DISTANCE = 40


def bbox_center(
    mask: np.ndarray,
    label: str,
    origin: tuple[int, int] = (0, 0),
) -> tuple[float, float]:
    ys, xs = np.nonzero(mask)
    if not len(xs):
        raise RuntimeError(f"{label} has no visible foreground")
    bbox = (
        int(xs.min()) + origin[0],
        int(ys.min()) + origin[1],
        int(xs.max()) + 1 + origin[0],
        int(ys.max()) + 1 + origin[1],
    )
    center = ((bbox[0] + bbox[2]) / 2, (bbox[1] + bbox[3]) / 2)
    print(f"{label}.bbox={bbox[0]},{bbox[1]},{bbox[2]},{bbox[3]}")
    print(f"{label}.center={center[0]:.1f},{center[1]:.1f}")
    return center


def verify_colored_icon(path: Path, plate_color: tuple[int, int, int]) -> None:
    image = np.asarray(Image.open(path).convert("RGBA"))
    if image.shape[:2] != (ICON_SIZE, ICON_SIZE):
        raise RuntimeError(f"{path} must be {ICON_SIZE}x{ICON_SIZE}")

    left, top, right, bottom = CONTENT_BOX
    pixels = image[top:bottom, left:right, :3].astype(np.int16)
    alpha = image[top:bottom, left:right, 3] >= 128
    distance = np.abs(pixels - np.asarray(plate_color, dtype=np.int16)).sum(axis=2)
    center = bbox_center(
        alpha & (distance > MINIMUM_COLOR_DISTANCE),
        str(path.relative_to(REPO_ROOT)),
        origin=(left, top),
    )
    expected_center = ICON_SIZE / 2
    if any(abs(value - expected_center) > MAX_CENTER_OFFSET for value in center):
        raise SystemExit(f"{path} visible mark is not centered: {center}")


def verify_transparent_icon(path: Path) -> None:
    image = np.asarray(Image.open(path).convert("RGBA"))
    if image.shape[:2] != (ICON_SIZE, ICON_SIZE):
        raise RuntimeError(f"{path} must be {ICON_SIZE}x{ICON_SIZE}")
    center = bbox_center(
        image[:, :, 3] >= 128,
        str(path.relative_to(REPO_ROOT)),
    )
    expected_center = ICON_SIZE / 2
    if any(abs(value - expected_center) > MAX_CENTER_OFFSET for value in center):
        raise SystemExit(f"{path} visible mark is not centered: {center}")


def main() -> None:
    for asset_root in ASSET_ROOTS:
        verify_colored_icon(
            asset_root / "relay-mesh-icon-light.png",
            (235, 232, 247),
        )
        verify_colored_icon(
            asset_root / "relay-mesh-icon-dark.png",
            (10, 15, 40),
        )
        verify_transparent_icon(asset_root / "relay-mesh-icon-mono-light.png")
        verify_transparent_icon(asset_root / "relay-mesh-icon-mono-dark.png")


if __name__ == "__main__":
    main()
