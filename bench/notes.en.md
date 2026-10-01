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
