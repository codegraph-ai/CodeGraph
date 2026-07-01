#!/usr/bin/env python3
"""Extract a (name, signature, doc) retrieval-eval corpus from Rust source.

Scans `<root>/**/*.rs` for doc-commented items (`///` lines, optionally followed
by `#[..]` attributes, then `pub fn|struct|trait|enum NAME`). Emits JSON
`[{id, signature, doc}]` for items whose doc has >= MIN_DOC_WORDS words.

The eval (examples/embed_eval.rs) embeds `"name: signature"` as the symbol and
the **doc as the query** (the symbol text excludes the doc), so it's a real
natural-language-description -> code-signature retrieval task. The same task is
run for every embedder, so the *comparison* is fair regardless of difficulty.

    python scripts/extract_eval_corpus.py [ROOT=crates] [OUT=/tmp/cg_eval.json]
"""
import json
import re
import sys
from pathlib import Path

MIN_DOC_WORDS = 6
ROOT = Path(sys.argv[1]) if len(sys.argv) > 1 else Path("crates")
OUT = Path(sys.argv[2]) if len(sys.argv) > 2 else Path("/tmp/codegraph_eval_corpus.json")

ITEM_RE = re.compile(
    r"^\s*(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?(?:fn|struct|trait|enum)\s+([A-Za-z_][A-Za-z0-9_]*)"
)


def main() -> int:
    corpus: dict[str, dict] = {}
    for f in ROOT.rglob("*.rs"):
        p = str(f)
        if any(seg in p for seg in ("/tests/", "/examples/", "/benches/")):
            continue
        doc: list[str] = []
        for line in f.read_text(errors="ignore").splitlines():
            s = line.strip()
            if s.startswith("///"):
                doc.append(s[3:].strip())
            elif s.startswith("#["):
                pass  # attribute between doc and item — keep the doc block
            elif s == "":
                doc = []
            else:
                m = ITEM_RE.match(line)
                if m and doc:
                    name = m.group(1)
                    sig = s.rstrip().rstrip("{").rstrip()
                    doctext = " ".join(d for d in doc if d and not d.startswith("#"))
                    if name not in corpus and len(doctext.split()) >= MIN_DOC_WORDS:
                        corpus[name] = {"id": name, "signature": sig, "doc": doctext}
                doc = []
    items = list(corpus.values())
    OUT.write_text(json.dumps(items))
    print(f"extracted {len(items)} doc-commented symbols -> {OUT}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
