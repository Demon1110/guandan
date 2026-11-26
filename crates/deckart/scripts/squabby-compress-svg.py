r"""把主题目录下所有 SVG 里的长浮点压缩到固定小数位，减小体积。

用法（在仓库根目录执行）：
    python crates/deckart/scripts/squabby-compress-svg.py
    python crates/deckart/scripts/squabby-compress-svg.py crates/deckart/themes/squabby --decimals 2
    python crates/deckart/scripts/squabby-compress-svg.py --dry-run        # 只看体积，不写文件

做什么：
- 匹配所有 -?\d+\.\d+ 的浮点，按 --decimals 四舍五入；
- 去掉尾部 0，并把 0.5 写成 .5、-0.5 写成 -.5（SVG / path 解析都认）；
- 只处理数值，不动结构、颜色和小数点前的整数，所以可以反复跑。

生成脚本（squabby-ranks.py / squabby-art.py）写文件时已经内置同样压缩，
这个脚本用于“素材已经存在、只想再瘦一遍”或其它主题临时压缩。
"""

import argparse
import re
from pathlib import Path

REPO = Path(__file__).resolve().parents[3]
DEFAULT_ROOT = REPO / "crates" / "deckart" / "themes" / "squabby"

NUM_RE = re.compile(r"-?\d+\.\d+")


def compact(text, ndigits=2):
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

    return NUM_RE.sub(fmt, text)


def main():
    ap = argparse.ArgumentParser(description="压缩主题 SVG 里的浮点")
    ap.add_argument("root", nargs="?", type=Path, default=DEFAULT_ROOT, help=f"主题目录，默认 {DEFAULT_ROOT}")
    ap.add_argument("--decimals", type=int, default=2, help="保留小数位，默认 2")
    ap.add_argument("--dry-run", action="store_true", help="只统计，不写回文件")
    args = ap.parse_args()

    total_before = total_after = 0
    for p in sorted(args.root.rglob("*.svg")):
        raw = p.read_text(encoding="utf-8")
        new = compact(raw, args.decimals)
        before = len(raw.encode("utf-8"))
        after = len(new.encode("utf-8"))
        total_before += before
        total_after += after
        if not args.dry_run and new != raw:
            p.write_text(new, encoding="utf-8", newline="\n")
        print(f"{str(p.relative_to(args.root)):22s} {before:6d} → {after:6d} B")
    if total_before:
        saved = (1 - total_after / total_before) * 100
        print(f"total {total_before} → {total_after} B  ({saved:.1f}% saved)")
    else:
        print(f"{args.root} 下没有 SVG")


if __name__ == "__main__":
    main()
