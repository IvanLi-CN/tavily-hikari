#!/usr/bin/env python3
from __future__ import annotations

from collections import deque
import shutil
from pathlib import Path

from PIL import Image, ImageDraw


WEB_ROOT = Path(__file__).resolve().parent.parent
REPO_ROOT = WEB_ROOT.parent
REFERENCE_DIR = WEB_ROOT / "brand" / "relay-mesh" / "reference"
WEB_PUBLIC_DIR = WEB_ROOT / "public"
DOCS_PUBLIC_DIR = REPO_ROOT / "docs-site" / "docs" / "public"
ASSETS_DIR_NAME = "assets"

CLAY_BG = (244, 241, 250)
CLAY_BG_ALT = (235, 232, 247)
CLAY_INK = (51, 47, 58)
CLAY_INK_SOFT = (99, 95, 105)
LIGHT_CLAY = (242, 238, 251)
DARK_PLATE = (15, 20, 49)
DEEP_PLATE = (10, 15, 40)
WHITE = (255, 255, 255)
ICON_MASTER_SIZE = 1024
ICON_MARGIN = 92
ICON_RADIUS = 208
ICON_BORDER_WIDTH = 10


def load_rgba(path: Path) -> Image.Image:
    return Image.open(path).convert("RGBA")


def sample_border_color(image: Image.Image) -> tuple[int, int, int]:
    width, height = image.size
    pixels = image.load()
    samples: list[tuple[int, int, int]] = []
    for x in range(width):
        samples.append(pixels[x, 0][:3])
        samples.append(pixels[x, height - 1][:3])
    for y in range(height):
        samples.append(pixels[0, y][:3])
        samples.append(pixels[width - 1, y][:3])
    count = len(samples)
    return tuple(sum(sample[i] for sample in samples) // count for i in range(3))


def rgb_diff(left: tuple[int, int, int], right: tuple[int, int, int]) -> int:
    return sum(abs(left[i] - right[i]) for i in range(3))


def chroma(rgb: tuple[int, int, int]) -> int:
    return max(rgb) - min(rgb)


def luminance(rgb: tuple[int, int, int]) -> float:
    red, green, blue = rgb
    return 0.2126 * red + 0.7152 * green + 0.0722 * blue


def blend(left: tuple[int, int, int], right: tuple[int, int, int], ratio: float) -> tuple[int, int, int]:
    clamped = max(0.0, min(1.0, ratio))
    return tuple(
        int(round(left[i] * (1.0 - clamped) + right[i] * clamped))
        for i in range(3)
    )


def remove_background(
    image: Image.Image,
    *,
    fill_threshold: int = 58,
    soft_threshold: int = 82,
    chroma_threshold: int = 38,
) -> Image.Image:
    rgba = image.copy().convert("RGBA")
    width, height = rgba.size
    pixels = rgba.load()
    background = sample_border_color(rgba)

    visited = [[False] * height for _ in range(width)]
    queue: deque[tuple[int, int]] = deque()

    for x in range(width):
        queue.append((x, 0))
        queue.append((x, height - 1))
    for y in range(height):
        queue.append((0, y))
        queue.append((width - 1, y))

    while queue:
        x, y = queue.popleft()
        if x < 0 or y < 0 or x >= width or y >= height or visited[x][y]:
            continue
        visited[x][y] = True
        red, green, blue, alpha = pixels[x, y]
        if alpha == 0:
            continue
        rgb = (red, green, blue)
        if rgb_diff(rgb, background) <= fill_threshold and chroma(rgb) <= chroma_threshold:
            pixels[x, y] = (red, green, blue, 0)
            queue.extend(((x + 1, y), (x - 1, y), (x, y + 1), (x, y - 1)))

    for y in range(height):
        for x in range(width):
            red, green, blue, alpha = pixels[x, y]
            if alpha == 0:
                continue
            rgb = (red, green, blue)
            diff = rgb_diff(rgb, background)
            if diff < soft_threshold and chroma(rgb) <= chroma_threshold + 10:
                softened = max(0, min(255, int((diff - fill_threshold + 8) * 255 / 32)))
                pixels[x, y] = (red, green, blue, softened)

    return rgba


def recolor_lockup_for_dark(image: Image.Image) -> Image.Image:
    rgba = image.copy().convert("RGBA")
    pixels = rgba.load()
    width, height = rgba.size
    for y in range(height):
        for x in range(width):
            red, green, blue, alpha = pixels[x, y]
            if alpha == 0:
                continue
            rgb = (red, green, blue)
            diff = chroma(rgb)
            lightness = luminance(rgb)
            if diff <= 44 and lightness < 205:
                target = LIGHT_CLAY if lightness < 150 else blend(LIGHT_CLAY, CLAY_BG, 0.35)
                pixels[x, y] = (*target, alpha)
            elif diff > 44:
                boosted = blend(rgb, WHITE, 0.12)
                pixels[x, y] = (*boosted, alpha)
    return rgba


def recolor_mark_for_dark(image: Image.Image) -> Image.Image:
    rgba = image.copy().convert("RGBA")
    pixels = rgba.load()
    width, height = rgba.size
    for y in range(height):
        for x in range(width):
            red, green, blue, alpha = pixels[x, y]
            if alpha == 0:
                continue
            boosted = blend((red, green, blue), WHITE, 0.14)
            pixels[x, y] = (*boosted, alpha)
    return rgba


def recolor_monochrome(image: Image.Image, color: tuple[int, int, int]) -> Image.Image:
    rgba = image.copy().convert("RGBA")
    pixels = rgba.load()
    width, height = rgba.size
    for y in range(height):
        for x in range(width):
            red, green, blue, alpha = pixels[x, y]
            if alpha == 0:
                continue
            pixels[x, y] = (*color, alpha)
    return rgba


def save_png(image: Image.Image, path: Path) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    image.save(path)


def save_resized(image: Image.Image, size: int, path: Path) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    image.resize((size, size), Image.Resampling.LANCZOS).save(path)


def write_svg_wrapper(href: str, width: int, height: int, path: Path) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(
        "\n".join(
            [
                f'<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="0 0 {width} {height}">',
                f'  <image href="{href}" width="{width}" height="{height}" preserveAspectRatio="xMidYMid meet" />',
                "</svg>",
                "",
            ]
        ),
        encoding="utf-8",
    )


def write_theme_svg(
    light_href: str,
    dark_href: str,
    width: int,
    height: int,
    path: Path,
) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(
        "\n".join(
            [
                f'<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="0 0 {width} {height}">',
                "  <style>",
                "    .theme-dark { display: none; }",
                "    @media (prefers-color-scheme: dark) {",
                "      .theme-light { display: none; }",
                "      .theme-dark { display: inline; }",
                "    }",
                "  </style>",
                f'  <image class="theme-light" href="{light_href}" width="{width}" height="{height}" preserveAspectRatio="xMidYMid meet" />',
                f'  <image class="theme-dark" href="{dark_href}" width="{width}" height="{height}" preserveAspectRatio="xMidYMid meet" />',
                "</svg>",
                "",
            ]
        ),
        encoding="utf-8",
    )


def asset_path(file_name: str) -> str:
    return f"{ASSETS_DIR_NAME}/{file_name}"


def local_asset_path(file_name: str) -> str:
    return file_name


def remove_if_exists(path: Path) -> None:
    if path.exists():
        path.unlink()


def make_launcher_icon(
    mark: Image.Image,
    *,
    plate_color: tuple[int, int, int],
    border_color: tuple[int, int, int],
) -> Image.Image:
    canvas = Image.new("RGBA", (ICON_MASTER_SIZE, ICON_MASTER_SIZE), (0, 0, 0, 0))
    draw = ImageDraw.Draw(canvas)
    inset = ICON_MARGIN
    box = (
        inset,
        inset,
        ICON_MASTER_SIZE - inset,
        ICON_MASTER_SIZE - inset,
    )
    draw.rounded_rectangle(box, radius=ICON_RADIUS, fill=(*plate_color, 255))
    draw.rounded_rectangle(box, radius=ICON_RADIUS, outline=(*border_color, 255), width=ICON_BORDER_WIDTH)

    mark_target_height = 650
    mark_target_width = int(round(mark.width * mark_target_height / mark.height))
    placed_mark = mark.resize((mark_target_width, mark_target_height), Image.Resampling.LANCZOS)
    offset = (
        (ICON_MASTER_SIZE - mark_target_width) // 2,
        (ICON_MASTER_SIZE - mark_target_height) // 2,
    )
    canvas.alpha_composite(placed_mark, offset)
    return canvas


def make_mono_square_icon(mark: Image.Image) -> Image.Image:
    canvas = Image.new("RGBA", (ICON_MASTER_SIZE, ICON_MASTER_SIZE), (0, 0, 0, 0))
    mark_target_height = 760
    mark_target_width = int(round(mark.width * mark_target_height / mark.height))
    placed_mark = mark.resize((mark_target_width, mark_target_height), Image.Resampling.LANCZOS)
    offset = (
        (ICON_MASTER_SIZE - mark_target_width) // 2,
        (ICON_MASTER_SIZE - mark_target_height) // 2,
    )
    canvas.alpha_composite(placed_mark, offset)
    return canvas


def export_static_assets(public_dir: Path) -> None:
    assets_dir = public_dir / ASSETS_DIR_NAME
    lockup_seed = load_rgba(REFERENCE_DIR / "approved-lockup-raster.png")
    mark_seed = load_rgba(REFERENCE_DIR / "approved-mark-raster.png")

    lockup_light = remove_background(lockup_seed, fill_threshold=60, soft_threshold=84)
    lockup_dark = recolor_lockup_for_dark(lockup_light)
    lockup_mono_dark = recolor_monochrome(lockup_light, CLAY_INK)
    lockup_mono_light = recolor_monochrome(lockup_light, LIGHT_CLAY)
    mark_light = remove_background(mark_seed, fill_threshold=62, soft_threshold=88)
    mark_dark = recolor_mark_for_dark(mark_light)
    mark_mono_dark = recolor_monochrome(mark_light, CLAY_INK)
    mark_mono_light = recolor_monochrome(mark_light, LIGHT_CLAY)

    launcher_light = make_launcher_icon(
        mark_light,
        plate_color=CLAY_BG_ALT,
        border_color=blend(CLAY_BG_ALT, CLAY_INK_SOFT, 0.24),
    )
    launcher_dark = make_launcher_icon(
        mark_dark,
        plate_color=DEEP_PLATE,
        border_color=blend(DEEP_PLATE, WHITE, 0.18),
    )
    launcher_mono_dark = make_mono_square_icon(mark_mono_dark)
    launcher_mono_light = make_mono_square_icon(mark_mono_light)

    save_png(lockup_light, assets_dir / "relay-mesh-lockup.png")
    save_png(lockup_light, assets_dir / "relay-mesh-lockup-light.png")
    save_png(lockup_dark, assets_dir / "relay-mesh-lockup-dark.png")
    save_png(lockup_mono_dark, assets_dir / "relay-mesh-lockup-mono-dark.png")
    save_png(lockup_mono_light, assets_dir / "relay-mesh-lockup-mono-light.png")
    save_png(lockup_light, assets_dir / "relay-mesh-mobile-logo-light.png")
    save_png(lockup_dark, assets_dir / "relay-mesh-mobile-logo-dark.png")

    save_png(mark_light, assets_dir / "relay-mesh-mark.png")
    save_png(mark_light, assets_dir / "relay-mesh-mark-light.png")
    save_png(mark_dark, assets_dir / "relay-mesh-mark-dark.png")
    save_png(mark_mono_dark, assets_dir / "relay-mesh-mark-mono-dark.png")
    save_png(mark_mono_light, assets_dir / "relay-mesh-mark-mono-light.png")

    save_png(launcher_light, assets_dir / "relay-mesh-icon.png")
    save_png(launcher_light, assets_dir / "relay-mesh-icon-light.png")
    save_png(launcher_dark, assets_dir / "relay-mesh-icon-dark.png")
    save_png(launcher_mono_dark, assets_dir / "relay-mesh-icon-mono-dark.png")
    save_png(launcher_mono_light, assets_dir / "relay-mesh-icon-mono-light.png")

    save_resized(mark_light, 16, assets_dir / "favicon-16x16.png")
    save_resized(mark_light, 32, assets_dir / "favicon-32x32.png")
    save_resized(mark_light, 48, assets_dir / "favicon-48x48.png")
    save_resized(launcher_light, 180, assets_dir / "apple-touch-icon.png")

    write_theme_svg(
        asset_path("relay-mesh-mark-light.png"),
        asset_path("relay-mesh-mark-dark.png"),
        mark_light.width,
        mark_light.height,
        public_dir / "favicon.svg",
    )
    write_svg_wrapper(
        local_asset_path("relay-mesh-mark-light.png"),
        mark_light.width,
        mark_light.height,
        assets_dir / "relay-mesh-mark-light.svg",
    )
    write_svg_wrapper(
        local_asset_path("relay-mesh-mark-dark.png"),
        mark_dark.width,
        mark_dark.height,
        assets_dir / "relay-mesh-mark-dark.svg",
    )
    write_svg_wrapper(
        local_asset_path("relay-mesh-mark-mono-dark.png"),
        mark_mono_dark.width,
        mark_mono_dark.height,
        assets_dir / "relay-mesh-mark-mono-dark.svg",
    )
    write_svg_wrapper(
        local_asset_path("relay-mesh-mark-mono-light.png"),
        mark_mono_light.width,
        mark_mono_light.height,
        assets_dir / "relay-mesh-mark-mono-light.svg",
    )
    write_svg_wrapper(
        local_asset_path("relay-mesh-icon-mono-dark.png"),
        launcher_mono_dark.width,
        launcher_mono_dark.height,
        assets_dir / "relay-mesh-icon-mono-dark.svg",
    )
    write_svg_wrapper(
        local_asset_path("relay-mesh-icon-mono-light.png"),
        launcher_mono_light.width,
        launcher_mono_light.height,
        assets_dir / "relay-mesh-icon-mono-light.svg",
    )

    legacy_root_files = (
        "relay-mesh-lockup.png",
        "relay-mesh-lockup-light.png",
        "relay-mesh-lockup-dark.png",
        "relay-mesh-lockup-mono-dark.png",
        "relay-mesh-lockup-mono-light.png",
        "relay-mesh-mark.png",
        "relay-mesh-mark-light.png",
        "relay-mesh-mark-dark.png",
        "relay-mesh-mark-mono-dark.png",
        "relay-mesh-mark-mono-light.png",
        "relay-mesh-icon.png",
        "relay-mesh-icon-light.png",
        "relay-mesh-icon-dark.png",
        "relay-mesh-icon-mono-dark.png",
        "relay-mesh-icon-mono-light.png",
        "relay-mesh-mobile-logo-light.png",
        "relay-mesh-mobile-logo-dark.png",
        "relay-mesh-mark-light.svg",
        "relay-mesh-mark-dark.svg",
        "relay-mesh-mark-mono-dark.svg",
        "relay-mesh-mark-mono-light.svg",
        "relay-mesh-icon-mono-dark.svg",
        "relay-mesh-icon-mono-light.svg",
        "favicon-16x16.png",
        "favicon-32x32.png",
        "favicon-48x48.png",
        "apple-touch-icon.png",
    )
    for file_name in legacy_root_files:
        remove_if_exists(public_dir / file_name)


def main() -> None:
    export_static_assets(WEB_PUBLIC_DIR)
    export_static_assets(DOCS_PUBLIC_DIR)
    print(f"[brand] generated relay mesh assets into {WEB_PUBLIC_DIR} and {DOCS_PUBLIC_DIR}")


if __name__ == "__main__":
    main()
