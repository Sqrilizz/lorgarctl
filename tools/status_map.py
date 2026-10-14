#!/usr/bin/env python3
"""
Generate AnyPS5-style progress treemap and SVG badges for LORGAR MSA10W.
Implements the exact squarified treemap algorithm from AnyPS5 (tools/progress.py).
"""

from html import escape
import subprocess
from pathlib import Path

PANEL_WIDTH = 495
GAP = 10
MAP_HEIGHT = 280
HEADER = 30
BORDER = "#0d1117"
TEXT = "#ffffff"
DONE_COLOR = "#2ea043"
TODO_COLOR = "#6e7681"

ROOT = Path(__file__).resolve().parent.parent
ASSETS = ROOT / "assets"
ASSETS.mkdir(exist_ok=True)


def worst_ratio(row, side):
    area = sum(row)
    if area == 0 or side == 0:
        return float("inf")
    return max(
        max(side * side * r / (area**2), (area**2) / (side * side * r))
        for r in row
        if r > 0
    )


def place(row, x, y, w, h, rects):
    side = min(w, h)
    if side <= 0:
        return (x, y, w, h)
    thickness = sum(row) / side
    offset = 0
    for area in row:
        if thickness <= 0:
            continue
        length = area / thickness
        if w >= h:
            rects.append((x, y + offset, thickness, length))
        else:
            rects.append((x + offset, y, length, thickness))
        offset += length
    return (
        (x + thickness, y, w - thickness, h)
        if w >= h
        else (x, y + thickness, w, h - thickness)
    )


def squarify(values, x, y, w, h):
    total = sum(values)
    if total <= 0 or w <= 0 or h <= 0:
        return []
    areas = [v * w * h / total for v in values]
    rects, row = [], []
    while areas:
        side = min(w, h)
        if not row or worst_ratio(row + [areas[0]], side) <= worst_ratio(
            row, side
        ):
            row.append(areas.pop(0))
            continue
        x, y, w, h = place(row, x, y, w, h, rects)
        row = []
    if row:
        place(row, x, y, w, h, rects)
    return rects


def cells(count, x, y, w, h):
    if not count or w <= 0 or h <= 0:
        return []
    rows = max(1, min(count, round((count * h / w) ** 0.5)))
    result = []
    for r in range(rows):
        in_row = count // rows + (r < count % rows)
        if in_row == 0:
            continue
        cw = w / in_row
        result += [
            (x + c * cw, y + r * h / rows, cw, h / rows) for c in range(in_row)
        ]
    return result


def rect(x, y, w, h, color, stroke=0.5):
    return (
        f'<rect x="{x:.2f}" y="{y:.2f}" width="{w:.2f}" height="{h:.2f}" '
        f'fill="{color}" stroke="{BORDER}" stroke-width="{stroke}"/>'
    )


def text(x, y, value, size):
    return (
        f'<text x="{x:.2f}" y="{y:.2f}" font-family="DejaVu Sans, Noto Sans, sans-serif" font-size="{size}" fill="{TEXT}" '
        f'stroke="{BORDER}" stroke-width="3" paint-order="stroke">{escape(value)}</text>'
    )


def treemap(title, groups, left):
    total_done = sum(g["done"] for g in groups)
    total_all = sum(g["done"] + g["todo"] for g in groups)
    percent = (
        f"{100 * total_done / total_all:.1f}" if total_all else "100.0"
    ).rstrip("0").rstrip(".") if total_done != total_all else "100.0"

    parts = [text(left + 4, 21, f"{title}: {percent}% ({total_done}/{total_all})", 16)]
    totals = [g["done"] + g["todo"] for g in groups]
    rects = squarify(totals, left, HEADER, PANEL_WIDTH, MAP_HEIGHT)

    for group, (x, y, w, h) in zip(groups, rects):
        total = group["done"] + group["todo"]
        parts.append(
            f'<g><title>{escape(group["name"])}: {group["done"]}/{total} ({100 * group["done"] / total:.0f}%)</title>'
        )
        for i, cell in enumerate(cells(total, x + 1, y + 1, w - 2, h - 2)):
            parts.append(rect(*cell, DONE_COLOR if i < group["done"] else TODO_COLOR))
        parts.append(
            f'<rect x="{x:.2f}" y="{y:.2f}" width="{w:.2f}" height="{h:.2f}" fill="none" stroke="{BORDER}" stroke-width="2"/>'
        )
        label = group.get("label", group["name"])
        max_chars = int((w - 10) / 7)
        if max_chars >= 3 and h > 22:
            display_label = label[:max_chars]
            parts.append(text(x + 5, y + 16, display_label, 12))
        parts.append("</g>")
    return parts


def badge(label, value_str, percent=100.0):
    color = "#4c1" if percent >= 90 else "#97ca00" if percent >= 60 else "#dfb317" if percent >= 30 else "#fe7d37"
    left = 10 + 7 * len(label)
    right = 10 + 7 * len(value_str)
    width = left + right
    return (
        f'<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="20" role="img" aria-label="{label}: {value_str}">\n'
        f'  <rect width="{left}" height="20" fill="#555"/>\n'
        f'  <rect x="{left}" width="{right}" height="20" fill="{color}"/>\n'
        f'  <g fill="#fff" font-family="DejaVu Sans,Verdana,sans-serif" font-size="11" text-anchor="middle">\n'
        f'    <text x="{left / 2:.1f}" y="14">{label}</text>\n'
        f'    <text x="{left + right / 2:.1f}" y="14">{value_str}</text>\n'
        f"  </g>\n"
        f"</svg>"
    )


def main():
    # Left panel: Firmware NVRAM configuration packets (32 bytes per packet = 288 total confirmed bytes)
    firmware_groups = [
        {"name": "Pkt 0x01: DPI & LED", "label": "Pkt 0x01 (DPI/RGB)", "done": 32, "todo": 0},
        {"name": "Pkt 0x02: Sensor & Sleep", "label": "Pkt 0x02 (Sensor/Sleep)", "done": 32, "todo": 0},
        {"name": "Pkt 0x03: Stage Palette", "label": "Pkt 0x03 (Palette)", "done": 32, "todo": 0},
        {"name": "Pkt 0x04: Debounce & Fire", "label": "Pkt 0x04 (Debounce)", "done": 32, "todo": 0},
        {"name": "Pkt 0x05: DPI Stages 0-3", "label": "Pkt 0x05 (DPI 0-3)", "done": 32, "todo": 0},
        {"name": "Pkt 0x06: DPI Stages 4-7", "label": "Pkt 0x06 (DPI 4-7)", "done": 32, "todo": 0},
        {"name": "Pkt 0x07: Button Matrix", "label": "Pkt 0x07 (Remap)", "done": 32, "todo": 0},
        {"name": "Pkt 0x08: Macro Bank A", "label": "Pkt 0x08 (Macro A)", "done": 32, "todo": 0},
        {"name": "Pkt 0x09: Macro Bank B", "label": "Pkt 0x09 (Macro B)", "done": 32, "todo": 0},
    ]

    # Right panel: Hardware protocol commands & Linux driver components
    stack_groups = [
        {"name": "hid-lorgar (DKMS)", "label": "hid-lorgar", "done": 28, "todo": 0},
        {"name": "lorgard daemon", "label": "lorgard", "done": 26, "todo": 0},
        {"name": "lorgarctl & TUI", "label": "lorgarctl", "done": 26, "todo": 0},
        {"name": "0x20 PollRate", "label": "0x20:Poll", "done": 12, "todo": 0},
        {"name": "0x21 ActiveStage", "label": "0x21:Stage", "done": 12, "todo": 0},
        {"name": "0x26 SetDPI", "label": "0x26:DPI", "done": 12, "todo": 0},
        {"name": "0x82 StateQuery", "label": "0x82:Query", "done": 16, "todo": 0},
        {"name": "0x22 LightMode", "label": "0x22:Mode", "done": 12, "todo": 0},
        {"name": "0x23 RGBColor", "label": "0x23:RGB", "done": 12, "todo": 0},
        {"name": "0x27 StageColor", "label": "0x27:Palette", "done": 12, "todo": 0},
        {"name": "0x05 VendorEvents", "label": "0x05:Events", "done": 14, "todo": 0},
        {"name": "UPower & Plasma", "label": "Plasma/UPower", "done": 16, "todo": 0},
    ]

    width, height = 2 * PANEL_WIDTH + GAP, HEADER + MAP_HEIGHT
    parts = [
        f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {width} {height}" width="{width}" height="{height}">',
        rect(0, 0, width, height, BORDER, 0),
    ]
    parts += treemap("Firmware NVRAM*", firmware_groups, 0)
    parts += treemap("Protocol & Linux Stack", stack_groups, PANEL_WIDTH + GAP)
    parts.append("</svg>")

    svg_content = "\n".join(parts) + "\n"
    (ASSETS / "status.svg").write_text(svg_content)
    print("Wrote assets/status.svg")

    # Render PNG with rsvg-convert (retina 2x)
    png_path = ASSETS / "status.png"
    subprocess.run(
        ["rsvg-convert", "-z", "2", str(ASSETS / "status.svg"), "-o", str(png_path)],
        check=True,
    )
    print(f"Rendered {png_path} via rsvg-convert")

    # Badges matching AnyPS5
    (ASSETS / "badge-firmware.svg").write_text(badge("firmware*", "100.0%", 100.0))
    (ASSETS / "badge-protocol.svg").write_text(badge("protocol", "100.0%", 100.0))
    (ASSETS / "badge-driver.svg").write_text(badge("linux-stack", "100.0%", 100.0))
    print("Wrote badges to assets/")


if __name__ == "__main__":
    main()
