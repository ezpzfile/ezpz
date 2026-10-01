### Strengths

- `-l 11` (the full brain codec with 64 MiB blocks) produced the smallest archive on all 5 datasets. On the Wikipedia text, the Silesia corpus, and the Python install folder it is 12 to 18% smaller than the best xz and 7z settings. On the multi-version backup it is 3 to 5% smaller, and on the executables 5.1% smaller than 7z and 14% smaller than xz.
- `--max` (the fast brain codec with 16 MiB blocks) compresses and extracts 1.6 to 2.3 times as fast as `-l 11` on this server, and pulls a single file out 5.4 to 6.3 times as fast (14 to 20 s instead of 76 to 112 s). On a 4-core Mac the gap is about 4 times: 48 MB of text took 9 s instead of 38 s to compress and 10 s instead of 41 s to extract. It stays well ahead of xz on text (13.3% smaller on the Wikipedia text, 10.9% on the Silesia corpus), and it compresses faster than xz -9e on every dataset (53 s vs 150 s on the Wikipedia text).
- On small files the brain codecs pull further ahead, because they start from built-in knowledge (75 KB of sample text, code, and data). Over the 15 small files, `--max` is 19.9% smaller than xz -9e and 26.4% smaller than zip -9, and only `-l 11` beats it on any single file.
- The default level now uses the same built-in knowledge as a zstd dictionary for blocks of up to 1 MiB. On the 15 small files it came out 5.4% smaller than in ezpz 0.3.0: smaller than zip -9 on every file (7.5% in total), and within 0.6% of xz -9e in total.
- Executables shrank at every level, by 3.5% (`-l 3`) to 10.5% (`-l 11`) compared with ezpz 0.3.0. For 64-bit x86 programs, ezpz now moves call and data addresses out of the machine code to the end of each file, an idea similar to 7z's BCJ2. The code that remains then matches across different builds of a program, and in large programs addresses that span more than 8 MiB are converted too. On a set of unrelated small libraries this loses about 1.5%, so at levels 4 and above the encoder tries both ways quickly and keeps the better one. It kept the old in-place method for the two Python datasets.
- The default level (`-l 7`) comes out about the same size as tar.zst -19 --long: at most 2.4% larger, slightly smaller on the Silesia corpus, and 5.6% smaller on the executables. It compresses faster on every dataset (78 s vs 113 s on Silesia), extracts in under 1 s, and pulls a single file out in under 0.1 s, which tar.zst cannot do.
- `-l 3` is 10 to 33% smaller than zip and compresses 5.6 to 26 times faster (on the backup data, zip took 41.9 s and ezpz 1.6 s).
- `-l 9` is close to xz and 7z in size: the smallest of the three on Silesia and on the Python install folder, and within 1.7% of 7z on the executables. It is faster at full extraction and at single-file extraction (one file from Silesia: 1.0 s, vs 4.3 s for xz and 3.5 s for 7z).
- Every extraction matched the original byte for byte.

### Weaknesses

- `--max` gives up size on program files and backups: 13 to 15% larger than `-l 11` on the Python folder, the backup, and the executables. It is 11.5% larger than 7z on the backup and 6.8% larger on the executables, and on both of those `-l 9` is smaller and extracts much faster. Most of the loss comes from the 16 MiB blocks, which capture less of the similarity between files than 64 MiB blocks. For the smallest archive of such data, use `-l 11`.
- Both brain codecs are slow to extract. Even `--max` runs at about 2 MB/s on this server (about 5 MB/s on a 4-core Mac), tens of times slower than xz. They suit long-term storage better than files you open often.
- On executables, 7z still beats the zstd and LZMA2 levels: `-l 9` is 1.7% larger than 7z, and only the brain codec at `-l 11` is smaller. Levels 1 to 3 skip the quick trial between the two address methods to stay fast, which cost up to 0.13% on the two Python datasets.
- The default level is still larger than xz -9e on 6 of the 15 small files (the JSON, table, and XML data and the Spanish and Japanese text). Part of the reason is the ezpz index, which adds about 250 bytes per archive, mostly hashes.
- Memory use is high. `-l 9` tries both zstd 22 and LZMA2 9e on every block and needs 1.2 to 1.6 GB while compressing. With 2 threads the brain codecs use about 0.4 to 0.55 GB (`--max`) and 0.4 to 0.9 GB (`-l 11`).
- The default level is up to 2.4% larger than tar.zst -19. Splitting the data into blocks costs that much, and it is what makes parallel work and single-file extraction possible.
- Every figure comes from a single run on a slow virtual server. An Apple Silicon machine will run all of these tools faster.

### Next steps

- `--max` on folders: share learned state between the 16 MiB blocks of one archive without losing parallel extraction
- Programs for Apple Silicon and other ARM64 machines: move call targets out of the code, as ezpz now does for x86-64
