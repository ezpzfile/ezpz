#!/usr/bin/env python3
"""Builds web/dist/ezpz.html: web/index.html with the worker, the wasm-bindgen glue, and the
WebAssembly module inlined, so the page works when opened straight from the disk.

Browsers refuse module workers on pages opened from the disk (origin "null"), so this page runs
the worker as a classic script with wasm-bindgen's no-modules glue (built by build.sh into
target/web-classic)."""
import base64
import json
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
CLASSIC = sys.argv[1] if len(sys.argv) > 1 else os.path.join(HERE, "..", "target", "web-classic")


def read(path, mode="r"):
    with open(path, mode, **({} if "b" in mode else {"encoding": "utf-8"})) as f:
        return f.read()


page = read(os.path.join(HERE, "index.html"))
glue = read(os.path.join(CLASSIC, "ezpz_wasm.js"))
worker = read(os.path.join(HERE, "worker.js"))
imp = "import * as ez from './pkg/ezpz_wasm.js';\n"
assert imp in worker, "worker.js import line changed; update inline.py"
worker_src = glue + "\nconst ez = wasm_bindgen;\n" + worker.replace(imp, "")
wasm_b64 = base64.b64encode(read(os.path.join(CLASSIC, "ezpz_wasm_bg.wasm"), "rb")).decode("ascii")


def js_string(s):
    # A JSON string is a valid JavaScript string; "</" is escaped so that the text can never
    # close the surrounding <script> element.
    return json.dumps(s).replace("</", "<\\/")


for old, new in (
    ("const WASM_B64 = null;", "const WASM_B64 = " + js_string(wasm_b64) + ";"),
    ("const WORKER_SRC = null;", "const WORKER_SRC = " + js_string(worker_src) + ";"),
):
    assert page.count(old) == 1, old
    page = page.replace(old, new)

os.makedirs(os.path.join(HERE, "dist"), exist_ok=True)
with open(os.path.join(HERE, "dist", "ezpz.html"), "w", encoding="utf-8") as f:
    f.write(page)
