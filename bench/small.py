#!/usr/bin/env python3
"""Small-file benchmark: compress each file on its own and record the output size.

Files used for BENCHMARK.md (put them in bench/small/):
  c_header.h      /usr/include/stdio.h (glibc)
  data.json       first 24,000 bytes of iso_3166-1.json (Debian iso-codes)
  data.tab        zone1970.tab (tzdata)
  data.xml        24,000 bytes of Silesia "xml" from offset 100,000
  en_license.txt  the Apache License 2.0 text
  en_novel.txt    32,000 bytes of Silesia "dickens" from offset 1,000,000
  en_page.html    TeX Live readme.en.html
  es_tutor.txt    12,000 bytes of the Spanish Vim tutor (tutor.es.utf-8) from offset 8,000
  ja_page.html    TeX Live readme.ja.html
  ja_tutor.txt    16,000 bytes of the Japanese Vim tutor (tutor.ja.utf-8) from offset 8,000
  ko_readme.md    README.ko.md of this repository (version 0.2.0)
  ko_spec.md      24,000 bytes of SPEC.ko.md (version 0.2.0) from offset 20,000
  ko_tutor.txt    16,000 bytes of the Korean Vim tutor (tutor.ko.utf-8) from offset 8,000
  py_module.py    textwrap.py from Python 3.12
  wiki.xml        16,000 bytes of enwik8 from offset 60,000,000
(Slices that would cut a UTF-8 character are shortened to the last whole character.)
Run:  EZPZ=../target/release/ezpz python3 small.py   -> results_small.jsonl"""
import json, os, shutil, subprocess, tempfile

ROOT = os.path.dirname(os.path.abspath(__file__))
SRC = os.path.join(ROOT, "small")
EZ = os.path.abspath(os.environ.get("EZPZ", os.path.join(ROOT, "..", "target", "release", "ezpz")))
METHODS = [
    ("zip -9", "out.zip", "zip -9 -q out.zip {f}"),
    ("gzip -9", "out.gz", "gzip -9 -c {f} > out.gz"),
    ("zstd -19", "out.zst", "zstd -19 -q -c {f} > out.zst"),
    ("xz -9e", "out.xz", "xz -9e -c {f} > out.xz"),
    ("7z -mx9", "out.7z", "7z a -bd -bso0 -mx=9 out.7z {f}"),
    ("ezpz (default -l 7)", "out.ezpz", EZ + " c out.ezpz {f}"),
    ("ezpz --max (-l 10)", "out.ezpz", EZ + " c out.ezpz {f} --max"),
    ("ezpz -l 11", "out.ezpz", EZ + " c out.ezpz {f} -l 11"),
]


def main():
    out = open(os.path.join(ROOT, "results_small.jsonl"), "w")
    for name in sorted(os.listdir(SRC)):
        raw = os.path.getsize(os.path.join(SRC, name))
        for method, arch, cmd in METHODS:
            with tempfile.TemporaryDirectory() as tmp:
                shutil.copy2(os.path.join(SRC, name), tmp)
                subprocess.run(cmd.format(f=name), shell=True, cwd=tmp, check=True,
                               stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
                size = os.path.getsize(os.path.join(tmp, arch))
            rec = dict(file=name, raw=raw, method=method, size=size)
            out.write(json.dumps(rec) + "\n")
            print(json.dumps(rec), flush=True)


if __name__ == "__main__":
    main()
