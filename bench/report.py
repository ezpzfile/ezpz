#!/usr/bin/env python3
"""Merge benchmark results and write BENCHMARK.md (English) and BENCHMARK.ko.md (Korean).

The discussion at the end of each report comes from notes.en.md / notes.ko.md.
"""
import json, os

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.join(HERE, "..")

load = lambda name: [json.loads(l) for l in open(os.path.join(HERE, name))]
# v2 measured the full brain codec with 64 MiB blocks under its old name, "--max".
# That setting is now level 11 and produces the same compressed data, so the rows are relabeled.
RENAMED = {"ezpz --max (brain)": "ezpz -l 11"}
rows = {}
for r in load("results_v1.jsonl"):
    if "ezpz" not in r["method"]:
        rows[(r["dataset"], r["method"])] = r
for r in load("results_v2.jsonl") + load("results_v3.jsonl") + load("results_v4.jsonl") + load("results_v5.jsonl"):  # later rows win
    r = dict(r, method=RENAMED.get(r["method"], r["method"]))
    rows[(r["dataset"], r["method"])] = r

DATASETS = ["text", "silesia", "source", "backups", "binaries"]
# Method keys as stored in the result files.
METHODS = ["zip -9", "tar.gz -9", "tar.zst -19 --long", "tar.xz -9e", "7z -mx9",
           "ezpz -l 3", "ezpz (default -l 7)", "ezpz -l 9", "ezpz --max (-l 10)", "ezpz -l 11"]
BRAIN = [("ezpz --max (-l 10)", "ezpz --max"), ("ezpz -l 11", "ezpz -l 11")]  # (result key, column name)
RIVALS = [("tar.xz -9e", "xz -9e"), ("7z -mx9", "7z -mx9")]  # (result key, short name)

SMALL = [json.loads(l) for l in open(os.path.join(HERE, "results_small.jsonl"))]
SMALL_METHODS = list(dict.fromkeys(r["method"] for r in SMALL))
SMALL_FILES = list(dict.fromkeys(r["file"] for r in SMALL))

# Original (uncompressed) size of each dataset, in bytes.
RAW = {}
for (ds, m), r in rows.items():
    assert RAW.setdefault(ds, r["raw"]) == r["raw"], (ds, m)

L = {
    "en": {
        "file": "BENCHMARK.md",
        "switch": "English | [한국어](BENCHMARK.ko.md)",
        "title": "# .ezpz benchmark",
        "intro": "ezpz compared with zip, tar.gz, tar.zst, tar.xz, and 7z on the same data.",
        "setup_h": "## Test setup",
        "setup": [
            "- A 2-core Intel Xeon 2.1 GHz virtual server (on the slow side), Linux. Every tool that supports threads ran with 2 threads (`zstd -T2`, `xz -T2`, `7z -mmt=2`, `ezpz -j2`).",
            "- Versions: zstd 1.5.5, XZ Utils 5.4.5, 7-Zip 23.01, Info-ZIP 3.0, gzip 1.12, GNU tar 1.35, ezpz 0.4.0 (0.5.0 writes the same data). On the Wikipedia text and the Silesia corpus, the ezpz rows come from earlier versions that write the same data for them: levels 3, 7, 9, and 11 from ezpz 0.1.0, and `--max` from ezpz 0.3.0.",
            "- Each measurement was taken once on an otherwise idle server, so expect a few percent of noise in the timings.",
            "- Sizes read original → compressed, in MB (1 MB = 1,000,000 bytes). The compressed size is the whole archive file, index included. \"80% smaller\" means the archive is one fifth the size of the original.",
            "- Single file is the time to extract one small file (for example `json/decoder.py`) from the archive. The tar formats have to be decompressed from the start to reach it.",
            "- Peak RAM is the most memory (RAM) the tool used while compressing / extracting. It is not a file size.",
            "- Every ezpz extraction was compared byte for byte with the original.",
            "- `--max` is level 10: the fast brain codec (codec 4) with 16 MiB blocks. `-l 11` is the full brain codec (codec 3) with 64 MiB blocks; ezpz 0.1.0 called this setting `--max`.",
        ],
        "summary_h": "## Summary: size after compression, in MB (smaller is better)",
        "method": "Method",
        "original_row": "**Original (before compression)**",
        "best_mark": "",
        "mb": " MB",
        "max_h": "### The brain codecs against the best existing settings",
        "max_cols": "| Dataset | Original | xz -9e | 7z -mx9 | ezpz --max | ezpz -l 11 |",
        "max_note": "In parentheses: each ezpz size compared with whichever of xz and 7z made the smaller file.",
        "vs": lambda p, rival, smaller: f"{p:.1f}% {'smaller' if smaller else 'larger'} than {rival}",
        "table_cols": "| Method | Original → compressed | Size reduction | Compress time | Extract time | Single file | Peak RAM while compressing / extracting |",
        "reduction": lambda p: f"{p:.1f}% smaller",
        "missing": "n/a",
        "sec": lambda x: (f"{x:.2f} s" if x < 1 else f"{x:.1f} s" if x < 100 else f"{x:.0f} s"),
        "small_h": "## Small files",
        "small_intro": [
            "Fifteen files of 4 to 35 KB, each compressed on its own. Sizes are in KB (1 KB = 1,000 bytes) and include each format's own headers. gzip, zstd, and xz write a single compressed stream with almost no header. zip, 7z, and ezpz write archives with an index; for ezpz that index plus the trailer adds about 250 bytes per archive, mostly hashes. Both brain codecs start these small blocks from the built-in priming data (SPEC §6.4.11), and the default level uses the same data as a zstd dictionary (§6.6).",
        ],
        "small_cols": "| Content | Original |",
        "small_total": "**Total**",
        "small_note": lambda mx, xz, zp, l7: f"In total, ezpz --max is {(1 - mx / xz) * 100:.1f}% smaller than xz -9e and {(1 - mx / zp) * 100:.1f}% smaller than zip -9 on these files. The default level is {(1 - l7 / zp) * 100:.1f}% smaller than zip -9.",
        "kb": " KB",
        "small_names": {
            "c_header.h": "C header (glibc stdio.h)", "data.json": "JSON (ISO country codes)",
            "data.tab": "Table, tab-separated (time zones)", "data.xml": "XML (Silesia slice)",
            "en_license.txt": "English license text (Apache 2.0)", "en_novel.txt": "English novel (Dickens)",
            "en_page.html": "English web page (HTML)", "es_tutor.txt": "Spanish tutorial",
            "ja_page.html": "Japanese web page (HTML)", "ja_tutor.txt": "Japanese tutorial",
            "ko_readme.md": "Korean Markdown (README)", "ko_spec.md": "Korean technical document",
            "ko_tutor.txt": "Korean tutorial", "py_module.py": "Python source (textwrap.py)",
            "wiki.xml": "Wikipedia XML",
        },
        "discussion_h": "## Discussion",
        "notes": "notes.en.md",
        "names": {
            "text": ("Wikipedia text", "enwik8: the first 100 MB of an English Wikipedia XML dump (1 file)"),
            "silesia": ("Silesia corpus", "Silesia corpus: a standard set used in compression research (212 MB, 12 files: literature, databases, medical images, executables, XML, and more)"),
            "source": ("Program install folder", "Python 3.12 install folder (53 MB, 1,212 files: .py sources, compiled .pyc files, libraries)"),
            "backups": ("Multi-version backup", "The Python 3.11, 3.12, and 3.13 install folders together (158 MB, 3,750 files), standing in for an archive that keeps several versions of one project"),
            "binaries": ("Executables", "231 executables from Linux /usr/bin (105 MB, x86-64)"),
        },
        "readme_names": {"text": "Wikipedia text (enwik8)", "silesia": "Silesia corpus", "source": "Python install folder",
                         "backups": "Backup of three Python versions", "binaries": "Linux executables"},
        "methods": {"ezpz --max (-l 10)": "ezpz --max (-l 10, fast brain)", "ezpz -l 11": "ezpz -l 11 (brain)"},
    },
    "ko": {
        "file": "BENCHMARK.ko.md",
        "switch": "[English](BENCHMARK.md) | 한국어",
        "title": "# .ezpz 벤치마크",
        "intro": "zip, tar.gz, tar.zst, tar.xz, 7z와 같은 데이터를 압축해서 비교했어요.",
        "setup_h": "## 측정 환경",
        "setup": [
            "- 2코어 Intel Xeon 2.1GHz 가상 서버(느린 편), 리눅스. 스레드를 지원하는 도구는 모두 2스레드로 실행했어요 (`zstd -T2`, `xz -T2`, `7z -mmt=2`, `ezpz -j2`).",
            "- 도구 버전: zstd 1.5.5, XZ Utils 5.4.5, 7-Zip 23.01, Info-ZIP 3.0, gzip 1.12, GNU tar 1.35, ezpz 0.4.0(0.5.0도 같은 데이터를 만들어요). 위키백과 텍스트와 표준 테스트 세트의 ezpz 줄은 같은 데이터를 만드는 이전 버전으로 쟀어요. 레벨 3, 7, 9, 11은 ezpz 0.1.0, `--max`는 ezpz 0.3.0이에요.",
            "- 각 항목은 한 번씩 측정했어요. 시간은 같은 서버에서 다른 작업이 거의 없을 때 잰 값이지만 몇 % 정도의 오차는 있을 수 있어요.",
            "- 크기는 **원래 크기 → 압축 후 크기** 순서로, MB 단위(1MB = 1,000,000바이트)로 적었어요. 압축 후 크기는 결과 파일 전체(인덱스·목록 포함)예요. **80% 줄어듦**이면 압축 파일이 원래의 5분의 1 크기라는 뜻이에요.",
            "- **파일 하나 꺼내기**: 아카이브 속 작은 파일 1개(예: `json/decoder.py`)를 꺼내는 데 걸린 시간. tar 계열은 구조상 앞에서부터 풀어야 해요.",
            "- **작업 중 최대 메모리**: 압축하거나 푸는 동안 컴퓨터 메모리(RAM)를 가장 많이 쓴 양이에요. 파일 크기가 아니에요.",
            "- ezpz 해제 결과는 매번 원본과 바이트 단위로 비교해 같은지 확인했어요.",
            "- `--max`는 레벨 10이에요. 빠른 뇌 코덱(코덱 4)에 16 MiB 블록을 써요. `-l 11`은 원래 뇌 코덱(코덱 3)에 64 MiB 블록이고, ezpz 0.1.0에서는 이 설정을 `--max`라고 불렀어요.",
        ],
        "summary_h": "## 한눈에: 압축 후 크기 (MB, 작을수록 좋음)",
        "method": "방식",
        "original_row": "**원래 크기 (압축 전)**",
        "best_mark": " 🥇",
        "mb": "MB",
        "max_h": "### 뇌 코덱을 기존 최고 압축과 비교하면",
        "max_cols": "| 데이터 | 원래 크기 | xz -9e | 7z -mx9 | ezpz --max | ezpz -l 11 |",
        "max_note": "괄호 안은 xz와 7z 중 더 작게 만든 쪽과 비교한 값이에요.",
        "vs": lambda p, rival, smaller: f"{rival}보다 {p:.1f}% {'작음' if smaller else '큼'}",
        "table_cols": "| 방식 | 원래 크기 → 압축 후 | 줄어든 정도 | 압축 시간 | 해제 시간 | 파일 하나 꺼내기 | 작업 중 최대 메모리 (압축할 때 / 풀 때) |",
        "reduction": lambda p: f"{p:.1f}% 줄어듦",
        "missing": "해당 없음",
        "sec": lambda x: (f"{x:.2f}초" if x < 1 else f"{x:.1f}초" if x < 100 else f"{x:.0f}초"),
        "small_h": "## 작은 파일",
        "small_intro": [
            "4~35KB짜리 파일 15개를 하나씩 따로 압축했어요. 크기는 KB(1KB = 1,000바이트) 단위이고 각 형식의 머리말까지 포함해요. gzip, zstd, xz는 머리말이 거의 없는 압축 스트림 하나만 써요. zip, 7z, ezpz는 목록이 붙은 아카이브인데, ezpz는 목록과 끝부분 정보가 아카이브마다 250바이트쯤 더 붙고 대부분은 해시예요. 두 뇌 코덱 모두 이런 작은 블록은 내장된 사전 학습 데이터로 미리 배운 상태에서 시작하고(SPEC §6.4.11), 기본 레벨은 같은 데이터를 zstd 사전으로 써요(§6.6).",
        ],
        "small_cols": "| 내용 | 원래 크기 |",
        "small_total": "**합계**",
        "small_note": lambda mx, xz, zp, l7: f"합치면 ezpz --max가 xz -9e보다 {(1 - mx / xz) * 100:.1f}%, zip -9보다 {(1 - mx / zp) * 100:.1f}% 작아요. 기본 레벨은 zip -9보다 {(1 - l7 / zp) * 100:.1f}% 작아요.",
        "kb": "KB",
        "small_names": {
            "c_header.h": "C 헤더 (glibc stdio.h)", "data.json": "JSON (ISO 국가 코드)",
            "data.tab": "탭으로 구분한 표 (시간대 목록)", "data.xml": "XML (Silesia 일부)",
            "en_license.txt": "영어 라이선스 문서 (Apache 2.0)", "en_novel.txt": "영어 소설 (디킨스)",
            "en_page.html": "영어 웹 페이지 (HTML)", "es_tutor.txt": "스페인어 사용 안내서",
            "ja_page.html": "일본어 웹 페이지 (HTML)", "ja_tutor.txt": "일본어 사용 안내서",
            "ko_readme.md": "한국어 마크다운 (README)", "ko_spec.md": "한국어 기술 문서",
            "ko_tutor.txt": "한국어 사용 안내서", "py_module.py": "파이썬 소스 (textwrap.py)",
            "wiki.xml": "위키백과 XML",
        },
        "discussion_h": "## 해석",
        "notes": "notes.ko.md",
        "names": {
            "text": ("위키백과 텍스트", "enwik8: 영어 위키백과 XML 덤프의 앞 100MB (파일 1개)"),
            "silesia": ("표준 테스트 세트", "Silesia corpus: 압축 연구에서 쓰는 표준 세트 (212MB, 파일 12개: 문학 텍스트, 데이터베이스, 의료 영상, 실행 파일, XML 등)"),
            "source": ("프로그램 설치 폴더", "Python 3.12 설치 폴더 (53MB, 파일 1,212개: .py 소스, 컴파일된 .pyc, 라이브러리)"),
            "backups": ("여러 버전 백업", "Python 3.11 · 3.12 · 3.13 설치 폴더 3개를 함께 (158MB, 파일 3,750개). 같은 프로젝트의 여러 버전을 보관하는 상황이에요"),
            "binaries": ("실행 파일", "리눅스 /usr/bin의 실행 파일 231개 (105MB, x86-64)"),
        },
        "readme_names": {"text": "위키백과 텍스트 (enwik8)", "silesia": "Silesia 표준 테스트 세트", "source": "Python 설치 폴더",
                         "backups": "Python 3개 버전 백업", "binaries": "리눅스 실행 파일"},
        "methods": {"ezpz (default -l 7)": "ezpz (기본 -l 7)", "ezpz --max (-l 10)": "ezpz --max (-l 10, 빠른 뇌)",
                    "ezpz -l 11": "ezpz -l 11 (뇌)"},
    },
}


def mb(b, digits):
    return f"{b / 1e6:.{digits}f}"


def max_rows(lang, names="names"):
    """Rows of the brain codecs vs xz/7z table: sizes in MB, and each ezpz size compared
    with the smaller of xz and 7z."""
    s = L[lang]
    out = []
    for ds in DATASETS:
        rival_sizes = [(rows[(ds, key)]["size"], short) for key, short in RIVALS]
        brain_sizes = [rows[(ds, key)]["size"] for key, _ in BRAIN]
        best_size, best_name = min(rival_sizes)
        smallest = min(brain_sizes + [best_size])
        cell = lambda b: (f"**{mb(b, 1)}{s['mb']}**" if b == smallest else f"{mb(b, 1)}{s['mb']}")
        vs = lambda b: s["vs"](abs(b / best_size - 1) * 100, best_name, b < best_size)
        label = s[names][ds] if names == "readme_names" else s[names][ds][0]
        out.append(f"| {label} | {mb(RAW[ds], 1)}{s['mb']} | "
                   + " | ".join(cell(size) for size, _ in rival_sizes) + " | "
                   + " | ".join(f"{cell(b)} ({vs(b)})" for b in brain_sizes) + " |")
    return out


def render(lang):
    s = L[lang]
    name = lambda m: s["methods"].get(m, m)
    t = lambda x: s["missing"] if x is None else s["sec"](x)
    best = {ds: min(rows[(ds, m)]["size"] for m in METHODS if (ds, m) in rows) for ds in DATASETS}
    is_best = lambda ds, r: r["size"] == best[ds]

    out = [s["title"], "", s["switch"], "", s["intro"], "", s["setup_h"], ""]
    out += s["setup"] + [""]

    out += [s["summary_h"], ""]
    out.append(f"| {s['method']} | " + " | ".join(s["names"][ds][0] for ds in DATASETS) + " |")
    out.append("|---" * (len(DATASETS) + 1) + "|")
    out.append(f"| {s['original_row']} | " + " | ".join(mb(RAW[ds], 1) for ds in DATASETS) + " |")
    for m in METHODS:
        cells = []
        for ds in DATASETS:
            r = rows.get((ds, m))
            if not r:
                cells.append(s["missing"])
                continue
            c = mb(r["size"], 1)
            cells.append(f"**{c}**{s['best_mark']}" if is_best(ds, r) else c)
        out.append(f"| {name(m)} | " + " | ".join(cells) + " |")
    out.append("")

    out += [s["max_h"], "", s["max_cols"], "|---|---|---|---|---|---|"]
    out += max_rows(lang)
    out += ["", s["max_note"], ""]

    for ds in DATASETS:
        title, desc = s["names"][ds]
        out += [f"## {title}", "", desc, "", s["table_cols"], "|---|---|---|---|---|---|---|"]
        for m in METHODS:
            r = rows.get((ds, m))
            if not r:
                continue
            after, red = f"{mb(r['size'], 2)}{s['mb']}", s["reduction"]((1 - r["size"] / RAW[ds]) * 100)
            if is_best(ds, r):
                after, red = f"**{after}**", f"**{red}**"
            out.append(f"| {name(m)} | {mb(RAW[ds], 2)} → {after} | {red} | {t(r['c_sec'])} | {t(r['x_sec'])} | "
                       f"{t(r['rand_sec'])} | {r['c_mem']} / {r['x_mem']} MB |")
        out.append("")

    out += [s["small_h"], ""] + s["small_intro"] + [""]
    out.append(s["small_cols"] + "".join(f" {m.replace('ezpz (default -l 7)', 'ezpz -l 7').replace('ezpz --max (-l 10)', 'ezpz --max')} |" for m in SMALL_METHODS))
    out.append("|---" * (len(SMALL_METHODS) + 2) + "|")
    size = {(r["file"], r["method"]): r["size"] for r in SMALL}
    raw = {r["file"]: r["raw"] for r in SMALL}
    kb = lambda b: f"{b / 1000:.1f}{s['kb']}"
    for f in SMALL_FILES:
        best = min(size[(f, m)] for m in SMALL_METHODS)
        cells = [f"**{kb(size[(f, m)])}**" if size[(f, m)] == best else kb(size[(f, m)]) for m in SMALL_METHODS]
        out.append(f"| {s['small_names'][f]} | {kb(raw[f])} | " + " | ".join(cells) + " |")
    tot = {m: sum(size[(f, m)] for f in SMALL_FILES) for m in SMALL_METHODS}
    best = min(tot.values())
    out.append(f"| {s['small_total']} | {kb(sum(raw.values()))} | "
               + " | ".join(f"**{kb(v)}**" if v == best else kb(v) for v in tot.values()) + " |")
    out += ["", s["small_note"](tot["ezpz --max (-l 10)"], tot["xz -9e"], tot["zip -9"], tot["ezpz (default -l 7)"]), ""]

    out += [s["discussion_h"], ""]
    out += open(os.path.join(HERE, s["notes"]), encoding="utf-8").read().strip().splitlines()
    text = "\n".join(out) + "\n"
    open(os.path.join(ROOT, s["file"]), "w", encoding="utf-8").write(text)
    return text


if __name__ == "__main__":
    import sys
    if "--readme-rows" in sys.argv:  # rows for the results table in README.md / README.ko.md
        for lang in ("en", "ko"):
            print("\n".join(max_rows(lang, "readme_names")), end="\n\n")
        sys.exit()
    for lang in ("en", "ko"):
        render(lang)
        print(f"wrote {L[lang]['file']}")
