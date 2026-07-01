#!/usr/bin/env python3
"""Distill a transformer embedding model into a static (model2vec) model.

Produces `config.json` + `tokenizer.json` + `model.safetensors` (an
`embeddings` [vocab, dim] F32 tensor) — exactly the format codegraph-memory's
`StaticEmbedding` (`VectorEngine::with_static_model`) loads. No runtime ONNX.

Runs on CPU (M4) in minutes — the teacher is used once to build the lookup
table, then discarded. Uses `distill_from_model` with an explicit
`trust_remote_code` load so Jina-Code's custom modeling (ALiBi) doesn't fight
the higher-level `distill()` helper.

Usage:
    python distill_static_model.py [TEACHER] [PCA_DIMS] [OUTPUT_DIR]

Defaults reproduce the plan's first experiment — the Apache-2.0 code teacher,
256d (same dim/speed as the generic potion floor, isolating "code teacher vs
generic teacher"):
    python distill_static_model.py \\
        jinaai/jina-embeddings-v2-base-code 256 \\
        ~/.codegraph/static_models/jina-code-static-256
"""
import sys
from pathlib import Path


def main() -> int:
    teacher = sys.argv[1] if len(sys.argv) > 1 else "jinaai/jina-embeddings-v2-base-code"
    pca_dims = int(sys.argv[2]) if len(sys.argv) > 2 else 256
    out = (
        Path(sys.argv[3]).expanduser()
        if len(sys.argv) > 3
        else Path.home() / ".codegraph/static_models/jina-code-static-256"
    )

    print(f"[distill] teacher={teacher}  pca_dims={pca_dims}  out={out}", flush=True)

    from transformers import AutoModel, AutoTokenizer
    from model2vec.distill import distill_from_model

    # Explicit load with trust_remote_code so Jina's custom code is honored.
    model = AutoModel.from_pretrained(teacher, trust_remote_code=True)
    tokenizer = AutoTokenizer.from_pretrained(teacher)

    static = distill_from_model(model=model, tokenizer=tokenizer, pca_dims=pca_dims)

    out.mkdir(parents=True, exist_ok=True)
    static.save_pretrained(str(out))

    # Sanity: confirm the saved model embeds and has the expected dim.
    vec = static.encode(["fn authenticate_user(token: Token) -> User"])
    dim = vec.shape[-1]
    print(f"[distill] done — output dim={dim} (expected {pca_dims}); saved to {out}", flush=True)
    return 0 if dim == pca_dims else 1


if __name__ == "__main__":
    raise SystemExit(main())
