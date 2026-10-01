# ezpz: `.ezpz` 아카이브 포맷 레퍼런스 구현

[English](README.md) | 한국어

zip·7z 같은 "여러 파일을 묶고 줄이는" 포맷이에요. 검증된 압축기(zstd, LZMA2)를 똑똑하게 조합하고, 직접 만든 **뇌 코덱**(예측 부호화 + 실시간으로 배우는 신경망)을 최대 압축 모드로 넣었어요.

- 규격서: [SPEC.ko.md](SPEC.ko.md)
- 벤치마크: [BENCHMARK.ko.md](BENCHMARK.ko.md)

## 결과 한눈에

최대 압축 모드(`ezpz --max`, 뇌 코덱)로 압축한 크기를 xz·7z의 최고 압축과 나란히 놓았어요. 마지막 칸은 xz와 7z 중 더 작게 만든 쪽과 비교한 결과예요.

| 데이터 | 원래 크기 | ezpz --max | xz -9e | 7z -mx9 | 결과 |
|---|---|---|---|---|---|
| 위키백과 텍스트 (enwik8) | 100.0MB | **20.5MB** | 24.8MB | 24.9MB | xz -9e보다 17.4% 작음 |
| Silesia 표준 테스트 세트 | 211.9MB | **41.8MB** | 48.4MB | 48.7MB | xz -9e보다 13.6% 작음 |
| Python 설치 폴더 | 53.2MB | **7.9MB** | 9.1MB | 9.1MB | 7z -mx9보다 12.5% 작음 |
| Python 3개 버전 백업 | 158.2MB | **23.1MB** | 24.3MB | 23.7MB | 7z -mx9보다 2.4% 작음 |
| 리눅스 실행 파일 | 104.9MB | 22.7MB | 23.1MB | **20.9MB** | 7z -mx9보다 8.7% 큼 |

기본 레벨은 tar.zst -19와 비슷한 크기에, 파일 하나를 0.1초 안에 꺼낼 수 있어요. 자세한 수치는 [BENCHMARK.ko.md](BENCHMARK.ko.md)에 있어요.

## 빌드

Rust 1.85 이상이 필요해요(edition 2024 사용). (맥: `brew install rust` 또는 https://rustup.rs)

```bash
cargo build --release
# 실행 파일: target/release/ezpz
```

외부 라이브러리(zstd, liblzma)는 소스째 같이 빌드되므로 따로 설치할 게 없어요.

## 사용법

```bash
# 압축 (기본 레벨 7: tar.zst -19 수준으로 작고, 푸는 건 아주 빠름)
ezpz c 프로젝트.ezpz 내폴더/ 다른파일.txt

# 레벨: 1(제일 빠름) ~ 9(제일 작게), --max 는 뇌 코덱(가장 작지만 느림)
ezpz c 보관용.ezpz 내폴더/ -l 9
ezpz c 보관용.ezpz 내폴더/ --max

# 풀기 / 일부만 풀기
ezpz x 프로젝트.ezpz -C 풀폴더/
ezpz x 프로젝트.ezpz 내폴더/docs -C 풀폴더/

# 목록, 파일 하나만 바로 보기(필요한 블록만 읽음)
ezpz l 프로젝트.ezpz
ezpz cat 프로젝트.ezpz 내폴더/README.md

# 무결성 검사
ezpz verify 프로젝트.ezpz

# 암호화 (파일 이름까지 숨김). 비밀번호는 물어보거나 --password / EZPZ_PASSWORD
ezpz c 비밀.ezpz 내폴더/ -e

# 비밀번호 없이도 손상 검사는 가능 (서명된 파일이면 변조까지)
ezpz verify 비밀.ezpz --no-password

# 전자 서명: 키 만들기 → 서명해서 압축 → 받는 사람이 공개키로 확인
ezpz keygen 나
ezpz c 배포.ezpz 내폴더/ --sign 나.key
ezpz verify 배포.ezpz --pubkey 나.pub

# 아카이브 정보(코덱별 블록, 중복 제거량, 지문 등)
ezpz info 프로젝트.ezpz
```

자주 쓰는 옵션: `-j N`(스레드 수), `--block-size MiB`, `--codec zstd|lzma2|brain|store`, `--no-dedup`, `--no-filter`, `--hash-len 0|16|32`, `-f/--force`(덮어쓰기), `--unsafe-links`(폴더 밖을 가리키는 심볼릭 링크도 만들기).

## 구조 (소스 파일)

| 파일 | 하는 일 |
|---|---|
| `src/format.rs` | 헤더·프레임·트레일러·서명 섹션의 바이트 배치, varint, 경로 규칙 |
| `src/index.rs` | 블록 테이블과 카탈로그(열 단위 인덱스) 인코딩/디코딩 |
| `src/codec.rs` | store / zstd / LZMA2 / brain 코덱 연결 |
| `src/brain.rs` | 뇌 코덱: 컨텍스트 모델 9개 + 매치 모델 + 신경망 믹서 3개 + APM + 산술 부호기 |
| `src/filter.rs` | 실행 파일 전처리(x86 E8/E9, ARM64 BL) |
| `src/classify.rs` | 파일 분류(실행 파일 / 이미 압축됨 / 일반) |
| `src/create.rs` | 압축기: 분류 → 정렬 → 내용 기반 조각내기 → 중복 제거 → 블록 병렬 압축 |
| `src/archive.rs` | 해제기: 검증, 블록 캐시, 랜덤 접근, 안전한 추출 |
| `src/crypto.rs` | Argon2id + XChaCha20-Poly1305 |

## 독립 구현 검증

`tools/ezpz_reader.py`는 **SPEC.md만 보고** 따로 만든 파이썬 해제기예요(Rust 코드를 보지 않고 작성). `testvectors/`의 아카이브를 이 해제기로 풀어서 원본과 비교하면 규격서만으로 호환 구현이 가능하다는 걸 확인할 수 있어요.

```bash
pip install zstandard blake3 cryptography
python3 tools/ezpz_reader.py testvectors/brain.ezpz --out /tmp/out --compare testvectors/input --sub input
python3 tools/ezpz_reader.py testvectors/signed.ezpz --pub testvectors/k.pub
```

암호화된 아카이브를 풀려면 `pip install argon2-cffi pynacl`도 설치하고 `--password`를 주면 돼요.

## 테스트

```bash
cargo test --release      # 단위 테스트 (포맷, 변조된 인덱스 거부, 경로 공격 등)
tests/e2e.sh              # 실제 CLI로 왕복·변조·암호·서명 시나리오
```

## 솔직한 한계

- 뇌 코덱은 텍스트·표준 테스트 세트·프로그램 폴더에서 xz·7z보다 12~18% 작지만, 압축과 해제가 둘 다 느려요(2코어 개발 서버에서 초당 약 1MB). 파일 하나를 꺼낼 때도 그 파일이 든 블록(최대 64MB)을 통째로 풀어야 해요. 그래서 기본값이 아니라 `--max`에서만 써요.
- 실행 파일은 아직 7z 쪽이 약 9% 더 작아요. 7z의 x86 전용 전처리기(BCJ2)만큼 정교한 처리가 없어서예요.
- 이미 압축된 데이터(jpg, mp4, zip 등)는 어떤 방법으로도 거의 줄지 않아요. ezpz는 이런 파일을 알아보고 그대로 저장해서 시간만 아껴요.
- 서명 없는 아카이브의 해시 검사는 **실수로 생긴 손상**(전송 오류, 디스크 손상)을 잡아요. 누군가 **일부러 고친 것**까지 확인하려면 `--sign`으로 서명하고, 받는 쪽에서 `verify --pubkey`로 확인하세요(공개키를 지정하면 서명 없는 파일은 거부돼요).
- 아직 v1 초안이에요. 규격이 바뀔 수 있으니 중요한 데이터의 유일한 보관본으로는 쓰지 마세요.

## 라이선스

[MIT 라이선스](LICENSE)로 공개해요. 규격서([SPEC.ko.md](SPEC.ko.md))를 보고 다른 언어로 호환 구현을 만드는 것도 자유예요.

