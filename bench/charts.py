#!/usr/bin/env python3
"""Draws the README charts (assets/chart-*.svg) from the benchmark results.

Two charts, each in English and Korean and in a light and a dark version (README.md picks one
with <picture> and prefers-color-scheme):
  chart-sizes:  archive size as a share of the original, per data set, for zip, 7z, and ezpz
  chart-small:  15 small files compressed one by one, total size per method

Colors follow the "emphasis" form: the other tools in gray, ezpz in blue (light blue = the
default level, blue = the brain codecs). Adjacent bars were checked for color-blind separation.
Run:  python3 bench/charts.py"""
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import report  # noqa: E402  (merged benchmark rows; importing writes nothing)

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")
OUT = os.path.join(ROOT, "assets")
FONT = "-apple-system, BlinkMacSystemFont, 'Segoe UI', 'Noto Sans KR', 'Apple SD Gothic Neo', Helvetica, Arial, sans-serif"

THEMES = {
    "light": {"ink": "#1f2328", "ink2": "#59636e", "grid": "#e4e7eb", "base": "#c3c9d1",
              "zip": "#c3c2b7", "7z": "#898781", "default": "#86b6ef", "best": "#0155ff"},
    "dark": {"ink": "#e6edf3", "ink2": "#9198a1", "grid": "#262c36", "base": "#3d444d",
             "zip": "#484f58", "7z": "#7d8590", "default": "#2563eb", "best": "#7cc2ff"},
}

TEXT = {
    "en": {
        "sizes_title": "Archive size as a share of the original",
        "sizes_sub": "Shorter is better. Each bar ends with the compressed size.",
        "small_title": "15 small files of 4 to 35 KB, each compressed on its own",
        "small_sub": "Total size after compression. Shorter is better.",
        "series": {"zip": "zip -9", "7z": "7z -mx9", "default": "ezpz default (-l 7)", "best": "ezpz -l 11"},
        "datasets": {"text": "Wikipedia text", "silesia": "Silesia corpus", "source": "Python install folder",
                     "backups": "Backup of 3 Python versions", "binaries": "Linux executables"},
        "orig": lambda mb: f"{mb:.0f} MB original",
        "small_names": {"ezpz (default -l 7)": "ezpz default (-l 7)", "ezpz --max (-l 10)": "ezpz --max"},
        "mb": "MB", "kb": "KB",
    },
    "ko": {
        "sizes_title": "압축 후 크기 (원래 크기 대비)",
        "sizes_sub": "짧을수록 좋아요. 막대 끝 숫자는 압축 후 크기예요.",
        "small_title": "4~35KB 작은 파일 15개를 하나씩 압축",
        "small_sub": "압축 후 크기를 모두 더한 값이에요. 짧을수록 좋아요.",
        "series": {"zip": "zip -9", "7z": "7z -mx9", "default": "ezpz 기본 (-l 7)", "best": "ezpz -l 11"},
        "datasets": {"text": "위키백과 텍스트", "silesia": "Silesia 표준 세트", "source": "Python 설치 폴더",
                     "backups": "Python 3개 버전 백업", "binaries": "리눅스 실행 파일"},
        "orig": lambda mb: f"원래 {mb:.0f}MB",
        "small_names": {"ezpz (default -l 7)": "ezpz 기본 (-l 7)", "ezpz --max (-l 10)": "ezpz --max"},
        "mb": "MB", "kb": "KB",
    },
}

SERIES = [("zip", "zip -9"), ("7z", "7z -mx9"), ("default", "ezpz (default -l 7)"), ("best", "ezpz -l 11")]


def text_width(s, size):
    """Rough rendered width: Latin letters about 0.53 em, Hangul and other wide letters 1 em."""
    return sum(size * (1.0 if ord(ch) > 0x2E80 else 0.53) for ch in s)


def esc(s):
    return s.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")


def bar(x, y, w, h, color, r=4):
    """Horizontal bar: square at the baseline (left), rounded at the data end (right)."""
    if w <= 0:
        return ""
    r = min(r, w, h / 2)
    return (f'<path d="M{x:.1f},{y:.1f} H{x + w - r:.1f} Q{x + w:.1f},{y:.1f} {x + w:.1f},{y + r:.1f} '
            f'V{y + h - r:.1f} Q{x + w:.1f},{y + h:.1f} {x + w - r:.1f},{y + h:.1f} H{x:.1f} Z" fill="{color}"/>')


def text(x, y, s, size, color, weight=400, anchor="start"):
    return (f'<text x="{x:.1f}" y="{y:.1f}" font-size="{size}" font-weight="{weight}" fill="{color}" '
            f'text-anchor="{anchor}">{esc(s)}</text>')


def svg(width, height, body, label):
    return (f'<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="0 0 {width} {height}" '
            f'font-family="{FONT}" role="img" aria-label="{esc(label)}">\n{body}\n</svg>\n')


def chart_sizes(lang, theme):
    t, c = TEXT[lang], THEMES[theme]
    width, left, right = 780, 196, 80
    plot = width - left - right
    top = 92
    bar_h, gap = 12, 2
    group_h = 4 * bar_h + 3 * gap
    pitch = group_h + 22
    xmax = 0.40
    out = [text(0, 20, t["sizes_title"], 16, c["ink"], 600), text(0, 41, t["sizes_sub"], 13, c["ink2"])]
    # legend
    lx = 0
    for key, _ in SERIES:
        label = t["series"][key]
        out.append(f'<rect x="{lx}" y="56" width="12" height="12" rx="3" fill="{c[key]}"/>')
        out.append(text(lx + 17, 66.5, label, 12.5, c["ink"]))
        lx += 17 + text_width(label, 12.5) + 20
    n = len(report.DATASETS)
    plot_bottom = top + n * pitch - 22 + 8
    for i in range(5):
        v = i * 0.10
        x = left + plot * v / xmax
        out.append(f'<line x1="{x:.1f}" y1="{top - 6}" x2="{x:.1f}" y2="{plot_bottom:.1f}" stroke="{c["grid"] if i else c["base"]}" stroke-width="1"/>')
        out.append(text(x, plot_bottom + 18, f"{v * 100:.0f}%", 12, c["ink2"], anchor="middle"))
    for gi, ds in enumerate(report.DATASETS):
        y0 = top + gi * pitch
        raw = report.RAW[ds]
        out.append(text(left - 12, y0 + group_h / 2 - 2, t["datasets"][ds], 13, c["ink"], 600, "end"))
        out.append(text(left - 12, y0 + group_h / 2 + 14, t["orig"](raw / 1e6), 12, c["ink2"], 400, "end"))
        for si, (key, method) in enumerate(SERIES):
            size = report.rows[(ds, method)]["size"]
            share = size / raw
            y = y0 + si * (bar_h + gap)
            w = plot * share / xmax
            out.append(bar(left, y, w, bar_h, c[key]))
            label = f"{size / 1e6:.1f} {t['mb']}" if lang == "en" else f"{size / 1e6:.1f}{t['mb']}"
            out.append(text(left + w + 6, y + bar_h - 2, label, 11.5, c["ink"], 600 if key == "best" else 400))
    height = int(plot_bottom + 30)
    return svg(width, height, "\n".join(out), t["sizes_title"])


def chart_small(lang, theme):
    t, c = TEXT[lang], THEMES[theme]
    tot = {}
    for r in report.SMALL:
        tot[r["method"]] = tot.get(r["method"], 0) + r["size"]
    items = sorted(tot.items(), key=lambda kv: -kv[1])
    width, left, right = 780, 196, 80
    plot = width - left - right
    top, bar_h, pitch = 62, 16, 26
    xmax = 90_000
    out = [text(0, 20, t["small_title"], 16, c["ink"], 600), text(0, 41, t["small_sub"], 13, c["ink2"])]
    plot_bottom = top + len(items) * pitch - (pitch - bar_h) + 8
    for i in range(4):
        v = i * 30_000
        x = left + plot * v / xmax
        out.append(f'<line x1="{x:.1f}" y1="{top - 6}" x2="{x:.1f}" y2="{plot_bottom:.1f}" stroke="{c["grid"] if i else c["base"]}" stroke-width="1"/>')
        out.append(text(x, plot_bottom + 18, f"{v // 1000} {t['kb']}" if lang == "en" else f"{v // 1000}{t['kb']}", 12, c["ink2"], anchor="middle"))
    for i, (method, size) in enumerate(items):
        y = top + i * pitch
        if method == "ezpz (default -l 7)":
            color, weight = c["default"], 600
        elif method.startswith("ezpz"):
            color, weight = c["best"], 600
        else:
            color, weight = c["7z"], 400
        name = t["small_names"].get(method, method)
        out.append(text(left - 12, y + bar_h - 3, name, 13, c["ink"], weight, "end"))
        w = plot * size / xmax
        out.append(bar(left, y, w, bar_h, color))
        label = f"{size / 1000:.1f} {t['kb']}" if lang == "en" else f"{size / 1000:.1f}{t['kb']}"
        out.append(text(left + w + 6, y + bar_h - 3, label, 12, c["ink"], weight))
    height = int(plot_bottom + 30)
    return svg(width, height, "\n".join(out), t["small_title"])


def main():
    os.makedirs(OUT, exist_ok=True)
    for lang in ("en", "ko"):
        suffix = "" if lang == "en" else "-ko"
        for theme in ("light", "dark"):
            for name, fn in (("sizes", chart_sizes), ("small", chart_small)):
                path = os.path.join(OUT, f"chart-{name}{suffix}-{theme}.svg")
                with open(path, "w", encoding="utf-8") as f:
                    f.write(fn(lang, theme))
                print("wrote", os.path.relpath(path, ROOT))


if __name__ == "__main__":
    main()
