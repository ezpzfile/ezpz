#!/usr/bin/env python3
"""Merge benchmark results and write BENCHMARK.md (Korean)."""
import json, os, sys
HERE = os.path.dirname(os.path.abspath(__file__))

v1 = [json.loads(l) for l in open(os.path.join(HERE, "results_v1.jsonl"))]
v2 = [json.loads(l) for l in open(os.path.join(HERE, "results_v2.jsonl"))]
rows = {}
for r in v1:
    if "ezpz" not in r["method"]:
        rows[(r["dataset"], r["method"])] = r
for r in v2:  # later rows win
    rows[(r["dataset"], r["method"])] = r

DS = [
    ("text", "위키백과 텍스트", "enwik8: 영어 위키백과 XML 덤프의 앞 100MB (파일 1개)"),
    ("silesia", "표준 테스트 세트", "Silesia corpus: 압축 연구에서 쓰는 표준 세트 (212MB, 파일 12개: 문학 텍스트, 데이터베이스, 의료 영상, 실행 파일, XML 등)"),
    ("source", "프로그램 설치 폴더", "Python 3.12 설치 폴더 (53MB, 파일 1,212개: .py 소스, 컴파일된 .pyc, 라이브러리)"),
    ("backups", "여러 버전 백업", "Python 3.11 · 3.12 · 3.13 설치 폴더 3개를 함께 (158MB, 파일 3,750개). 같은 프로젝트의 여러 버전을 보관하는 상황이에요"),
    ("binaries", "실행 파일", "리눅스 /usr/bin의 실행 파일 231개 (105MB, x86-64)"),
]
METHODS = ["zip -9", "tar.gz -9", "tar.zst -19 --long", "tar.xz -9e", "7z -mx9",
           "ezpz -l 3", "ezpz (기본 -l 7)", "ezpz -l 9", "ezpz --max (뇌)"]

def mb(n):
    return f"{n / 1e6:.2f}"

def t(x):
    if x is None:
        return "–"
    if x < 1:
        return f"{x:.2f}초"
    if x < 100:
        return f"{x:.1f}초"
    return f"{x:.0f}초"

out = []
w = out.append
w("# .ezpz 벤치마크")
w("")
w("zip, tar.gz, tar.zst, tar.xz, 7z와 같은 데이터를 압축해서 비교했어요.")
w("")
w("## 측정 환경")
w("")
w("- 2코어 Intel Xeon 2.1GHz 가상 서버(느린 편), 리눅스. 스레드를 지원하는 도구는 모두 2스레드로 실행했어요 (`zstd -T2`, `xz -T2`, `7z -mmt=2`, `ezpz -j2`).")
w("- 도구 버전: zstd 1.5.5, XZ Utils 5.4.5, 7-Zip 23.01, Info-ZIP 3.0, gzip 1.12, GNU tar 1.35, ezpz 0.1.0")
w("- 각 항목은 한 번씩 측정했어요. 시간은 같은 서버에서 다른 작업이 거의 없을 때 잰 값이지만 몇 % 정도의 오차는 있을 수 있어요.")
w("- **크기**는 결과 파일 전체 크기(인덱스·목록 포함)입니다. **비율**은 원본 대비 %라서 작을수록 좋아요.")
w("- **파일 하나 꺼내기**: 아카이브 속 작은 파일 1개(예: `json/decoder.py`)를 꺼내는 데 걸린 시간. tar 계열은 구조상 앞에서부터 풀어야 해요.")
w("- **메모리**: 압축 / 해제할 때 최대 사용량.")
w("- ezpz 해제 결과는 매번 원본과 바이트 단위로 비교해 같은지 확인했어요.")
w("")

# summary table: ratio per dataset
w("## 한눈에: 크기 비교 (원본 대비 %, 작을수록 좋음)")
w("")
hdr = "| 방식 | " + " | ".join(d[1] for d in DS) + " |"
w(hdr)
w("|---" * (len(DS) + 1) + "|")
best = {}
for ds, _, _ in DS:
    best[ds] = min(rows[(ds, m)]["ratio"] for m in METHODS if (ds, m) in rows)
for m in METHODS:
    cells = []
    for ds, _, _ in DS:
        r = rows.get((ds, m))
        if not r:
            cells.append("–")
            continue
        s = f"{r['ratio']*100:.1f}%"
        if abs(r["ratio"] - best[ds]) < 1e-12:
            s = f"**{s}** 🥇"
        cells.append(s)
    w(f"| {m} | " + " | ".join(cells) + " |")
w("")

# relative to xz and 7z for max
w("### 뇌 코덱(`--max`)을 기존 최고 압축과 비교하면")
w("")
w("| 데이터 | xz -9e 대비 | 7z -mx9 대비 |")
w("|---|---|---|")
for ds, name, _ in DS:
    mx = rows[(ds, "ezpz --max (뇌)")]["size"]
    xz = rows[(ds, "tar.xz -9e")]["size"]
    sz = rows[(ds, "7z -mx9")]["size"]
    f = lambda a, b: f"{(a/b-1)*100:+.1f}%"
    w(f"| {name} | {f(mx, xz)} | {f(mx, sz)} |")
w("")
w("음수면 뇌 코덱이 그만큼 더 작다는 뜻이에요.")
w("")

for ds, name, desc in DS:
    w(f"## {name}")
    w("")
    w(desc)
    w("")
    w("| 방식 | 크기(MB) | 비율 | 압축 | 해제 | 파일 하나 꺼내기 | 메모리 (압축/해제) |")
    w("|---|---|---|---|---|---|---|")
    for m in METHODS:
        r = rows.get((ds, m))
        if not r:
            continue
        size = mb(r["size"])
        ratio = f"{r['ratio']*100:.2f}%"
        if abs(r["ratio"] - best[ds]) < 1e-12:
            size, ratio = f"**{size}**", f"**{ratio}**"
        w(f"| {m} | {size} | {ratio} | {t(r['c_sec'])} | {t(r['x_sec'])} | {t(r['rand_sec'])} | {r['c_mem']} / {r['x_mem']} MB |")
    w("")

w("## 해석")
w("")
w("**잘한 점**")
w("")
for line in sys.stdin.read().strip().splitlines():
    w(line)
open(os.path.join(HERE, "..", "BENCHMARK.md"), "w").write("\n".join(out) + "\n")
print("\n".join(out))
