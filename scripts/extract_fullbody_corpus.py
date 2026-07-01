#!/usr/bin/env python3
"""Extract full-body embed texts (decl + body, ~2KB cap) — mirrors codegraph's
`--full-body-embedding` symbol text. Output JSON [{id, signature}] where
`signature` holds the whole text, so examples/embed_throughput.rs can consume it
via CODEGRAPH_THROUGHPUT_CORPUS for a static-vs-BGE throughput test on long texts.

    python scripts/extract_fullbody_corpus.py [ROOT=crates] [OUT=/tmp/cg_fullbody.json]
"""
import json
import re
import sys
from pathlib import Path

CAP = 2048  # codegraph FULL_BODY_MAX_CHARS
ROOT = Path(sys.argv[1]) if len(sys.argv) > 1 else Path("crates")
OUT = Path(sys.argv[2]) if len(sys.argv) > 2 else Path("/tmp/codegraph_fullbody_corpus.json")
DECL = re.compile(
    r"^\s*(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?(?:fn|struct|trait|enum|impl)\s+([A-Za-z_][A-Za-z0-9_]*)"
)


def main() -> int:
    items = {}
    for f in ROOT.rglob("*.rs"):
        p = str(f)
        if any(s in p for s in ("/tests/", "/examples/", "/benches/")):
            continue
        lines = f.read_text(errors="ignore").splitlines()
        for i, line in enumerate(lines):
            m = DECL.match(line)
            if not m:
                continue
            name = m.group(1)
            if name in items:
                continue
            body = "\n".join(lines[i : i + 45])[:CAP]
            if len(body) >= 40:
                items[name] = {"id": name, "signature": body}
    out = list(items.values())
    OUT.write_text(json.dumps(out))
    print(f"extracted {len(out)} full-body symbol texts -> {OUT}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
