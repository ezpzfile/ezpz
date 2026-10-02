# 브라우저에서 쓰는 ezpz

[English](README.md) | 한국어

이 폴더에는 ezpz의 웹어셈블리 빌드와, 그걸로 브라우저에서 `.ezpz`를 열고 풀고 만드는 페이지가 있어요. 파일은 기기 밖으로 나가지 않아요. 서버 쪽 코드가 아예 없어요.

## 써 보기

- **`dist/ezpz.html`**: 필요한 게 모두 들어 있는 파일 하나예요(약 1.7MB). 내려받아서 바로 열면 돼요.
- **`index.html`**: 같은 페이지인데 `worker.js`와 `pkg/`를 불러와요. 아무 정적 웹 서버로 이 폴더를 띄우고(예: `python3 -m http.server -d web`) http://localhost:8000/ 을 여세요.

페이지에서 할 수 있는 일은 이래요.

- 압축 파일을 열어 목록을 보고 손상 검사하기
- 파일 하나만 받거나, 전부 `.zip`으로 받기
- 암호가 걸린 파일을 비밀번호로 열고 서명한 사람의 키 보기
- 파일과 폴더로 새 압축 파일 만들기(폴더도 끌어다 놓을 수 있어요). 압축 레벨은 4가지 중에서 고르고 비밀번호는 원할 때만 걸어요

화면은 한국어와 영어를 지원하고 시스템의 밝은/어두운 설정을 따라가요.

## 명령어 도구와 다른 점

- **LZMA2 압축기가 없어요.** 레벨 9는 zstd로만 압축해요. 명령어 도구가 만든 LZMA2 블록은 순수 Rust 디코더로 문제없이 풀어요.
- **코어 하나로 일해요.** 블록을 하나씩 차례로 압축해요(테스트 서버에서 기본 레벨로 1초에 약 2MB). 페이지는 작업을 Web Worker에서 돌려서 화면이 멈추지 않아요.
- **모두 메모리에서 처리해요.** 압축 파일과 그 안의 파일이 브라우저 메모리에 들어가야 해요. 몇 GB짜리는 명령어 도구를 쓰세요.
- **뇌 코덱은 여기서도 느려요.** 같은 서버에서 레벨 10은 1초에 약 0.8MB, 레벨 11은 약 0.6MB를 처리했어요. 압축할 때나 풀 때나 비슷해요.

레벨 9를 뺀 나머지 레벨에서는 만드는 블록이 명령어 도구와 바이트 단위로 같아요.

## 파일

| 파일 | 하는 일 |
|---|---|
| `index.html` | 페이지 |
| `worker.js` | Web Worker. 메시지 형식은 파일 맨 위에 적혀 있어요 |
| `pkg/` | wasm-bindgen 결과물(`--target web`): `ezpz_wasm.js`, `ezpz_wasm_bg.wasm`, TypeScript 타입 |
| `dist/ezpz.html` | 파일 하나짜리 페이지. `inline.py`가 만들어요 |
| `build.sh` | `pkg/`와 `dist/`를 빌드해요 |
| `test.mjs` | 웹어셈블리판을 Node에서 시험해요: `node web/test.mjs` |

## 자바스크립트 API

```js
import init, { EzpzArchive, create, version } from './pkg/ezpz_wasm.js';
await init(); // ezpz_wasm.js 옆의 ezpz_wasm_bg.wasm을 불러와요

// 열기, 목록, 꺼내기
const archive = new EzpzArchive(new Uint8Array(await file.arrayBuffer()), password); // password는 선택
if (archive.needsPassword()) {
  // 암호가 걸렸는데 비밀번호를 안 줬어요. 비밀번호와 함께 다시 여세요
}
const entries = JSON.parse(archive.entriesJson()); // [{path, type, size, mode, mtime, block, target}]
const data = archive.read('photos/a.jpg');          // Uint8Array, 해시 확인됨
const report = JSON.parse(archive.verifyJson());    // {ok, blocksOk, blocksBad, filesOk, filesBad, contentChecked}
const info = JSON.parse(archive.infoJson());        // {encrypted, signed, blocks, storedBytes, files, dirs, links, contentBytes, creator, created}
archive.free();

// 만들기
const bytes = create(
  [
    { path: 'notes/today.txt', data: new TextEncoder().encode('hello'), mtime: Date.now() },
    { path: 'notes/drafts', dir: true },
  ],
  { level: 7, password: '' },                  // level 1~11(기본 7), password는 선택
  (done, total) => console.log(done / total), // 진행률(선택), 블록마다 불려요
);
```

- `type`은 `"file"`, `"dir"`, `"symlink"` 중 하나예요. `mtime`은 유닉스 초예요. `block` 순서대로 파일을 읽으면 블록마다 한 번씩만 풀어요. 아카이브 객체는 가장 최근에 푼 블록 2개를 기억해요.
- 그 밖의 메서드: `isEncrypted()`, `isSigned()`, `signerKey()`(Ed25519 공개키 16진수, 서명은 열 때 확인됨), `digest()`(아카이브 지문), `clearCache()`, `version()`
- 오류는 읽을 수 있는 메시지와 함께 던져요. 예를 들어 비밀번호가 틀렸거나 데이터가 손상됐을 때예요.

## 사이트에 넣기

1. `pkg/`와 `worker.js`를 사이트에 복사하고 `.wasm` 파일은 `application/wasm`으로 내보내세요.
2. 몇 MB를 넘는 작업은 워커에서 돌리세요(`worker.js`를 그대로 쓰면 돼요).
3. 한 번 불러온 뒤에는 네트워크가 필요 없어요.

사용자가 볼 수 있는 곳에 EZPZ File 출처를 표시해 주세요(아래 라이선스).

## 빌드

`wasm32-unknown-unknown` 타깃을 넣은 Rust, wasm32로 컴파일할 수 있는 clang(zstd가 C 코드라서 필요해요. 맥에서는 애플 clang에 웹어셈블리 백엔드가 없으니 Homebrew의 `llvm`을 설치하세요), `wasm-bindgen-cli` 0.2.129, Python 3이 필요해요.

```bash
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.129
web/build.sh
```

## 라이선스

저장소의 다른 파일과 같은 [MIT 라이선스](../LICENSE)예요. 제품·사이트·서비스에 쓴다면 사용자가 볼 수 있는 곳(정보 화면, 크레딧, 푸터 등)에 출처를 표시해 주세요.

    Powered by EZPZ File (https://ezpzfile.com)
