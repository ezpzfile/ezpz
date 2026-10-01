# .ezpz benchmark

English | [한국어](BENCHMARK.ko.md)

ezpz compared with zip, tar.gz, tar.zst, tar.xz, and 7z on the same data.

## Test setup

- A 2-core Intel Xeon 2.1 GHz virtual server (on the slow side), Linux. Every tool that supports threads ran with 2 threads (`zstd -T2`, `xz -T2`, `7z -mmt=2`, `ezpz -j2`).
- Versions: zstd 1.5.5, XZ Utils 5.4.5, 7-Zip 23.01, Info-ZIP 3.0, gzip 1.12, GNU tar 1.35, ezpz 0.1.0 (0.2.0 for the `--max` rows; levels 1 to 9 and 11 produce the same data in both)
- Each measurement was taken once on an otherwise idle server, so expect a few percent of noise in the timings.
- Sizes read original → compressed, in MB (1 MB = 1,000,000 bytes). The compressed size is the whole archive file, index included. "80% smaller" means the archive is one fifth the size of the original.
- Single file is the time to extract one small file (for example `json/decoder.py`) from the archive. The tar formats have to be decompressed from the start to reach it.
- Peak RAM is the most memory (RAM) the tool used while compressing / extracting. It is not a file size.
- Every ezpz extraction was compared byte for byte with the original.
- `--max` is level 10: the fast brain codec (codec 4) with 16 MiB blocks. `-l 11` is the full brain codec (codec 3) with 64 MiB blocks. The `-l 11` rows were measured with the previous build, where this setting was called `--max`; the compressed data is byte-for-byte the same.

## Summary: size after compression, in MB (smaller is better)

| Method | Wikipedia text | Silesia corpus | Program install folder | Multi-version backup | Executables |
|---|---|---|---|---|---|
| **Original (before compression)** | 100.0 | 211.9 | 53.2 | 158.2 | 104.9 |
| zip -9 | 36.4 | 67.6 | 16.0 | 52.0 | 39.2 |
| tar.gz -9 | 36.4 | 67.6 | 15.3 | 49.6 | 39.1 |
| tar.zst -19 --long | 26.5 | 52.7 | 10.1 | 27.9 | 25.7 |
| tar.xz -9e | 24.8 | 48.4 | 9.1 | 24.3 | 23.1 |
| 7z -mx9 | 24.9 | 48.7 | 9.1 | 23.7 | **20.9** |
| ezpz -l 3 | 32.7 | 61.0 | 12.8 | 35.0 | 33.4 |
| ezpz (default -l 7) | 26.8 | 52.6 | 10.4 | 28.4 | 26.2 |
| ezpz -l 9 | 25.4 | 48.2 | 9.1 | 24.3 | 23.3 |
| ezpz --max (-l 10, fast brain) | 21.5 | 43.8 | 9.1 | 26.8 | 25.7 |
| ezpz -l 11 (brain) | **20.5** | **41.8** | **7.9** | **23.1** | 22.7 |

### The brain codecs against the best existing settings

| Dataset | Original | xz -9e | 7z -mx9 | ezpz --max | ezpz -l 11 |
|---|---|---|---|---|---|
| Wikipedia text | 100.0 MB | 24.8 MB | 24.9 MB | 21.5 MB (13.3% smaller than xz -9e) | **20.5 MB** (17.4% smaller than xz -9e) |
| Silesia corpus | 211.9 MB | 48.4 MB | 48.7 MB | 43.8 MB (9.6% smaller than xz -9e) | **41.8 MB** (13.6% smaller than xz -9e) |
| Program install folder | 53.2 MB | 9.1 MB | 9.1 MB | 9.1 MB (0.3% larger than 7z -mx9) | **7.9 MB** (12.5% smaller than 7z -mx9) |
| Multi-version backup | 158.2 MB | 24.3 MB | 23.7 MB | 26.8 MB (13.2% larger than 7z -mx9) | **23.1 MB** (2.4% smaller than 7z -mx9) |
| Executables | 104.9 MB | 23.1 MB | **20.9 MB** | 25.7 MB (22.9% larger than 7z -mx9) | 22.7 MB (8.7% larger than 7z -mx9) |

In parentheses: each ezpz size compared with whichever of xz and 7z made the smaller file.

## Wikipedia text

enwik8: the first 100 MB of an English Wikipedia XML dump (1 file)

| Method | Original → compressed | Size reduction | Compress time | Extract time | Single file | Peak RAM while compressing / extracting |
|---|---|---|---|---|---|---|
| zip -9 | 100.00 → 36.45 MB | 63.6% smaller | 7.0 s | 0.88 s | n/a | 11 / 11 MB |
| tar.gz -9 | 100.00 → 36.45 MB | 63.6% smaller | 6.9 s | 0.80 s | n/a | 11 / 11 MB |
| tar.zst -19 --long | 100.00 → 26.50 MB | 73.5% smaller | 71.1 s | 0.26 s | n/a | 294 / 99 MB |
| tar.xz -9e | 100.00 → 24.83 MB | 75.2% smaller | 150 s | 1.8 s | n/a | 793 / 185 MB |
| 7z -mx9 | 100.00 → 24.86 MB | 75.1% smaller | 105 s | 1.7 s | n/a | 682 / 124 MB |
| ezpz -l 3 | 100.00 → 32.74 MB | 67.3% smaller | 1.2 s | 0.18 s | n/a | 74 / 74 MB |
| ezpz (default -l 7) | 100.00 → 26.84 MB | 73.2% smaller | 69.0 s | 0.26 s | n/a | 310 / 109 MB |
| ezpz -l 9 | 100.00 → 25.43 MB | 74.6% smaller | 188 s | 1.1 s | n/a | 1190 / 179 MB |
| ezpz --max (-l 10, fast brain) | 100.00 → 21.54 MB | 78.5% smaller | 59.8 s | 63.7 s | n/a | 474 / 484 MB |
| ezpz -l 11 (brain) | 100.00 → **20.51 MB** | **79.5% smaller** | 112 s | 118 s | n/a | 714 / 687 MB |

## Silesia corpus

Silesia corpus: a standard set used in compression research (212 MB, 12 files: literature, databases, medical images, executables, XML, and more)

| Method | Original → compressed | Size reduction | Compress time | Extract time | Single file | Peak RAM while compressing / extracting |
|---|---|---|---|---|---|---|
| zip -9 | 211.94 → 67.63 MB | 68.1% smaller | 22.4 s | 1.6 s | 0.03 s | 11 / 11 MB |
| tar.gz -9 | 211.94 → 67.65 MB | 68.1% smaller | 23.6 s | 1.7 s | 1.6 s | 11 / 11 MB |
| tar.zst -19 --long | 211.94 → 52.74 MB | 75.1% smaller | 113 s | 0.48 s | 0.53 s | 415 / 132 MB |
| tar.xz -9e | 211.94 → 48.44 MB | 77.1% smaller | 191 s | 3.8 s | 4.3 s | 1078 / 323 MB |
| 7z -mx9 | 211.94 → 48.72 MB | 77.0% smaller | 98.5 s | 3.7 s | 3.5 s | 685 / 254 MB |
| ezpz -l 3 | 211.94 → 60.95 MB | 71.2% smaller | 2.2 s | 0.44 s | 0.02 s | 139 / 92 MB |
| ezpz (default -l 7) | 211.94 → 52.59 MB | 75.2% smaller | 77.6 s | 0.43 s | 0.10 s | 440 / 228 MB |
| ezpz -l 9 | 211.94 → 48.16 MB | 77.3% smaller | 221 s | 2.3 s | 1.0 s | 1597 / 301 MB |
| ezpz --max (-l 10, fast brain) | 211.94 → 43.77 MB | 79.3% smaller | 118 s | 123 s | 30.7 s | 556 / 532 MB |
| ezpz -l 11 (brain) | 211.94 → **41.83 MB** | **80.3% smaller** | 224 s | 222 s | 112 s | 901 / 793 MB |

## Program install folder

Python 3.12 install folder (53 MB, 1,212 files: .py sources, compiled .pyc files, libraries)

| Method | Original → compressed | Size reduction | Compress time | Extract time | Single file | Peak RAM while compressing / extracting |
|---|---|---|---|---|---|---|
| zip -9 | 53.23 → 16.04 MB | 69.9% smaller | 14.4 s | 0.49 s | 0.01 s | 10 / 11 MB |
| tar.gz -9 | 53.23 → 15.25 MB | 71.3% smaller | 15.1 s | 0.39 s | 0.38 s | 11 / 11 MB |
| tar.zst -19 --long | 53.23 → 10.10 MB | 81.0% smaller | 26.8 s | 0.17 s | 0.11 s | 154 / 56 MB |
| tar.xz -9e | 53.23 → 9.12 MB | 82.9% smaller | 40.7 s | 0.73 s | 0.83 s | 592 / 114 MB |
| 7z -mx9 | 53.23 → 9.05 MB | 83.0% smaller | 18.8 s | 0.90 s | 0.53 s | 504 / 61 MB |
| ezpz -l 3 | 53.23 → 12.78 MB | 76.0% smaller | 1.1 s | 0.26 s | 0.02 s | 64 / 61 MB |
| ezpz (default -l 7) | 53.23 → 10.37 MB | 80.5% smaller | 20.5 s | 0.37 s | 0.09 s | 256 / 61 MB |
| ezpz -l 9 | 53.23 → 9.07 MB | 83.0% smaller | 44.6 s | 0.90 s | 0.50 s | 1189 / 109 MB |
| ezpz --max (-l 10, fast brain) | 53.23 → 9.08 MB | 82.9% smaller | 36.8 s | 33.4 s | 14.6 s | 426 / 411 MB |
| ezpz -l 11 (brain) | 53.23 → **7.92 MB** | **85.1% smaller** | 78.0 s | 75.0 s | 75.8 s | 379 / 377 MB |

## Multi-version backup

The Python 3.11, 3.12, and 3.13 install folders together (158 MB, 3,750 files), standing in for an archive that keeps several versions of one project

| Method | Original → compressed | Size reduction | Compress time | Extract time | Single file | Peak RAM while compressing / extracting |
|---|---|---|---|---|---|---|
| zip -9 | 158.18 → 52.05 MB | 67.1% smaller | 41.9 s | 1.2 s | 0.01 s | 11 / 11 MB |
| tar.gz -9 | 158.18 → 49.59 MB | 68.6% smaller | 44.7 s | 1.4 s | 1.1 s | 11 / 11 MB |
| tar.zst -19 --long | 158.18 → 27.91 MB | 82.4% smaller | 54.3 s | 1.3 s | 0.34 s | 352 / 132 MB |
| tar.xz -9e | 158.18 → 24.28 MB | 84.6% smaller | 134 s | 2.1 s | 2.2 s | 852 / 243 MB |
| 7z -mx9 | 158.18 → 23.68 MB | 85.0% smaller | 71.8 s | 3.0 s | 1.6 s | 689 / 168 MB |
| ezpz -l 3 | 158.18 → 35.03 MB | 77.9% smaller | 2.4 s | 1.9 s | 0.02 s | 96 / 82 MB |
| ezpz (default -l 7) | 158.18 → 28.41 MB | 82.0% smaller | 43.7 s | 0.46 s | 0.07 s | 363 / 155 MB |
| ezpz -l 9 | 158.18 → 24.26 MB | 84.7% smaller | 126 s | 1.9 s | 0.53 s | 1522 / 212 MB |
| ezpz --max (-l 10, fast brain) | 158.18 → 26.80 MB | 83.1% smaller | 74.6 s | 71.4 s | 14.9 s | 502 / 509 MB |
| ezpz -l 11 (brain) | 158.18 → **23.12 MB** | **85.4% smaller** | 128 s | 127 s | 101 s | 831 / 812 MB |

## Executables

231 executables from Linux /usr/bin (105 MB, x86-64)

| Method | Original → compressed | Size reduction | Compress time | Extract time | Single file | Peak RAM while compressing / extracting |
|---|---|---|---|---|---|---|
| zip -9 | 104.86 → 39.22 MB | 62.6% smaller | 19.1 s | 0.88 s | 0.01 s | 11 / 11 MB |
| tar.gz -9 | 104.86 → 39.15 MB | 62.7% smaller | 18.5 s | 1.0 s | 0.91 s | 11 / 11 MB |
| tar.zst -19 --long | 104.86 → 25.74 MB | 75.5% smaller | 40.8 s | 0.32 s | 0.28 s | 299 / 104 MB |
| tar.xz -9e | 104.86 → 23.10 MB | 78.0% smaller | 91.6 s | 1.9 s | 2.0 s | 797 / 188 MB |
| 7z -mx9 | 104.86 → **20.91 MB** | **80.1% smaller** | 41.2 s | 1.6 s | 0.02 s | 706 / 121 MB |
| ezpz -l 3 | 104.86 → 33.40 MB | 68.1% smaller | 1.4 s | 0.44 s | 0.03 s | 120 / 123 MB |
| ezpz (default -l 7) | 104.86 → 26.16 MB | 75.1% smaller | 41.5 s | 0.39 s | 0.10 s | 313 / 168 MB |
| ezpz -l 9 | 104.86 → 23.28 MB | 77.8% smaller | 105 s | 1.0 s | 0.74 s | 1470 / 181 MB |
| ezpz --max (-l 10, fast brain) | 104.86 → 25.69 MB | 75.5% smaller | 58.6 s | 61.5 s | 18.1 s | 488 / 512 MB |
| ezpz -l 11 (brain) | 104.86 → 22.72 MB | 78.3% smaller | 139 s | 114 s | 116 s | 730 / 705 MB |

## Discussion

### Strengths

- `-l 11` (the full brain codec with 64 MiB blocks) produced the smallest archive on 4 of the 5 datasets. On the Wikipedia text, the Silesia corpus, and the Python install folder it is 12 to 18% smaller than the best xz and 7z settings, and on the multi-version backup it is 2 to 5% smaller.
- `--max` (the fast brain codec with 16 MiB blocks) compresses and extracts 1.7 to 2.4 times as fast as `-l 11` on this server, and pulls a single file out 3.6 to 6.8 times as fast (15 to 31 s instead of 76 to 117 s). On a 4-core Mac the gap is about 4 times: 48 MB of text took 9 s instead of 38 s to compress and 10 s instead of 41 s to extract. On text it stays well ahead of xz and 7z (13.3% smaller on the Wikipedia text, 9.6% on the Silesia corpus), and it compresses faster than xz -9e (60 s vs 150 s on the Wikipedia text).
- The default level (`-l 7`) comes out about the same size as tar.zst -19 --long (within ±2%, and smaller on the Silesia corpus). It compresses as fast or faster (78 s vs 113 s on Silesia), extracts in under half a second, and pulls a single file out in under 0.1 s, which tar.zst cannot do.
- `-l 3` is 10 to 33% smaller than zip and compresses 6 to 17 times faster (on the backup data, zip took 41.9 s and ezpz 2.4 s).
- `-l 9` is close to xz and 7z in size, and the smallest of the three on Silesia. It is faster at full extraction and at single-file extraction (one file from Silesia: 1.0 s, vs 4.3 s for xz and 3.5 s for 7z).
- Every extraction matched the original byte for byte.

### Weaknesses

- `--max` gives up a lot of size on program files and backups. On the Python folder, the backup, and the executables it is 13 to 16% larger than `-l 11`: about the same as 7z on the Python folder, and 13% larger than 7z on the backup. On these three datasets `-l 9` is as small or smaller and extracts much faster. The predictors that brain-fast leaves out matter most for code and compiled files, and 16 MiB blocks capture less of the similarity between files than 64 MiB blocks. For the smallest archive of such data, use `-l 11`.
- Both brain codecs are slow to extract. Even `--max` runs at about 1.7 MB/s on this server (about 5 MB/s on a 4-core Mac), tens of times slower than xz. They suit long-term storage better than files you open often.
- On executables 7z is 8.7% smaller than even `-l 11`. 7z has an advanced x86 preprocessor (BCJ2), while the x86 transform in ezpz is still a simple one. This is the next thing to improve.
- Memory use is high. `-l 9` tries both zstd 22 and LZMA2 9e on every block and needs 1.2 to 1.6 GB while compressing. With 2 threads the brain codecs use 0.4 to 0.6 GB (`--max`) and 0.4 to 0.9 GB (`-l 11`).
- The default level is up to 2% larger than tar.zst -19. Splitting the data into blocks costs that much, and it is what makes parallel work and single-file extraction possible.
- Every figure comes from a single run on a slow virtual server. An Apple Silicon machine will run all of these tools faster.

### Next steps

- A better choice of predictors for brain-fast, so that `--max` loses less on program files
- Executables: a BCJ2-class x86 preprocessor, or a predictor in the brain codec that models machine code
- Built-in knowledge for the brain codec (starting from a pretrained state), which helps small files most
