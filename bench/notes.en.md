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
