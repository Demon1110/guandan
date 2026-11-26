"""生成 squabby 的底面、牌背、大小王、级牌/逢人配徽章（6 个 SVG）。

用法（在仓库根目录执行）：
    python crates/deckart/scripts/squabby-art.py
    python crates/deckart/scripts/squabby-art.py --out path/to/theme   # 可选

说明：
- 这 6 张是“插画层”，颜色写死（红/金/米白/深红），不参与牌面墨色替换；
- 大小王左侧的竖排 JOKER 文字用 Arial Bold 抽路径后转成竖排（从上往下读，不镜像），字号由 joker_text(52, …) 控制；
- 所有浮点保留两位小数、去掉多余的 0，避免 SVG 里出现一长串小数。

依赖字体：C:\\Windows\\Fonts\\arialbd.ttf（Arial Bold）。
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

RED = "#dd0808"
DEEP = "#A80707"
GOLD = "#F4B324"
CREAM = "#FFF6E3"
PAPER = "#FFFFFF"
INK = "#232323"
SKIN = "#F6D3AD"


def svg(w, h, inner):
    return (
        f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {w} {h}">\n'
        f"{inner}\n"
        "</svg>\n"
    )


_NUM_RE = re.compile(r"-?\d+\.\d+")


def compact_numbers(text, ndigits=2):
    """浮点保留两位小数并去掉多余的 0（0.50 → .5）。"""

    def fmt(m):
        v = round(float(m.group()), ndigits)
        if v == 0:
            return "0"
        s = f"{v:.{ndigits}f}".rstrip("0").rstrip(".")
        if s.startswith("0."):
            s = s[1:]
        elif s.startswith("-0."):
            s = "-" + s[2:]
        return s

    return _NUM_RE.sub(fmt, text)


def write(rel, content):
    content = compact_numbers(content, 2)
    p = OUT / rel
    p.parent.mkdir(parents=True, exist_ok=True)
    p.write_text(content, encoding="utf-8", newline="\n")
    print(f"{rel:22s} {len(content):5d} B")


def _parse_out():
    ap = argparse.ArgumentParser(description="生成 squabby 的插画层素材")
    ap.add_argument("--out", type=Path, default=DEFAULT_OUT, help=f"主题目录，默认 {DEFAULT_OUT}")
    return ap.parse_args().out


# 先定输出目录，再执行下面的素材生成
OUT = _parse_out()
OUT.mkdir(parents=True, exist_ok=True)
if not FONT_PATH.exists():
    raise SystemExit(f"找不到字体 {FONT_PATH}，改脚本里的 FONT_PATH")
NUM_FONT = TTFont(str(FONT_PATH))

# ---------------- L1 底面 500×700 ----------------
write(
    "base.svg",
    svg(
        500,
        700,
        f'  <rect x="0" y="0" width="500" height="700" rx="64" ry="64" fill="{PAPER}"/>',
    ),
)


# ---------------- 牌背 500×700 ----------------
def lattice():
    parts = []
    for row, y in enumerate(range(96, 640, 62)):
        for x in range(46 + (row % 2) * 31, 470, 62):
            if (x - 250) ** 2 + (y - 350) ** 2 < 190 ** 2:
                continue
            parts.append(f'<path d="M{x} {y - 7} L{x + 7} {y} L{x} {y + 7} L{x - 7} {y} Z"/>')
    return "".join(parts)


write(
    "back.svg",
    svg(
        500,
        700,
        f'''  <rect x="0" y="0" width="500" height="700" rx="64" ry="64" fill="{DEEP}"/>
  <rect x="22" y="22" width="456" height="656" rx="46" ry="46" fill="none" stroke="{CREAM}" stroke-width="4"/>
  <rect x="38" y="38" width="424" height="624" rx="34" ry="34" fill="none" stroke="{CREAM}" stroke-width="1.5" opacity="0.55"/>
  <g fill="{CREAM}" opacity="0.42">{lattice()}</g>
  <circle cx="250" cy="350" r="154" fill="none" stroke="{CREAM}" stroke-width="3" opacity="0.8"/>
  <circle cx="250" cy="350" r="140" fill="{RED}" stroke="{CREAM}" stroke-width="3"/>
  <path d="M250 270 C 226 242 186 252 186 288 C 186 320 220 350 250 372 C 280 350 314 320 314 288 C 314 252 274 242 250 270 Z" fill="{CREAM}"/>
  <path d="M250 300 C 240 288 222 292 222 306 C 222 320 236 332 250 342 C 264 332 278 320 278 306 C 278 292 260 288 250 300 Z" fill="{RED}"/>
  <g fill="{GOLD}">
    <path d="M250 396 L258 418 L282 420 L264 436 L270 460 L250 448 L230 460 L236 436 L218 420 L242 418 Z"/>
    <path d="M196 350 L200 360 L210 364 L200 368 L196 378 L192 368 L182 364 L192 360 Z"/>
    <path d="M304 350 L308 360 L318 364 L308 368 L304 378 L300 368 L290 364 L300 360 Z"/>
  </g>''',
    ),
)


# ---------------- 竖排 JOKER 文字 ----------------
def joker_text_path():
    gs = NUM_FONT.getGlyphSet()
    cmap = NUM_FONT.getBestCmap()
    hmtx = NUM_FONT["hmtx"]
    d_parts, x = [], 0
    for ch in "JOKER":
        name = cmap[ord(ch)]
        pen = SVGPathPen(gs)
        gs[name].draw(TransformPen(pen, (1, 0, 0, 1, x, 0)))
        d_parts.append(pen.getCommands())
        x += hmtx[name][0]
    box = BoundsPen(gs)
    xpos = 0
    for ch in "JOKER":
        name = cmap[ord(ch)]
        gs[name].draw(TransformPen(box, (1, 0, 0, 1, xpos, 0)))
        xpos += hmtx[name][0]
    return " ".join(d_parts), box.bounds


JOKER_D, JOKER_BOX = joker_text_path()


def joker_text(cap, right, top, color):
    """左侧竖排 JOKER：从上往下读，字头朝右，整体右边缘贴 right、上边缘贴 top。

    字形坐标是 y 向上的，先 scale(s, -s) 翻正，再 rotate(90) 转成竖排（顺时针）；
    最后按包围盒平移，让首字母 J 落在 top、整列贴住 right，字不会被镜像。
    """
    x0, y0, x1, y1 = JOKER_BOX
    s = cap / (y1 - y0)
    # 旋转后：x = s*gy + e，y = s*gx + f
    e = right - cap - s * y0
    f = top - s * x0
    return (
        f'<g transform="translate({e:.3f} {f:.3f}) '
        f'rotate(90) scale({s:.6f} {-s:.6f})" fill="{color}">'
        f'<path d="{JOKER_D}"/></g>'
    )


# ---------------- 小王：小丑 500×700 ----------------
write(
    "jokers/small.svg",
    svg(
        500,
        700,
        f'''  <rect x="288" y="456" width="56" height="62" fill="#F1E9DC"/>
  <path d="M190 700 C 200 596 248 548 300 548 C 352 548 400 596 410 700 Z" fill="#2E2E2E"/>
  <path d="M226 552 C 228 518 258 510 268 534 C 278 502 322 502 332 534 C 342 510 372 518 374 552 Z" fill="#FBFBFB" stroke="#DEDEDE" stroke-width="2"/>
  <circle cx="200" cy="400" r="17" fill="#F1E9DC"/>
  <circle cx="400" cy="400" r="17" fill="#F1E9DC"/>
  <ellipse cx="300" cy="380" rx="100" ry="104" fill="#F1E9DC"/>
  <path d="M206 302 C 240 274 360 274 394 302 C 382 322 218 322 206 302 Z" fill="#2E2E2E"/>
  <path d="M206 300 C 214 254 244 216 294 204 C 346 192 392 200 416 220 C 414 238 390 250 356 254 C 304 262 256 274 224 300 Z" fill="#2E2E2E"/>
  <circle cx="418" cy="220" r="17" fill="{CREAM}"/>
  <circle cx="418" cy="220" r="6" fill="#C9C9C9"/>
  <g fill="{CREAM}" opacity="0.9">
    <circle cx="258" cy="248" r="5"/><circle cx="304" cy="236" r="6"/><circle cx="350" cy="230" r="5"/>
  </g>
  <path d="M252 396 C 262 380 278 380 288 396 M312 396 C 322 380 338 380 348 396" fill="none" stroke="#262626" stroke-width="5"/>
  <circle cx="236" cy="436" r="17" fill="#E7C6AC" opacity="0.7"/>
  <circle cx="364" cy="436" r="17" fill="#E7C6AC" opacity="0.7"/>
  <ellipse cx="300" cy="416" rx="10" ry="8" fill="#E3B48F"/>
  <path d="M282 442 C 292 460 308 460 318 442 C 308 468 292 468 282 442 Z" fill="#262626"/>
  <g fill="{CREAM}">
    <circle cx="300" cy="600" r="7"/><circle cx="300" cy="640" r="7"/><circle cx="300" cy="680" r="7"/>
  </g>
  {joker_text(52, 172, 268, INK)}''',
    ),
)


# ---------------- 大王：国王 500×700 ----------------
write(
    "jokers/big.svg",
    svg(
        500,
        700,
        f'''  <path d="M440 500 L440 690" stroke="{GOLD}" stroke-width="9"/>
  <circle cx="440" cy="484" r="21" fill="{GOLD}"/>
  <circle cx="440" cy="484" r="21" fill="none" stroke="#C98E14" stroke-width="2"/>
  <circle cx="440" cy="484" r="8" fill="{RED}"/>
  <rect x="284" y="450" width="72" height="124" fill="#EDC39C"/>
  <path d="M196 700 C 204 606 250 566 300 566 C 350 566 396 606 404 700 Z" fill="{RED}"/>
  <path d="M300 566 L266 664 L300 700 L334 664 Z" fill="{CREAM}"/>
  <path d="M300 578 L282 654 L300 690 L318 654 Z" fill="{DEEP}"/>
  <path d="M234 552 C 252 526 348 526 366 552 C 350 576 250 576 234 552 Z" fill="{CREAM}"/>
  <g fill="{INK}">
    <circle cx="266" cy="558" r="4"/><circle cx="300" cy="562" r="4"/><circle cx="334" cy="558" r="4"/>
  </g>
  <circle cx="200" cy="400" r="17" fill="#EDC39C"/>
  <circle cx="400" cy="400" r="17" fill="#EDC39C"/>
  <ellipse cx="300" cy="380" rx="100" ry="104" fill="{SKIN}"/>
  <circle cx="212" cy="340" r="26" fill="#FBFBFB"/>
  <circle cx="388" cy="340" r="26" fill="#FBFBFB"/>
  <path d="M200 420 C 186 470 192 522 224 552 C 256 578 344 578 376 552 C 408 522 414 470 400 420 C 380 448 344 458 300 458 C 256 458 220 448 200 420 Z" fill="#FBFBFB" stroke="#E4E4E4" stroke-width="2"/>
  <path d="M252 456 C 266 440 292 444 300 462 C 308 444 334 440 348 456 C 338 478 310 484 300 468 C 290 484 262 478 252 456 Z" fill="#FFFFFF" stroke="#EAEAEA" stroke-width="1.5"/>
  <path d="M252 456 C 262 464 270 468 278 466 M348 456 C 338 464 330 468 322 466" fill="none" stroke="#DFDFDF" stroke-width="3"/>
  <path d="M208 316 C 246 300 354 300 392 316 L 394 344 C 354 362 246 362 206 344 Z" fill="{GOLD}"/>
  <path d="M208 316 L 182 222 L 240 286 L 300 206 L 360 286 L 418 222 L 392 316 Z" fill="{GOLD}" stroke="#D99A12" stroke-width="2"/>
  <path d="M208 316 L 182 222 L 240 286 Z" fill="{CREAM}" opacity="0.16"/>
  <circle cx="182" cy="216" r="13" fill="{CREAM}" stroke="#D99A12" stroke-width="2"/>
  <circle cx="300" cy="200" r="13" fill="{CREAM}" stroke="#D99A12" stroke-width="2"/>
  <circle cx="418" cy="216" r="13" fill="{CREAM}" stroke="#D99A12" stroke-width="2"/>
  <circle cx="300" cy="200" r="5" fill="{RED}"/>
  <circle cx="248" cy="336" r="7" fill="{RED}"/>
  <circle cx="300" cy="340" r="7" fill="{CREAM}"/>
  <circle cx="352" cy="336" r="7" fill="{RED}"/>
  <path d="M240 386 C 250 376 264 376 274 386 M326 386 C 336 376 350 376 360 386" fill="none" stroke="#E9C79A" stroke-width="6"/>
  <ellipse cx="258" cy="406" rx="9" ry="12" fill="{INK}"/>
  <ellipse cx="342" cy="406" rx="9" ry="12" fill="{INK}"/>
  <ellipse cx="300" cy="436" rx="14" ry="10" fill="#E9B78C"/>
  {joker_text(52, 172, 268, RED)}''',
    ),
)


# ---------------- 级牌徽章（text-only 模式下不画，留作 badge 模式备用）----------------
write(
    "badges/level.svg",
    svg(
        500,
        700,
        f'''  <rect x="318" y="612" width="152" height="62" rx="31" ry="31" fill="{RED}"/>
  <path d="M348 640 L 356 626 L 364 640 L 356 654 Z" fill="{CREAM}"/>
  <path d="M376 640 L 384 626 L 392 640 L 384 654 Z" fill="{CREAM}"/>
  <path d="M404 630 L 412 616 L 420 630 L 412 644 Z" fill="{CREAM}"/>
  <rect x="432" y="630" width="20" height="20" rx="6" fill="{CREAM}"/>''',
    ),
)


# ---------------- 逢人配徽章 120×120 ----------------
write(
    "badges/wild.svg",
    svg(
        120,
        120,
        f'''  <circle cx="60" cy="60" r="54" fill="{PAPER}" stroke="{RED}" stroke-width="7"/>
  <path d="M60 26 C 44 12 20 20 20 42 C 20 60 40 78 60 96 C 80 78 100 60 100 42 C 100 20 76 12 60 26 Z" fill="{RED}"/>
  <path d="M96 40 L 100 50 L 110 54 L 100 58 L 96 68 L 92 58 L 82 54 L 92 50 Z" fill="{GOLD}"/>
  <path d="M16 74 L 19 81 L 26 84 L 19 87 L 16 94 L 13 87 L 6 84 L 13 81 Z" fill="{GOLD}"/>''',
    ),
)

print("完成：底面 / 牌背 / 大小王 / 2 个徽章")
