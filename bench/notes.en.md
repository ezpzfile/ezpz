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
