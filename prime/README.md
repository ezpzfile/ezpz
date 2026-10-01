# Priming data

English | [한국어](README.ko.md)

`v1.txt` is the built-in knowledge of the brain codecs ([SPEC.md](../SPEC.md) §6.4.11 and Appendix B). Before it codes a small block, the codec can run this text through its model, so it starts out already knowing common words, code, and data formats. Small files come out about 9% smaller this way. The zstd levels use the same text as a dictionary (§6.6), which makes small files about 5% smaller at the default level.

The file is part of the format: changing a single byte breaks compatibility, so a future version will go into a new file. The text was written for this purpose and is released under the MIT license with the rest of the repository.
