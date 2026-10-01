#!/usr/bin/env python3
"""Merge benchmark results and write BENCHMARK.md (English) and BENCHMARK.ko.md (Korean).

The discussion at the end of each report comes from notes.en.md / notes.ko.md.
"""
import json, os

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.join(HERE, "..")

v1 = [json.loads(l) for l in open(os.path.join(HERE, "results_v1.jsonl"))]
v2 = [json.loads(l) for l in open(os.path.join(HERE, "results_v2.jsonl"))]
rows = {}
for r in v1:
    if "ezpz" not in r["method"]:
        rows[(r["dataset"], r["method"])] = r
for r in v2:  # later rows win
    rows[(r["dataset"], r["method"])] = r

DATASETS = ["text", "silesia", "source", "backups", "binaries"]
# Method keys as stored in the result files.
METHODS = ["zip -9", "tar.gz -9", "tar.zst -19 --long", "tar.xz -9e", "7z -mx9",
           "ezpz -l 3", "ezpz (default -l 7)", "ezpz -l 9", "ezpz --max (brain)"]
MAX = "ezpz --max (brain)"

L = {
    "en": {
        "file": "BENCHMARK.md",
        "switch": "English | [한국어](BENCHMARK.ko.md)",
        "title": "# .ezpz benchmark",
        "intro": "ezpz compared with zip, tar.gz, tar.zst, tar.xz, and 7z on the same data.",
        "setup_h": "## Test setup",
        "setup": [
            "- A 2-core Intel Xeon 2.1 GHz virtual server (on the slow side), Linux. Every tool that supports threads ran with 2 threads (`zstd -T2`, `xz -T2`, `7z -mmt=2`, `ezpz -j2`).",
            "- Versions: zstd 1.5.5, XZ Utils 5.4.5, 7-Zip 23.01, Info-ZIP 3.0, gzip 1.12, GNU tar 1.35, ezpz 0.1.0",
            "- Each measurement was taken once on an otherwise idle server, so expect a few percent of noise in the timings.",
            "- Size is the size of the whole output file, index included. Ratio is the output size as a percentage of the input, so lower is better.",
            "- Single file is the time to extract one small file (for example `json/decoder.py`) from the archive. The tar formats have to be decompressed from the start to reach it.",
            "- Memory is the peak memory use while compressing / extracting.",
            "- Every ezpz extraction was compared byte for byte with the original.",
        ],
        "summary_h": "## Summary: size (% of original, lower is better)",
        "method": "Method",
        "best_mark": "",
        "max_h": "### The brain codec (`--max`) against the best existing settings",
        "max_cols": "| Dataset | vs xz -9e | vs 7z -mx9 |",
        "max_note": "Negative numbers mean the brain codec output is that much smaller.",
        "table_cols": "| Method | Size (MB) | Ratio | Compress | Extract | Single file | Memory (compress / extract) |",
        "missing": "n/a",
        "sec": lambda x: (f"{x:.2f} s" if x < 1 else f"{x:.1f} s" if x < 100 else f"{x:.0f} s"),
        "discussion_h": "## Discussion",
        "notes": "notes.en.md",
        "names": {
            "text": ("Wikipedia text", "enwik8: the first 100 MB of an English Wikipedia XML dump (1 file)"),
            "silesia": ("Silesia corpus", "Silesia corpus: a standard set used in compression research (212 MB, 12 files: literature, databases, medical images, executables, XML, and more)"),
            "source": ("Program install folder", "Python 3.12 install folder (53 MB, 1,212 files: .py sources, compiled .pyc files, libraries)"),
            "backups": ("Multi-version backup", "The Python 3.11, 3.12, and 3.13 install folders together (158 MB, 3,750 files), standing in for an archive that keeps several versions of one project"),
            "binaries": ("Executables", "231 executables from Linux /usr/bin (105 MB, x86-64)"),
        },
        "methods": {},
    },
    "ko": {
        "file": "BENCHMARK.ko.md",
        "switch": "[English](BENCHMARK.md) | 한국어",
        "title": "# .ezpz 벤치마크",
        "intro": "zip, tar.gz, tar.zst, tar.xz, 7z와 같은 데이터를 압축해서 비교했어요.",
        "setup_h": "## 측정 환경",
        "setup": [
            "- 2코어 Intel Xeon 2.1GHz 가상 서버(느린 편), 리눅스. 스레드를 지원하는 도구는 모두 2스레드로 실행했어요 (`zstd -T2`, `xz -T2`, `7z -mmt=2`, `ezpz -j2`).",
            "- 도구 버전: zstd 1.5.5, XZ Utils 5.4.5, 7-Zip 23.01, Info-ZIP 3.0, gzip 1.12, GNU tar 1.35, ezpz 0.1.0",
            "- 각 항목은 한 번씩 측정했어요. 시간은 같은 서버에서 다른 작업이 거의 없을 때 잰 값이지만 몇 % 정도의 오차는 있을 수 있어요.",
            "- **크기**는 결과 파일 전체 크기(인덱스·목록 포함)입니다. **비율**은 원본 대비 %라서 작을수록 좋아요.",
            "- **파일 하나 꺼내기**: 아카이브 속 작은 파일 1개(예: `json/decoder.py`)를 꺼내는 데 걸린 시간. tar 계열은 구조상 앞에서부터 풀어야 해요.",
            "- **메모리**: 압축 / 해제할 때 최대 사용량.",
            "- ezpz 해제 결과는 매번 원본과 바이트 단위로 비교해 같은지 확인했어요.",
        ],
        "summary_h": "## 한눈에: 크기 비교 (원본 대비 %, 작을수록 좋음)",
        "method": "방식",
        "best_mark": " 🥇",
        "max_h": "### 뇌 코덱(`--max`)을 기존 최고 압축과 비교하면",
        "max_cols": "| 데이터 | xz -9e 대비 | 7z -mx9 대비 |",
        "max_note": "음수면 뇌 코덱이 그만큼 더 작다는 뜻이에요.",
        "table_cols": "| 방식 | 크기(MB) | 비율 | 압축 | 해제 | 파일 하나 꺼내기 | 메모리 (압축/해제) |",
        "missing": "–",
        "sec": lambda x: (f"{x:.2f}초" if x < 1 else f"{x:.1f}초" if x < 100 else f"{x:.0f}초"),
        "discussion_h": "## 해석",
        "notes": "notes.ko.md",
        "names": {
            "text": ("위키백과 텍스트", "enwik8: 영어 위키백과 XML 덤프의 앞 100MB (파일 1개)"),
            "silesia": ("표준 테스트 세트", "Silesia corpus: 압축 연구에서 쓰는 표준 세트 (212MB, 파일 12개: 문학 텍스트, 데이터베이스, 의료 영상, 실행 파일, XML 등)"),
            "source": ("프로그램 설치 폴더", "Python 3.12 설치 폴더 (53MB, 파일 1,212개: .py 소스, 컴파일된 .pyc, 라이브러리)"),
            "backups": ("여러 버전 백업", "Python 3.11 · 3.12 · 3.13 설치 폴더 3개를 함께 (158MB, 파일 3,750개). 같은 프로젝트의 여러 버전을 보관하는 상황이에요"),
            "binaries": ("실행 파일", "리눅스 /usr/bin의 실행 파일 231개 (105MB, x86-64)"),
        },
        "methods": {"ezpz (default -l 7)": "ezpz (기본 -l 7)", "ezpz --max (brain)": "ezpz --max (뇌)"},
    },
}


def render(lang):
    s = L[lang]
    name = lambda m: s["methods"].get(m, m)
    t = lambda x: s["missing"] if x is None else s["sec"](x)
    best = {ds: min(rows[(ds, m)]["ratio"] for m in METHODS if (ds, m) in rows) for ds in DATASETS}
    is_best = lambda ds, r: abs(r["ratio"] - best[ds]) < 1e-12

    out = [s["title"], "", s["switch"], "", s["intro"], "", s["setup_h"], ""]
    out += s["setup"] + [""]

    out += [s["summary_h"], ""]
    out.append(f"| {s['method']} | " + " | ".join(s["names"][ds][0] for ds in DATASETS) + " |")
    out.append("|---" * (len(DATASETS) + 1) + "|")
    for m in METHODS:
        cells = []
        for ds in DATASETS:
            r = rows.get((ds, m))
            if not r:
                cells.append(s["missing"])
                continue
            c = f"{r['ratio'] * 100:.1f}%"
            cells.append(f"**{c}**{s['best_mark']}" if is_best(ds, r) else c)
        out.append(f"| {name(m)} | " + " | ".join(cells) + " |")
    out.append("")

    out += [s["max_h"], "", s["max_cols"], "|---|---|---|"]
    pct = lambda a, b: f"{(a / b - 1) * 100:+.1f}%"
    for ds in DATASETS:
        mx = rows[(ds, MAX)]["size"]
        out.append(f"| {s['names'][ds][0]} | {pct(mx, rows[(ds, 'tar.xz -9e')]['size'])} | "
                   f"{pct(mx, rows[(ds, '7z -mx9')]['size'])} |")
    out += ["", s["max_note"], ""]

    for ds in DATASETS:
        title, desc = s["names"][ds]
        out += [f"## {title}", "", desc, "", s["table_cols"], "|---|---|---|---|---|---|---|"]
        for m in METHODS:
            r = rows.get((ds, m))
            if not r:
                continue
            size, ratio = f"{r['size'] / 1e6:.2f}", f"{r['ratio'] * 100:.2f}%"
            if is_best(ds, r):
                size, ratio = f"**{size}**", f"**{ratio}**"
            out.append(f"| {name(m)} | {size} | {ratio} | {t(r['c_sec'])} | {t(r['x_sec'])} | "
                       f"{t(r['rand_sec'])} | {r['c_mem']} / {r['x_mem']} MB |")
        out.append("")

    out += [s["discussion_h"], ""]
    out += open(os.path.join(HERE, s["notes"]), encoding="utf-8").read().strip().splitlines()
    text = "\n".join(out) + "\n"
    open(os.path.join(ROOT, s["file"]), "w", encoding="utf-8").write(text)
    return text


if __name__ == "__main__":
    for lang in ("en", "ko"):
        render(lang)
        print(f"wrote {L[lang]['file']}")
