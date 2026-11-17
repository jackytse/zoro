#!/usr/bin/env python3
"""生成托盘图标 src-tauri/icons/tray.png。

macOS 的菜单栏图标推荐使用「模板图标」（template image）：系统只取 alpha 通道，
再按明暗主题自动着色，所以这里画成纯黑 + 不同透明度的形状即可。

不依赖 Pillow：直接用 zlib 手写 PNG，避免为了一张图标引入图片库。
"""

import struct
import zlib
from pathlib import Path

SIZE = 44  # 44x44 物理像素，在 2x 屏上等价于 22pt，符合 macOS 菜单栏图标尺寸
SS = 4  # 每像素 4x4 超采样，用于抗锯齿
OUTER_R = 19.5  # 外六边形半径（像素）
RING = 3.4  # 环的厚度


def hex_distance(x: float, y: float) -> float:
    """点到正六边形中心的最大投影：用来判断是否落在六边形内。"""
    # 三条法线（0°、60°、120°）的最大投影即为六边形范数
    import math

    projections = []
    for deg in (0, 60, 120):
        rad = math.radians(deg)
        nx, ny = math.cos(rad), math.sin(rad)
        projections.append(abs(x * nx + y * ny))
    return max(projections)


def coverage(px: int, py: int) -> int:
    """返回该像素被环覆盖的比例（0-255），用超采样求平均。"""
    hits = 0
    for sy in range(SS):
        for sx in range(SS):
            # 子采样点相对中心的坐标
            x = px + (sx + 0.5) / SS - SIZE / 2
            y = py + (sy + 0.5) / SS - SIZE / 2
            d = hex_distance(x, y)
            if d <= OUTER_R and d >= OUTER_R - RING:
                hits += 1
    return round(255 * hits / (SS * SS))


def build_png() -> bytes:
    raw = bytearray()
    for py in range(SIZE):
        raw.append(0)  # 每行的 filter type：0 = None
        for px in range(SIZE):
            alpha = coverage(px, py)
            # RGB 固定为黑色，模板图标只认 alpha
            raw.extend((0, 0, 0, alpha))

    def chunk(tag: bytes, data: bytes) -> bytes:
        return (
            struct.pack(">I", len(data))
            + tag
            + data
            + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF)
        )

    header = struct.pack(">IIBBBBB", SIZE, SIZE, 8, 6, 0, 0, 0)  # 8bit RGBA
    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", header)
        + chunk(b"IDAT", zlib.compress(bytes(raw), 9))
        + chunk(b"IEND", b"")
    )


if __name__ == "__main__":
    target = Path(__file__).resolve().parent.parent / "src-tauri" / "icons" / "tray.png"
    target.write_bytes(build_png())
    print(f"wrote {target} ({target.stat().st_size} bytes)")
