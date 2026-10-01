# .ezpz benchmark

English | [한국어](BENCHMARK.ko.md)

ezpz compared with zip, tar.gz, tar.zst, tar.xz, and 7z on the same data.

## Test setup

- A 2-core Intel Xeon 2.1 GHz virtual server (on the slow side), Linux. Every tool that supports threads ran with 2 threads (`zstd -T2`, `xz -T2`, `7z -mmt=2`, `ezpz -j2`).
- Versions: zstd 1.5.5, XZ Utils 5.4.5, 7-Zip 23.01, Info-ZIP 3.0, gzip 1.12, GNU tar 1.35, ezpz 0.1.0
- Each measurement was taken once on an otherwise idle server, so expect a few percent of noise in the timings.
- Size is the size of the whole output file, index included. Ratio is the output size as a percentage of the input, so lower is better.
- Single file is the time to extract one small file (for example `json/decoder.py`) from the archive. The tar formats have to be decompressed from the start to reach it.
- Memory is the peak memory use while compressing / extracting.
- Every ezpz extraction was compared byte for byte with the original.

## Summary: size (% of original, lower is better)

| Method | Wikipedia text | Silesia corpus | Program install folder | Multi-version backup | Executables |
|---|---|---|---|---|---|
| zip -9 | 36.4% | 31.9% | 30.1% | 32.9% | 37.4% |
| tar.gz -9 | 36.4% | 31.9% | 28.7% | 31.4% | 37.3% |
| tar.zst -19 --long | 26.5% | 24.9% | 19.0% | 17.6% | 24.5% |
| tar.xz -9e | 24.8% | 22.9% | 17.1% | 15.4% | 22.0% |
| 7z -mx9 | 24.9% | 23.0% | 17.0% | 15.0% | **19.9%** |
| ezpz -l 3 | 32.7% | 28.8% | 24.0% | 22.1% | 31.9% |
| ezpz (default -l 7) | 26.8% | 24.8% | 19.5% | 18.0% | 24.9% |
| ezpz -l 9 | 25.4% | 22.7% | 17.0% | 15.3% | 22.2% |
| ezpz --max (brain) | **20.5%** | **19.7%** | **14.9%** | **14.6%** | 21.7% |

### The brain codec (`--max`) against the best existing settings

| Dataset | vs xz -9e | vs 7z -mx9 |
|---|---|---|
| Wikipedia text | -17.4% | -17.5% |
| Silesia corpus | -13.6% | -14.1% |
| Program install folder | -13.1% | -12.5% |
| Multi-version backup | -4.8% | -2.4% |
| Executables | -1.6% | +8.7% |

Negative numbers mean the brain codec output is that much smaller.

## Wikipedia text

enwik8: the first 100 MB of an English Wikipedia XML dump (1 file)

| Method | Size (MB) | Ratio | Compress | Extract | Single file | Memory (compress / extract) |
|---|---|---|---|---|---|---|
| zip -9 | 36.45 | 36.45% | 7.0 s | 0.88 s | n/a | 11 / 11 MB |
| tar.gz -9 | 36.45 | 36.45% | 6.9 s | 0.80 s | n/a | 11 / 11 MB |
| tar.zst -19 --long | 26.50 | 26.50% | 71.1 s | 0.26 s | n/a | 294 / 99 MB |
| tar.xz -9e | 24.83 | 24.83% | 150 s | 1.8 s | n/a | 793 / 185 MB |
| 7z -mx9 | 24.86 | 24.86% | 105 s | 1.7 s | n/a | 682 / 124 MB |
| ezpz -l 3 | 32.74 | 32.74% | 1.2 s | 0.18 s | n/a | 74 / 74 MB |
| ezpz (default -l 7) | 26.84 | 26.84% | 69.0 s | 0.26 s | n/a | 310 / 109 MB |
| ezpz -l 9 | 25.43 | 25.43% | 188 s | 1.1 s | n/a | 1190 / 179 MB |
| ezpz --max (brain) | **20.51** | **20.51%** | 112 s | 118 s | n/a | 714 / 687 MB |

## Silesia corpus

Silesia corpus: a standard set used in compression research (212 MB, 12 files: literature, databases, medical images, executables, XML, and more)

| Method | Size (MB) | Ratio | Compress | Extract | Single file | Memory (compress / extract) |
|---|---|---|---|---|---|---|
| zip -9 | 67.63 | 31.91% | 22.4 s | 1.6 s | 0.03 s | 11 / 11 MB |
| tar.gz -9 | 67.65 | 31.92% | 23.6 s | 1.7 s | 1.6 s | 11 / 11 MB |
| tar.zst -19 --long | 52.74 | 24.88% | 113 s | 0.48 s | 0.53 s | 415 / 132 MB |
| tar.xz -9e | 48.44 | 22.86% | 191 s | 3.8 s | 4.3 s | 1078 / 323 MB |
| 7z -mx9 | 48.72 | 22.99% | 98.5 s | 3.7 s | 3.5 s | 685 / 254 MB |
| ezpz -l 3 | 60.95 | 28.76% | 2.2 s | 0.44 s | 0.02 s | 139 / 92 MB |
| ezpz (default -l 7) | 52.59 | 24.81% | 77.6 s | 0.43 s | 0.10 s | 440 / 228 MB |
| ezpz -l 9 | 48.16 | 22.73% | 221 s | 2.3 s | 1.0 s | 1597 / 301 MB |
| ezpz --max (brain) | **41.83** | **19.74%** | 224 s | 222 s | 112 s | 901 / 793 MB |

## Program install folder

Python 3.12 install folder (53 MB, 1,212 files: .py sources, compiled .pyc files, libraries)

| Method | Size (MB) | Ratio | Compress | Extract | Single file | Memory (compress / extract) |
|---|---|---|---|---|---|---|
| zip -9 | 16.04 | 30.14% | 14.4 s | 0.49 s | 0.01 s | 10 / 11 MB |
| tar.gz -9 | 15.25 | 28.66% | 15.1 s | 0.39 s | 0.38 s | 11 / 11 MB |
| tar.zst -19 --long | 10.10 | 18.98% | 26.8 s | 0.17 s | 0.11 s | 154 / 56 MB |
| tar.xz -9e | 9.12 | 17.14% | 40.7 s | 0.73 s | 0.83 s | 592 / 114 MB |
| 7z -mx9 | 9.05 | 17.01% | 18.8 s | 0.90 s | 0.53 s | 504 / 61 MB |
| ezpz -l 3 | 12.78 | 24.01% | 1.1 s | 0.26 s | 0.02 s | 64 / 61 MB |
| ezpz (default -l 7) | 10.37 | 19.48% | 20.5 s | 0.37 s | 0.09 s | 256 / 61 MB |
| ezpz -l 9 | 9.07 | 17.03% | 44.6 s | 0.90 s | 0.50 s | 1189 / 109 MB |
| ezpz --max (brain) | **7.92** | **14.89%** | 78.0 s | 75.0 s | 75.8 s | 379 / 377 MB |

## Multi-version backup

The Python 3.11, 3.12, and 3.13 install folders together (158 MB, 3,750 files), standing in for an archive that keeps several versions of one project

| Method | Size (MB) | Ratio | Compress | Extract | Single file | Memory (compress / extract) |
|---|---|---|---|---|---|---|
| zip -9 | 52.05 | 32.90% | 41.9 s | 1.2 s | 0.01 s | 11 / 11 MB |
| tar.gz -9 | 49.59 | 31.35% | 44.7 s | 1.4 s | 1.1 s | 11 / 11 MB |
| tar.zst -19 --long | 27.91 | 17.64% | 54.3 s | 1.3 s | 0.34 s | 352 / 132 MB |
| tar.xz -9e | 24.28 | 15.35% | 134 s | 2.1 s | 2.2 s | 852 / 243 MB |
| 7z -mx9 | 23.68 | 14.97% | 71.8 s | 3.0 s | 1.6 s | 689 / 168 MB |
| ezpz -l 3 | 35.03 | 22.14% | 2.4 s | 1.9 s | 0.02 s | 96 / 82 MB |
| ezpz (default -l 7) | 28.41 | 17.96% | 43.7 s | 0.46 s | 0.07 s | 363 / 155 MB |
| ezpz -l 9 | 24.26 | 15.34% | 126 s | 1.9 s | 0.53 s | 1522 / 212 MB |
| ezpz --max (brain) | **23.12** | **14.61%** | 128 s | 127 s | 101 s | 831 / 812 MB |

## Executables

231 executables from Linux /usr/bin (105 MB, x86-64)

| Method | Size (MB) | Ratio | Compress | Extract | Single file | Memory (compress / extract) |
|---|---|---|---|---|---|---|
| zip -9 | 39.22 | 37.40% | 19.1 s | 0.88 s | 0.01 s | 11 / 11 MB |
| tar.gz -9 | 39.15 | 37.34% | 18.5 s | 1.0 s | 0.91 s | 11 / 11 MB |
| tar.zst -19 --long | 25.74 | 24.55% | 40.8 s | 0.32 s | 0.28 s | 299 / 104 MB |
| tar.xz -9e | 23.10 | 22.03% | 91.6 s | 1.9 s | 2.0 s | 797 / 188 MB |
| 7z -mx9 | **20.91** | **19.94%** | 41.2 s | 1.6 s | 0.02 s | 706 / 121 MB |
| ezpz -l 3 | 33.40 | 31.86% | 1.4 s | 0.44 s | 0.03 s | 120 / 123 MB |
| ezpz (default -l 7) | 26.16 | 24.95% | 41.5 s | 0.39 s | 0.10 s | 313 / 168 MB |
| ezpz -l 9 | 23.28 | 22.20% | 105 s | 1.0 s | 0.74 s | 1470 / 181 MB |
| ezpz --max (brain) | 22.72 | 21.67% | 139 s | 114 s | 116 s | 730 / 705 MB |

## Discussion

### Strengths

- The brain codec (`--max`) produced the smallest archive on 4 of the 5 datasets. On the Wikipedia text, the Silesia corpus, and the Python install folder it is 12 to 18% smaller than the best xz and 7z settings, and on the multi-version backup it is 2 to 5% smaller.
- The default level (`-l 7`) comes out about the same size as tar.zst -19 --long (within ±2%, and smaller on the Silesia corpus). It compresses as fast or faster (78 s vs 113 s on Silesia), extracts in under half a second, and pulls a single file out in under 0.1 s, which tar.zst cannot do.
- `-l 3` is 10 to 33% smaller than zip and compresses 6 to 17 times faster (on the backup data, zip took 41.9 s and ezpz 2.4 s).
- `-l 9` is close to xz and 7z in size, and the smallest of the three on Silesia. It is faster at full extraction and at single-file extraction (one file from Silesia: 1.0 s, vs 4.3 s for xz and 3.5 s for 7z).
- Every extraction matched the original byte for byte.

### Weaknesses

- The brain codec is slow. On this 2-core server it compresses and decompresses at roughly 1 MB/s, tens of times slower than xz decompression. Extracting a single file means decoding the whole 64 MB block that holds it, which takes one to two minutes. That is why only `--max` uses it; it is meant for long-term storage.
- On executables 7z is 8.7% smaller. 7z has an advanced x86 preprocessor (BCJ2), while the x86 transform in ezpz is still a simple one. This is the next thing to improve.
- Memory use is high. `-l 9` tries both zstd 22 and LZMA2 9e on every block and needs 1.2 to 1.6 GB while compressing. The brain codec uses 0.4 to 0.9 GB with 2 threads.
- The default level is up to 2% larger than tar.zst -19. Splitting the data into blocks costs that much, and it is what makes parallel work and single-file extraction possible.
- Every figure comes from a single run on a slow virtual server. An Apple Silicon machine will run all of these tools faster.

### Next steps

- Brain codec speed: SIMD for the mixer arithmetic, a "fast brain" mode with fewer models, and a block size option
- Executables: a BCJ2-class x86 preprocessor, or a predictor in the brain codec that models machine code
- Built-in knowledge for the brain codec (starting from a pretrained state), which helps small files most
