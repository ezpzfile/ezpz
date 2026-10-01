#!/usr/bin/env python3
"""Benchmark .ezpz against zip / tar.gz / tar.zst / tar.xz / 7z.
Measures archive size, compress time, full-extract time, single-file extract time, peak memory.

Setup used for BENCHMARK.md (put the datasets under bench/data/):
  data/text/enwik8                  https://mattmahoney.net/dc/enwik8.zip
  data/silesia/*                    https://sun.aei.polsl.pl/~sdeor/corpus/silesia.zip
  data/source/python3.12            a copy of /usr/lib/python3.12
  data/backups/python3.11,12,13     copies of /usr/lib/python3.11, 3.12, 3.13
  data/binaries/bin                 ~100 MB of regular files from /usr/bin
Run:  EZPZ=../target/release/ezpz python3 bench.py [datasets...]
Env:  ONLY_EZ=1 (only ezpz rows), ONLY=<substring of method name>, RESULTS=<file>"""
import json, os, shutil, subprocess, sys, time

ROOT = os.path.dirname(os.path.abspath(__file__))
DATA = os.path.join(ROOT, "data")
OUT = os.path.join(ROOT, "out")
TMP = os.path.join(ROOT, "tmp")
EZ = os.path.abspath(os.environ.get("EZPZ", os.path.join(ROOT, "..", "target", "release", "ezpz")))
J = "2"

DATASETS = {
    "source": "source/python3.12/json/decoder.py",
    "backups": "backups/python3.13/json/decoder.py",
    "binaries": None,  # filled below
    "silesia": "silesia/xml",
    "text": None,
}
bins = sorted(os.listdir(os.path.join(DATA, "binaries/bin")))
DATASETS["binaries"] = "binaries/bin/" + bins[-1]

def methods(ds):
    a = lambda ext: os.path.join(OUT, f"{ds}.{ext}")
    return [
        ("zip -9", a("zip"), f"zip -9 -r -q {a('zip')} {ds}",
         f"unzip -q {a('zip')} -d {{dest}}", f"unzip -p {a('zip')} {{path}} > /dev/null"),
        ("tar.gz -9", a("tar.gz"), f"tar cf - {ds} | gzip -9 > {a('tar.gz')}",
         f"tar xzf {a('tar.gz')} -C {{dest}}", f"tar xzOf {a('tar.gz')} {{path}} > /dev/null"),
        ("tar.zst -19 --long", a("tar.zst"), f"tar cf - {ds} | zstd -q -19 --long=27 -T{J} -o {a('tar.zst')}",
         f"zstd -q -d --long=27 -c {a('tar.zst')} | tar xf - -C {{dest}}",
         f"zstd -q -d --long=27 -c {a('tar.zst')} | tar xOf - {{path}} > /dev/null"),
        ("tar.xz -9e", a("tar.xz"), f"tar cf - {ds} | xz -9e -T{J} -c > {a('tar.xz')}",
         f"xz -d -T{J} -c {a('tar.xz')} | tar xf - -C {{dest}}",
         f"xz -d -c {a('tar.xz')} | tar xOf - {{path}} > /dev/null"),
        ("7z -mx9", a("7z"), f"7z a -bd -bso0 -mx=9 -mmt={J} {a('7z')} {ds}",
         f"7z x -bd -bso0 -mmt={J} -o{{dest}} {a('7z')}", f"7z e -bd -so {a('7z')} {{path}} > /dev/null"),
        ("ezpz -l 3", a("l3.ezpz"), f"{EZ} c {a('l3.ezpz')} {ds} -l 3 -j{J}",
         f"{EZ} x {a('l3.ezpz')} -C {{dest}} -j{J} --unsafe-links", f"{EZ} cat {a('l3.ezpz')} {{path}} > /dev/null"),
        ("ezpz (기본 -l 7)", a("l7.ezpz"), f"{EZ} c {a('l7.ezpz')} {ds} -l 7 -j{J}",
         f"{EZ} x {a('l7.ezpz')} -C {{dest}} -j{J} --unsafe-links", f"{EZ} cat {a('l7.ezpz')} {{path}} > /dev/null"),
        ("ezpz -l 9", a("l9.ezpz"), f"{EZ} c {a('l9.ezpz')} {ds} -l 9 -j{J}",
         f"{EZ} x {a('l9.ezpz')} -C {{dest}} -j{J} --unsafe-links", f"{EZ} cat {a('l9.ezpz')} {{path}} > /dev/null"),
        ("ezpz --max (뇌)", a("max.ezpz"), f"{EZ} c {a('max.ezpz')} {ds} --max -j{J}",
         f"{EZ} x {a('max.ezpz')} -C {{dest}} -j{J} --unsafe-links", f"{EZ} cat {a('max.ezpz')} {{path}} > /dev/null"),
    ]

def run(cmd, cwd):
    t = time.perf_counter()
    p = subprocess.Popen(["bash", "-o", "pipefail", "-c", cmd], cwd=cwd,
                         stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
    _, status, ru = os.wait4(p.pid, 0)
    dt = time.perf_counter() - t
    err = p.stderr.read().decode(errors="replace")
    p.returncode = os.waitstatus_to_exitcode(status)
    if p.returncode != 0:
        raise RuntimeError(f"command failed ({p.returncode}): {cmd}\n{err}")
    return dt, ru.ru_maxrss // 1024  # MiB

def dir_size(path):
    t = 0
    for r, _, fs in os.walk(path):
        for f in fs:
            fp = os.path.join(r, f)
            if not os.path.islink(fp):
                t += os.path.getsize(fp)
    return t

def main():
    only = sys.argv[1:]
    os.makedirs(OUT, exist_ok=True)
    res_path = os.path.join(ROOT, os.environ.get("RESULTS", "results.jsonl"))
    for ds, rnd in DATASETS.items():
        if only and ds not in only:
            continue
        raw = dir_size(os.path.join(DATA, ds))
        for name, arch, c, x, r in methods(ds):
            if os.environ.get("ONLY_EZ") and "ezpz" not in name:
                continue
            if os.environ.get("ONLY") and os.environ["ONLY"] not in name:
                continue
            if os.path.exists(arch):
                os.remove(arch)
            ct, cm = run(c, DATA)
            size = os.path.getsize(arch)
            dest = os.path.join(TMP, "x")
            shutil.rmtree(TMP, ignore_errors=True)
            os.makedirs(dest)
            xt, xm = run(x.format(dest=dest), DATA)
            ok = None
            if "ezpz" in name:
                ok = subprocess.run(["diff", "-r", "--no-dereference", os.path.join(DATA, ds), os.path.join(dest, ds)],
                                    capture_output=True).returncode == 0
            shutil.rmtree(TMP, ignore_errors=True)
            rt = None
            if rnd:
                rt, _ = run(r.format(path=rnd), DATA)
            rec = dict(dataset=ds, method=name, raw=raw, size=size, ratio=size / raw, c_sec=ct, x_sec=xt,
                       rand_sec=rt, c_mem=cm, x_mem=xm, roundtrip_ok=ok)
            print(json.dumps(rec, ensure_ascii=False), flush=True)
            with open(res_path, "a") as f:
                f.write(json.dumps(rec, ensure_ascii=False) + "\n")

if __name__ == "__main__":
    main()
