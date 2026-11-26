"""生成 squabby 主题的 13 个点数 + 4 个花色素材。

用法（在仓库根目录执行，需要 fontTools / Pillow 所在的 Python）：
    python crates/deckart/scripts/squabby-ranks.py
    python crates/deckart/scripts/squabby-ranks.py --out path/to/theme   # 可选

约定：
- 点数画在 100×140 的框里，所有字形统一按 **大写高度（cap height）** 对齐：
  A / 2–9 / 10 / J / Q / K 主体同高，J/Q 的降部自然伸到基线以下；
- 宽字形（A/Q/K）只压缩 x 方向，不缩小高度；"10" 先收紧字距再压到 97 宽，
  并补一圈同色描边，让横向压缩后的竖笔和个位数一样粗；
- 字号 / 描边粗细想调，改下面的 CAP / MAX_INK_W / TEN_STROKE；
- 花色 ♠♥♦♣ 是手绘轮廓，墨色占位统一写 #1B1B1B，渲染期按 palette 替换；
- 所有浮点保留两位小数并去掉多余的 0，避免 fontTools 输出的长小数把 SVG 撑大。

字体依赖：Windows 自带 Arial Bold（arialbd.ttf）。换字体就改 FONT_PATH，
换字重（比如想整体更粗）可以试 ariblk.ttf（Arial Black），但字形会变。
"""

import argparse
import re
from pathlib import Path

from fontTools.ttLib import TTFont
from fontTools.pens.boundsPen import BoundsPen
from fontTools.pens.svgPathPen import SVGPathPen
from fontTools.pens.transformPen import TransformPen

REPO = Path(__file__).resolve().parents[3]
DEFAULT_OUT = REPO / "crates" / "deckart" / "themes" / "squabby"
FONT_PATH = Path(r"C:\Windows\Fonts\arialbd.ttf")

INK = "#1B1B1B"      # 渲染期替换成 palette.red / palette.black

# 点数框参数：目标大写高度 / 最大墨迹宽度 / 顶部留白
CAP = 112.0
MAX_INK_W = 96.0
PAD_TOP = 12.0
# "10" 专用：字距（字体单位，负数收紧）+ 允许宽度 + 同色描边宽度
TEN_TRACKING = -320.0
TEN_MAX_W = 97.0
TEN_STROKE = 3.0


def _glyphs(font):
    return font.getGlyphSet(), font.getBestCmap(), font["hmtx"]


def text_bounds(font, text):
    """字串的包围盒（字形坐标，y 轴向上）。"""
    gs, cmap, hmtx = _glyphs(font)
    pen = BoundsPen(gs)
    x = 0.0
    for ch in text:
        name = cmap[ord(ch)]
        gs[name].draw(TransformPen(pen, (1, 0, 0, 1, x, 0)))
        x += hmtx[name][0]
    box = pen.bounds
    return box, x


def text_path(font, text, w, h, cap=CAP, max_w=MAX_INK_W, pad_top=PAD_TOP, tracking=0.0):
    """把 text 放进 w×h 的框，所有字形统一到大写高度 cap。

    - 纵向：按字体的 sCapHeight 换算，基线固定在 pad_top + cap；
      J/Q 的降部自然伸到基线以下，不再把整个字压小；
    - 横向：墨迹宽超过 max_w 时只压缩 x（A/Q/K 与 "10"），高度不变；
    - tracking：多字形之间的额外字距（字体单位，负数收紧），"10" 用负值收掉
      Arial 里 1 的大侧边距。
    """
    gs, cmap, hmtx = _glyphs(font)
    box, _advance = text_bounds(font, text)
    x0, y0, x1, y1 = box
    ink_w, ink_h = x1 - x0, y1 - y0
    if ink_w <= 0 or ink_h <= 0:
        raise ValueError(f"空字形：{text!r}")

    # 先按 tracking 修正总宽（最后一个字后不加字距）
    ink_w += tracking * max(0, len(text) - 1)

    cap_units = font["OS/2"].sCapHeight
    sy = cap / cap_units
    sx = sy
    if ink_w * sx > max_w:
        sx = max_w / ink_w

    tx = (w - ink_w * sx) / 2 - x0 * sx
    ty = pad_top + cap   # 翻转后基线的屏幕 y

    pen = SVGPathPen(gs)
    x = 0.0
    for i, ch in enumerate(text):
        if i:
            x += tracking
        name = cmap[ord(ch)]
        t = TransformPen(pen, (sx, 0, 0, -sy, x * sx + tx, ty))
        gs[name].draw(t)
        x += hmtx[name][0]
    return pen.getCommands()


_NUM_RE = re.compile(r"-?\d+\.\d+")


def compact_numbers(text, ndigits=2):
    """把 SVG 里的浮点压到 ndigits 位并去掉多余的 0（0.50 → .5，-0 → 0）。"""

    def fmt(m):
        v = round(float(m.group()), ndigits)
        if v == 0:
            return "0"
        s = f"{v:.{ndigits}f}".rstrip("0").rstrip(".")
        if s.startswith("0."):
            s = s[1:]                  # 0.5 → .5
        elif s.startswith("-0."):
            s = "-" + s[2:]            # -0.5 → -.5
        return s

    return _NUM_RE.sub(fmt, text)


def svg(w, h, inner):
    return (
        f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {w} {h}">\n'
        f"{inner}\n"
        "</svg>\n"
    )


def write(out_dir, rel, content):
    content = compact_numbers(content, 2)
    p = out_dir / rel
    p.parent.mkdir(parents=True, exist_ok=True)
    p.write_text(content, encoding="utf-8", newline="\n")
    print(f"{rel:22s} {len(content):5d} B")


RANK_TEXT = {
    1: "A", 2: "2", 3: "3", 4: "4", 5: "5", 6: "6", 7: "7",
    8: "8", 9: "9", 10: "10", 11: "J", 12: "Q", 13: "K",
}

# 花色：黑桃 / 红桃 / 菱形 / 梅花全部手绘，坐标在 100×100 内
SPADE = (
    "M50 5 C54 15 66 27 78 37 C88 45 95 55 95 65 C95 77 86 85 75 85 "
    "C67 85 60 82 56 77 L63 96 L37 96 L44 77 "
    "C40 82 33 85 25 85 C14 85 5 77 5 65 C5 55 12 45 22 37 "
    "C34 27 46 15 50 5 Z"
)
DIAMOND = "M50 2 L84 50 L50 98 L16 50 Z"
CLUB = (
    '<circle cx="50" cy="27" r="22"/>'
    '<circle cx="50" cy="58" r="21"/>'
    '<circle cx="28" cy="55" r="22"/>'
    '<circle cx="72" cy="55" r="22"/>'
    '<path d="M45 58 L55 58 L63 96 L37 96 Z"/>'
)
HEART = (
    "M50 94 C44 86 4 60 4 34 C4 16 18 4 34 4 C42 4 47 9 50 15 "
    "C53 9 58 4 66 4 C82 4 96 16 96 34 C96 60 56 86 50 94 Z"
)


def make_ranks(font, out_dir):
    for rank, text in RANK_TEXT.items():
        is_ten = text == "10"
        d = text_path(
            font, text, 100, 140,
            tracking=TEN_TRACKING if is_ten else 0.0,
            max_w=TEN_MAX_W if is_ten else MAX_INK_W,
        )
        if is_ten:
            body = (
                f'<path d="{d}" fill="{INK}" stroke="{INK}" '
                f'stroke-width="{TEN_STROKE:g}" stroke-linejoin="round"/>'
            )
        else:
            body = f'<path d="{d}" fill="{INK}"/>'
        write(out_dir, f"ranks/{rank}.svg", svg(100, 140, body))


def make_suits(out_dir):
    for name, body in (
        ("spade", f'<path d="{SPADE}" fill="{INK}"/>'),
        ("heart", f'<path d="{HEART}" fill="{INK}"/>'),
        ("diamond", f'<path d="{DIAMOND}" fill="{INK}"/>'),
        ("club", f'<g fill="{INK}">{CLUB}</g>'),
    ):
        write(out_dir, f"suits/{name}.svg", svg(100, 100, body))


def main():
    ap = argparse.ArgumentParser(description="生成 squabby 的点数与花色 SVG")
    ap.add_argument("--out", type=Path, default=DEFAULT_OUT, help=f"主题目录，默认 {DEFAULT_OUT}")
    args = ap.parse_args()
    if not FONT_PATH.exists():
        raise SystemExit(f"找不到字体 {FONT_PATH}，改脚本里的 FONT_PATH")
    font = TTFont(str(FONT_PATH))
    args.out.mkdir(parents=True, exist_ok=True)
    make_ranks(font, args.out)
    make_suits(args.out)
    print("完成：13 个点数 + 4 个花色")


if __name__ == "__main__":
    main()
