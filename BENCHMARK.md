# .ezpz benchmark

English | [한국어](BENCHMARK.ko.md)

ezpz compared with zip, tar.gz, tar.zst, tar.xz, and 7z on the same data.

## Test setup

- A 2-core Intel Xeon 2.1 GHz virtual server (on the slow side), Linux. Every tool that supports threads ran with 2 threads (`zstd -T2`, `xz -T2`, `7z -mmt=2`, `ezpz -j2`).
- Versions: zstd 1.5.5, XZ Utils 5.4.5, 7-Zip 23.01, Info-ZIP 3.0, gzip 1.12, GNU tar 1.35, ezpz 0.3.0. The ezpz rows for levels 3, 7, 9, and 11 on the Wikipedia text and the Silesia corpus come from ezpz 0.1.0, which writes the same data for them.
- Each measurement was taken once on an otherwise idle server, so expect a few percent of noise in the timings.
- Sizes read original → compressed, in MB (1 MB = 1,000,000 bytes). The compressed size is the whole archive file, index included. "80% smaller" means the archive is one fifth the size of the original.
- Single file is the time to extract one small file (for example `json/decoder.py`) from the archive. The tar formats have to be decompressed from the start to reach it.
- Peak RAM is the most memory (RAM) the tool used while compressing / extracting. It is not a file size.
- Every ezpz extraction was compared byte for byte with the original.
- `--max` is level 10: the fast brain codec (codec 4) with 16 MiB blocks. `-l 11` is the full brain codec (codec 3) with 64 MiB blocks; ezpz 0.1.0 called this setting `--max`.

## Summary: size after compression, in MB (smaller is better)

| Method | Wikipedia text | Silesia corpus | Program install folder | Multi-version backup | Executables |
|---|---|---|---|---|---|
| **Original (before compression)** | 100.0 | 211.9 | 53.2 | 158.2 | 104.9 |
| zip -9 | 36.4 | 67.6 | 16.0 | 52.0 | 39.2 |
| tar.gz -9 | 36.4 | 67.6 | 15.3 | 49.6 | 39.1 |
| tar.zst -19 --long | 26.5 | 52.7 | 10.1 | 27.9 | 25.7 |
| tar.xz -9e | 24.8 | 48.4 | 9.1 | 24.3 | 23.1 |
| 7z -mx9 | 24.9 | 48.7 | 9.1 | 23.7 | **20.9** |
| ezpz -l 3 | 32.7 | 61.0 | 12.8 | 35.0 | 32.8 |
| ezpz (default -l 7) | 26.8 | 52.6 | 10.4 | 28.4 | 25.7 |
| ezpz -l 9 | 25.4 | 48.2 | 9.0 | 24.2 | 22.8 |
| ezpz --max (-l 10, fast brain) | 21.5 | 43.2 | 9.0 | 26.4 | 23.8 |
| ezpz -l 11 (brain) | **20.5** | **41.8** | **7.9** | **23.1** | 22.2 |

### The brain codecs against the best existing settings

| Dataset | Original | xz -9e | 7z -mx9 | ezpz --max | ezpz -l 11 |
|---|---|---|---|---|---|
| Wikipedia text | 100.0 MB | 24.8 MB | 24.9 MB | 21.5 MB (13.3% smaller than xz -9e) | **20.5 MB** (17.4% smaller than xz -9e) |
| Silesia corpus | 211.9 MB | 48.4 MB | 48.7 MB | 43.2 MB (10.9% smaller than xz -9e) | **41.8 MB** (13.6% smaller than xz -9e) |
| Program install folder | 53.2 MB | 9.1 MB | 9.1 MB | 9.0 MB (0.8% smaller than 7z -mx9) | **7.9 MB** (12.7% smaller than 7z -mx9) |
| Multi-version backup | 158.2 MB | 24.3 MB | 23.7 MB | 26.4 MB (11.5% larger than 7z -mx9) | **23.1 MB** (2.6% smaller than 7z -mx9) |
| Executables | 104.9 MB | 23.1 MB | **20.9 MB** | 23.8 MB (14.0% larger than 7z -mx9) | 22.2 MB (6.0% larger than 7z -mx9) |

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
| ezpz --max (-l 10, fast brain) | 100.00 → 21.54 MB | 78.5% smaller | 53.3 s | 58.3 s | n/a | 480 / 485 MB |
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
| ezpz --max (-l 10, fast brain) | 211.94 → 43.17 MB | 79.6% smaller | 113 s | 122 s | 20.0 s | 549 / 529 MB |
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
| ezpz -l 3 | 53.23 → 12.76 MB | 76.0% smaller | 0.63 s | 0.12 s | 0.02 s | 64 / 60 MB |
| ezpz (default -l 7) | 53.23 → 10.35 MB | 80.6% smaller | 17.7 s | 0.21 s | 0.09 s | 256 / 58 MB |
| ezpz -l 9 | 53.23 → 9.05 MB | 83.0% smaller | 40.3 s | 0.73 s | 0.49 s | 1187 / 109 MB |
| ezpz --max (-l 10, fast brain) | 53.23 → 8.98 MB | 83.1% smaller | 31.5 s | 33.5 s | 14.9 s | 436 / 411 MB |
| ezpz -l 11 (brain) | 53.23 → **7.90 MB** | **85.2% smaller** | 73.4 s | 74.6 s | 71.4 s | 379 / 377 MB |

## Multi-version backup

The Python 3.11, 3.12, and 3.13 install folders together (158 MB, 3,750 files), standing in for an archive that keeps several versions of one project

| Method | Original → compressed | Size reduction | Compress time | Extract time | Single file | Peak RAM while compressing / extracting |
|---|---|---|---|---|---|---|
| zip -9 | 158.18 → 52.05 MB | 67.1% smaller | 41.9 s | 1.2 s | 0.01 s | 11 / 11 MB |
| tar.gz -9 | 158.18 → 49.59 MB | 68.6% smaller | 44.7 s | 1.4 s | 1.1 s | 11 / 11 MB |
| tar.zst -19 --long | 158.18 → 27.91 MB | 82.4% smaller | 54.3 s | 1.3 s | 0.34 s | 352 / 132 MB |
| tar.xz -9e | 158.18 → 24.28 MB | 84.6% smaller | 134 s | 2.1 s | 2.2 s | 852 / 243 MB |
| 7z -mx9 | 158.18 → 23.68 MB | 85.0% smaller | 71.8 s | 3.0 s | 1.6 s | 689 / 168 MB |
| ezpz -l 3 | 158.18 → 34.97 MB | 77.9% smaller | 1.9 s | 0.37 s | 0.02 s | 90 / 82 MB |
| ezpz (default -l 7) | 158.18 → 28.37 MB | 82.1% smaller | 41.4 s | 1.2 s | 0.07 s | 363 / 155 MB |
| ezpz -l 9 | 158.18 → 24.22 MB | 84.7% smaller | 116 s | 2.3 s | 0.53 s | 1522 / 211 MB |
| ezpz --max (-l 10, fast brain) | 158.18 → 26.40 MB | 83.3% smaller | 72.3 s | 76.5 s | 15.0 s | 509 / 508 MB |
| ezpz -l 11 (brain) | 158.18 → **23.06 MB** | **85.4% smaller** | 115 s | 119 s | 97.2 s | 839 / 815 MB |

## Executables

231 executables from Linux /usr/bin (105 MB, x86-64)

| Method | Original → compressed | Size reduction | Compress time | Extract time | Single file | Peak RAM while compressing / extracting |
|---|---|---|---|---|---|---|
| zip -9 | 104.86 → 39.22 MB | 62.6% smaller | 19.1 s | 0.88 s | 0.01 s | 11 / 11 MB |
| tar.gz -9 | 104.86 → 39.15 MB | 62.7% smaller | 18.5 s | 1.0 s | 0.91 s | 11 / 11 MB |
| tar.zst -19 --long | 104.86 → 25.74 MB | 75.5% smaller | 40.8 s | 0.32 s | 0.28 s | 299 / 104 MB |
| tar.xz -9e | 104.86 → 23.10 MB | 78.0% smaller | 91.6 s | 1.9 s | 2.0 s | 797 / 188 MB |
| 7z -mx9 | 104.86 → **20.91 MB** | **80.1% smaller** | 41.2 s | 1.6 s | 0.02 s | 706 / 121 MB |
| ezpz -l 3 | 104.86 → 32.79 MB | 68.7% smaller | 2.4 s | 0.80 s | 0.02 s | 116 / 120 MB |
| ezpz (default -l 7) | 104.86 → 25.67 MB | 75.5% smaller | 42.0 s | 0.80 s | 0.08 s | 340 / 168 MB |
| ezpz -l 9 | 104.86 → 22.80 MB | 78.3% smaller | 106 s | 1.4 s | 0.96 s | 1469 / 181 MB |
| ezpz --max (-l 10, fast brain) | 104.86 → 23.84 MB | 77.3% smaller | 59.4 s | 62.5 s | 18.1 s | 501 / 512 MB |
| ezpz -l 11 (brain) | 104.86 → 22.16 MB | 78.9% smaller | 114 s | 109 s | 115 s | 730 / 707 MB |

## Small files

Fifteen files of 4 to 35 KB, each compressed on its own. Sizes are in KB (1 KB = 1,000 bytes) and include each format's own headers. gzip, zstd, and xz write a single compressed stream with almost no header. zip, 7z, and ezpz write archives with an index; for ezpz that index plus the trailer adds about 250 bytes per archive, mostly hashes. Both brain codecs start these small blocks from the built-in priming data (SPEC §6.4.11).

| Content | Original | zip -9 | gzip -9 | zstd -19 | xz -9e | 7z -mx9 | ezpz -l 7 | ezpz --max | ezpz -l 11 |
|---|---|---|---|---|---|---|---|---|---|
| C header (glibc stdio.h) | 34.6 KB | 7.0 KB | 6.9 KB | 6.5 KB | 6.5 KB | 6.6 KB | 6.8 KB | 5.1 KB | **5.0 KB** |
| JSON (ISO country codes) | 24.0 KB | 3.8 KB | 3.7 KB | 3.3 KB | 3.0 KB | 3.0 KB | 3.5 KB | 2.5 KB | **2.5 KB** |
| Table, tab-separated (time zones) | 17.6 KB | 8.8 KB | 8.7 KB | 8.4 KB | 7.7 KB | 7.7 KB | 8.6 KB | 6.5 KB | **6.4 KB** |
| XML (Silesia slice) | 24.0 KB | 2.8 KB | 2.6 KB | 2.4 KB | 2.3 KB | 2.4 KB | 2.6 KB | 2.1 KB | **2.1 KB** |
| English license text (Apache 2.0) | 11.4 KB | 4.1 KB | 4.0 KB | 3.8 KB | 3.9 KB | 4.0 KB | 4.1 KB | 2.9 KB | **2.9 KB** |
| English novel (Dickens) | 32.0 KB | 13.7 KB | 13.5 KB | 13.0 KB | 12.9 KB | 13.0 KB | 13.2 KB | 10.2 KB | **10.1 KB** |
| English web page (HTML) | 4.5 KB | 2.1 KB | 2.0 KB | 1.9 KB | 2.0 KB | 2.1 KB | 2.2 KB | 1.5 KB | **1.5 KB** |
| Spanish tutorial | 12.0 KB | 4.0 KB | 3.9 KB | 3.8 KB | 3.8 KB | 3.9 KB | 4.0 KB | 3.2 KB | **3.2 KB** |
| Japanese web page (HTML) | 5.9 KB | 2.4 KB | 2.3 KB | 2.3 KB | 2.2 KB | 2.3 KB | 2.5 KB | 1.9 KB | **1.8 KB** |
| Japanese tutorial | 16.0 KB | 4.6 KB | 4.5 KB | 4.3 KB | 4.3 KB | 4.3 KB | 4.6 KB | 3.5 KB | **3.4 KB** |
| Korean Markdown (README) | 7.4 KB | 3.7 KB | 3.6 KB | 3.5 KB | 3.5 KB | 3.5 KB | 3.7 KB | 2.7 KB | **2.7 KB** |
| Korean technical document | 24.0 KB | 10.3 KB | 10.2 KB | 9.8 KB | 9.6 KB | 9.6 KB | 10.0 KB | 7.6 KB | **7.5 KB** |
| Korean tutorial | 16.0 KB | 5.2 KB | 5.0 KB | 4.8 KB | 4.8 KB | 4.9 KB | 5.1 KB | 3.6 KB | **3.5 KB** |
| Python source (textwrap.py) | 19.7 KB | 6.2 KB | 6.1 KB | 5.9 KB | 5.9 KB | 6.0 KB | 6.1 KB | 4.6 KB | **4.6 KB** |
| Wikipedia XML | 16.0 KB | 7.4 KB | 7.2 KB | 7.0 KB | 6.9 KB | 7.0 KB | 7.3 KB | 5.6 KB | **5.5 KB** |
| **Total** | 265.2 KB | 86.2 KB | 84.1 KB | 80.6 KB | 79.3 KB | 80.3 KB | 84.3 KB | 63.5 KB | **62.7 KB** |

In total, ezpz --max is 19.9% smaller than xz -9e and 26.4% smaller than zip -9 on these files.

## Discussion

### Strengths

- `-l 11` (the full brain codec with 64 MiB blocks) produced the smallest archive on 4 of the 5 datasets. On the Wikipedia text, the Silesia corpus, and the Python install folder it is 12 to 18% smaller than the best xz and 7z settings, and on the multi-version backup it is 3 to 5% smaller.
- `--max` (the fast brain codec with 16 MiB blocks) compresses and extracts 1.6 to 2.3 times as fast as `-l 11` on this server, and pulls a single file out 4.8 to 6.5 times as fast (15 to 20 s instead of 71 to 115 s). On a 4-core Mac the gap is about 4 times: 48 MB of text took 9 s instead of 38 s to compress and 10 s instead of 41 s to extract. It stays well ahead of xz on text (13.3% smaller on the Wikipedia text, 10.9% on the Silesia corpus), and it compresses faster than xz -9e on every dataset (53 s vs 150 s on the Wikipedia text).
- On small files the brain codecs pull further ahead, because they start from built-in knowledge (75 KB of sample text, code, and data). Over the 15 small files, `--max` is 19.9% smaller than xz -9e and 26.4% smaller than zip -9, and only `-l 11` beats it on any single file.
- The x86-64 transform (RIP-relative addresses as well as call targets) made the executables 1.8 to 2.5% smaller at every level, and the brain-fast codec now picks models that suit machine code, so `--max` shrank 7.2% on that dataset compared with ezpz 0.2.0.
- The default level (`-l 7`) comes out about the same size as tar.zst -19 --long (at most 2.4% larger, and slightly smaller on the Silesia corpus and the executables). It compresses as fast or faster (78 s vs 113 s on Silesia), extracts in under 1.5 s, and pulls a single file out in under 0.1 s, which tar.zst cannot do.
- `-l 3` is 10 to 33% smaller than zip and compresses 5.6 to 23 times faster (on the backup data, zip took 41.9 s and ezpz 1.9 s).
- `-l 9` is close to xz and 7z in size, and the smallest of the three on Silesia and on the Python install folder. It is faster at full extraction and at single-file extraction (one file from Silesia: 1.0 s, vs 4.3 s for xz and 3.5 s for 7z).
- Every extraction matched the original byte for byte.

### Weaknesses

- `--max` gives up size on program files and backups: 14 to 15% larger than `-l 11` on the Python folder and the backup, and 8% larger on the executables. On the backup it is 11.5% larger than 7z, and on the backup and the executables `-l 9` is smaller and extracts much faster. Most of the loss comes from the 16 MiB blocks, which capture less of the similarity between files than 64 MiB blocks. For the smallest archive of such data, use `-l 11`.
- Both brain codecs are slow to extract. Even `--max` runs at about 2 MB/s on this server (about 5 MB/s on a 4-core Mac), tens of times slower than xz. They suit long-term storage better than files you open often.
- On executables `-l 11` is still 6.0% larger than 7z. 7z's x86 preprocessor (BCJ2) sends call targets to a separate stream, which ezpz does not do yet. The dataset is also dominated by large Go programs, where the x86-64 transform helps less than on C programs (6 to 8% on a sample of 145 smaller programs).
- For files of a few KB, the default level is no smaller than zip: the ezpz index (about 250 bytes, mostly hashes) costs as much as zstd saves.
- Memory use is high. `-l 9` tries both zstd 22 and LZMA2 9e on every block and needs 1.2 to 1.6 GB while compressing. With 2 threads the brain codecs use about 0.4 to 0.55 GB (`--max`) and 0.4 to 0.9 GB (`-l 11`).
- The default level is up to 2.4% larger than tar.zst -19. Splitting the data into blocks costs that much, and it is what makes parallel work and single-file extraction possible.
- Every figure comes from a single run on a slow virtual server. An Apple Silicon machine will run all of these tools faster.

### Next steps

- Executables: split call targets into their own stream (BCJ2-style) to close the remaining gap to 7z
- Small files at the default level: use the same priming data as a zstd dictionary
- `--max` on folders: share learned state between the 16 MiB blocks of one archive without losing parallel extraction
