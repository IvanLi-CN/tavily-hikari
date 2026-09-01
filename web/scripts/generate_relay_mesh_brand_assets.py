#!/usr/bin/env python3
from __future__ import annotations

from collections import deque
import shutil
import subprocess
from pathlib import Path
import xml.etree.ElementTree as ET

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
SVG_NAMESPACE = "http://www.w3.org/2000/svg"

ET.register_namespace("", SVG_NAMESPACE)


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
    if path.exists():
        existing = load_rgba(path)
        candidate = image.convert("RGBA")
        if existing.size == candidate.size and existing.tobytes() == candidate.tobytes():
            return
    image.save(path)


def save_resized(image: Image.Image, size: int, path: Path) -> None:
    save_png(image.resize((size, size), Image.Resampling.LANCZOS), path)


def render_vector_png(source: Path, path: Path) -> Image.Image:
    renderer = shutil.which("rsvg-convert")
    if renderer is None:
        raise RuntimeError("rsvg-convert is required to export lockup PNG assets")
    path.parent.mkdir(parents=True, exist_ok=True)
    subprocess.run([renderer, str(source), "--output", str(path)], check=True)
    return load_rgba(path)


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


def write_vector_svg(
    source: Path,
    path: Path,
    *,
    color: tuple[int, int, int] | None = None,
    lighten_ratio: float | None = None,
    dark_lockup: bool = False,
) -> None:
    root = ET.parse(source).getroot()

    for element in root.iter():
        for attribute in ("fill", "stroke", "stop-color", "flood-color"):
            value = element.get(attribute)
            if not value or not value.startswith("#") or len(value) != 7:
                continue
            source_color = tuple(int(value[index:index + 2], 16) for index in (1, 3, 5))
            if color is not None:
                output_color = color
            elif dark_lockup and value.upper() == "#6D28D9":
                output_color = (167, 139, 250)
            elif dark_lockup and value.upper() == "#0369A1":
                output_color = (56, 189, 248)
            elif dark_lockup:
                if chroma(source_color) <= 44 and luminance(source_color) < 205:
                    output_color = (
                        LIGHT_CLAY
                        if luminance(source_color) < 150
                        else blend(LIGHT_CLAY, CLAY_BG, 0.35)
                    )
                else:
                    output_color = blend(source_color, WHITE, 0.24)
            elif lighten_ratio is not None:
                output_color = blend(source_color, WHITE, lighten_ratio)
            else:
                output_color = source_color
            element.set(attribute, "#" + "".join(f"{channel:02X}" for channel in output_color))

    path.parent.mkdir(parents=True, exist_ok=True)
    ET.ElementTree(root).write(path, encoding="utf-8", xml_declaration=True)


def write_mark_svg(
    path: Path,
    *,
    color: tuple[int, int, int] | None = None,
    lighten_ratio: float | None = None,
) -> None:
    write_vector_svg(
        REFERENCE_DIR / "approved-mark-vector-light.svg",
        path,
        color=color,
        lighten_ratio=lighten_ratio,
    )


def write_lockup_svgs(assets_dir: Path, *, compact: bool = False) -> None:
    source_name = (
        "approved-lockup-compact-vector-light.svg"
        if compact
        else "approved-lockup-vector-light.svg"
    )
    output_stem = "relay-mesh-mobile-logo" if compact else "relay-mesh-lockup"
    source = REFERENCE_DIR / source_name
    write_vector_svg(source, assets_dir / f"{output_stem}.svg")
    write_vector_svg(source, assets_dir / f"{output_stem}-light.svg")
    write_vector_svg(source, assets_dir / f"{output_stem}-dark.svg", dark_lockup=True)
    write_vector_svg(source, assets_dir / f"{output_stem}-mono-dark.svg", color=CLAY_INK)
    write_vector_svg(source, assets_dir / f"{output_stem}-mono-light.svg", color=LIGHT_CLAY)


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


def resize_mark_for_square(mark: Image.Image, target_height: int) -> tuple[Image.Image, tuple[int, int]]:
    target_width = int(round(mark.width * target_height / mark.height))
    placed_mark = mark.resize((target_width, target_height), Image.Resampling.LANCZOS)
    visible_bbox = placed_mark.getchannel("A").getbbox()
    if visible_bbox is None:
        raise RuntimeError("brand mark contains no visible artwork")
    left, top, right, bottom = visible_bbox
    offset = (
        int(round((ICON_MASTER_SIZE - left - right) / 2)),
        int(round((ICON_MASTER_SIZE - top - bottom) / 2)),
    )
    return placed_mark, offset


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

    placed_mark, offset = resize_mark_for_square(mark, 650)
    canvas.alpha_composite(placed_mark, offset)
    return canvas


def make_mono_square_icon(mark: Image.Image) -> Image.Image:
    canvas = Image.new("RGBA", (ICON_MASTER_SIZE, ICON_MASTER_SIZE), (0, 0, 0, 0))
    placed_mark, offset = resize_mark_for_square(mark, 760)
    canvas.alpha_composite(placed_mark, offset)
    return canvas


def export_static_assets(public_dir: Path) -> None:
    assets_dir = public_dir / ASSETS_DIR_NAME
    mark_seed = load_rgba(REFERENCE_DIR / "approved-mark-raster.png")

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
    write_mark_svg(assets_dir / "relay-mesh-mark-light.svg")
    write_mark_svg(assets_dir / "relay-mesh-mark-dark.svg", lighten_ratio=0.14)
    write_mark_svg(assets_dir / "relay-mesh-mark-mono-dark.svg", color=CLAY_INK)
    write_mark_svg(assets_dir / "relay-mesh-mark-mono-light.svg", color=LIGHT_CLAY)
    write_lockup_svgs(assets_dir)
    write_lockup_svgs(assets_dir, compact=True)
    render_vector_png(
        assets_dir / "relay-mesh-lockup-light.svg",
        assets_dir / "relay-mesh-lockup-light.png",
    )
    shutil.copyfile(assets_dir / "relay-mesh-lockup-light.png", assets_dir / "relay-mesh-lockup.png")
    render_vector_png(assets_dir / "relay-mesh-lockup-dark.svg", assets_dir / "relay-mesh-lockup-dark.png")
    render_vector_png(assets_dir / "relay-mesh-lockup-mono-dark.svg", assets_dir / "relay-mesh-lockup-mono-dark.png")
    render_vector_png(assets_dir / "relay-mesh-lockup-mono-light.svg", assets_dir / "relay-mesh-lockup-mono-light.png")
    render_vector_png(assets_dir / "relay-mesh-mobile-logo-light.svg", assets_dir / "relay-mesh-mobile-logo-light.png")
    render_vector_png(assets_dir / "relay-mesh-mobile-logo-dark.svg", assets_dir / "relay-mesh-mobile-logo-dark.png")
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
        "relay-mesh-lockup.svg",
        "relay-mesh-lockup-light.svg",
        "relay-mesh-lockup-dark.svg",
        "relay-mesh-lockup-mono-dark.svg",
        "relay-mesh-lockup-mono-light.svg",
        "relay-mesh-mobile-logo.svg",
        "relay-mesh-mobile-logo-light.svg",
        "relay-mesh-mobile-logo-dark.svg",
        "relay-mesh-mobile-logo-mono-dark.svg",
        "relay-mesh-mobile-logo-mono-light.svg",
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
