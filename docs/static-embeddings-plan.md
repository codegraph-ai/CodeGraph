# Plan: distill Jina-Code → static embeddings, validated at every step

## Goal
Replace ONNX transformer embeddings (BGE/Jina via fastembed) with a **static
model distilled from Jina-Code-V2** — ~100× faster indexing at acceptable
quality — and prove the quality at each step rather than hoping. Secondary win:
removing the `ort`/ONNX runtime kills the 1.5 GB RAM gate, the glibc-2.31 shim,
and the `fastembed` diamond conflict.

## Progress (branch `feat/static-embeddings`)
- **1.1 ✓** `Embedder` trait; `VectorEngine` holds `Arc<dyn Embedder>` (`eb60736`).
- **1.1 ✓** `StaticEmbedding` backend — model2vec format (`tokenizer.json` +
  `safetensors`), `tokenize → gather → mean-pool → L2-norm`; validated against
  the real `potion-base-8M` (`a30df5e`).
- **1.2 ✓** gated identifier-splitting in `build_embed_text`, default-off
  (`de3e45d`).
- **Speed proven** — `examples/embed_throughput.rs` (`1da5309`): static vs ONNX
  BGE-small over 512 symbol texts. Debug floor 8.6×; **release 103× — 32,986 vs
  319 texts/sec** (M4, potion-base-8M 256d vs BGE-small 384d). The embedding step
  of indexing drops ~100×.
- **Distillation works (Phase 2.2 ✓)** — `scripts/distill_static_model.py`
  distilled `jinaai/jina-embeddings-v2-base-code` → 256d in **~32 s on the M4
  CPU** (Apache-2.0 teacher); loads in Rust (F16 + SIF weights) and embeds at
  **70× BGE** (23k vs 327 texts/sec — slower than potion's 103× only because
  jina's vocab is 2× larger).
- **Quality (micro-eval) — saturated.** On the 12-query NL→symbol set
  jina-code-static-256 and the generic potion floor **tie exactly: R@1 0.92 /
  R@3 1.00 / MRR 0.958** vs BGE **1.00 / 1.00 / 1.000** (~95% of BGE). Both
  always land the answer in the top 3, so the set **cannot distinguish a code
  teacher from a generic one** — it proves the static path is real, fast, and
  ~95% of BGE, but is too easy to measure the code-teacher delta.
- **Real eval (965-way, pure-semantic) — the micro-eval was misleading.**
  `scripts/extract_eval_corpus.py` (965 doc-commented symbols) +
  `examples/embed_eval.rs` (doc→symbol retrieval, recall@k):

  | model | R@1 | R@5 | R@10 | MRR |
  |---|---|---|---|---|
  | BGE-small (ONNX) | 0.591 | 0.824 | 0.873 | 0.691 |
  | potion-base-8M (generic) | 0.378 | 0.616 | 0.696 | 0.488 |
  | jina-code-static-256 | 0.379 | 0.598 | 0.685 | 0.480 |

  (1) On a hard, unsaturated task, static is **~65% of BGE R@1 / ~70% MRR** — a
  real gap, not ~95%. (2) The code teacher **ties the generic potion** at 256d —
  plain Jina distillation at 256d is no win.
- **Lever sweep — the gap is fundamental, not tunable cheaply:**
  - **512d: no change** (R@1 0.378) — not a compression problem.
  - **code teacher vs generic potion: no change** — not a teacher problem.
  - **identifier-splitting (`CODEGRAPH_SPLIT_IDS`): +6% relative** (static R@1
    0.379→0.401, MRR 0.480→0.511; also lifts BGE 0.591→0.608) — real but modest;
    validates the Phase-1.2 lever.

  So static's ~65% of BGE is the **contextualization ceiling** (no attention),
  not dim/teacher. Only a learned pooling head addresses the root; tokenlearn may
  help modestly.
- **Hybrid eval (40% BM25 + 60% semantic) — static is viable.** `embed_eval` now
  scores both; BM25 recovers most of static's semantic gap (lexical matching is
  strongest exactly where static is weak):

  | config (id-split on) | R@1 sem → hybrid | MRR sem → hybrid |
  |---|---|---|
  | jina-code-static-256 | 0.401 → **0.547** | 0.511 → **0.656** |
  | BGE-small | 0.608 → 0.609 | 0.716 → 0.720 |

  In the **real hybrid system static is ~90% of BGE** (R@1 0.547 vs 0.609 = 90%;
  MRR = 91%; R@10 0.854 vs 0.909 = 94%) — at ~70–100× indexing speed. BGE barely
  uses BM25 (already-strong semantics); static leans on it (+36% R@1). Caveat:
  a faithful *approximation* of `symbol_search`'s blend; the exact server number
  needs the server-side eval.
- **Verdict:** static embedding is a **viable default/opt-in** — ~90% of BGE
  end-to-end quality, ~100× faster indexing, none of the ONNX / 1.5 GB RAM-gate /
  glibc / fastembed baggage. The code teacher ties the generic potion (no edge,
  but Apache-clean). Closing the last ~10% would need a learned pooling head;
  ~90% at 100× is a strong default trade. Next: wire static as a selectable
  `--embedding-model` + the server-side hybrid eval to confirm the exact number.

## Why this is worth doing (grounded in history)
The original static model (`migration.rs` v3) was `potion-base-8M`: a **generic
256d** static model, fed **raw identifiers**, via stock `encode()`. It lost to
BGE because it was the *weakest* static config vs an eventually code-specialized
transformer — not because static is unworkable. A **code-teacher** (Jina-Code),
at **full dim**, with **split identifiers** has never been tried. The distilled
model runs at static speed (~8000 samples/sec) regardless of how slow the Jina
*teacher* is — the teacher is used once, offline, to build the lookup table.

## Operating principles (the validation philosophy)
1. **Eval-first.** Build the measuring stick (Phase 0) before any embedding work.
   It is the single source of truth; every later step is one row on a scoreboard.
2. **ONNX stays a selectable fallback** the whole way. No bridges burned — a
   `--embedding-model` value keeps BGE/Jina available for a "max-quality" mode.
3. **One lever at a time.** Never bundle changes; you won't know what helped.
4. **Two validation levels per model:**
   - *Distillation-level* (in Python, before codegraph): does the static model
     reproduce the teacher's geometry? (correlation of pairwise cosines)
   - *Task-level* (codegraph): code-retrieval recall@k / MRR on a real repo.
5. **Stop rule.** Ship when a config reaches the quality bar (e.g. ≥95% of BGE
   recall@5) or the levers stop paying; don't gold-plate.

---

## Phase 0 — Evaluation harness + baselines (the measuring stick)
**Objective:** a deterministic code-retrieval eval and a baseline scoreboard.

- **0.1 Build a labeled eval set** — ~150–300 `(query → expected symbol(s))`
  pairs on a real indexed repo (use this repo + one large external repo).
  Sources, cheapest first: docstring → its symbol; test name → tested symbol;
  commit subject → changed symbol; ~50 hand-curated natural-language queries.
  - **Validate:** ≥150 queries; every target symbol exists in the indexed graph
    (assert lookups resolve); eyeball 10 pairs for sanity.
- **0.2 Eval runner** — for each query, run `symbol_search` (ai_query/engine.rs)
  and compute recall@{1,5,10} and MRR; also record embed-throughput (samples/s)
  and full-repo index wall-clock + peak RAM.
  - **Validate:** re-running gives identical numbers (deterministic); a trivially
    correct query (exact symbol name) scores recall@1 = 1.0.
- **0.3 Baseline table** — run the eval for every current option:
  BGE-small-384d (default), Jina-Code-768d, and — if reconstructable —
  potion-base-8M-256d as the floor.
  - **Validate:** a committed scoreboard (model → recall@k, MRR, throughput,
    index-time, RAM). These are the numbers to beat / match.

**Gate to Phase 1:** baselines captured and reproducible.

---

## Phase 1 — Rust static plumbing, proven with an off-the-shelf model
**Objective:** make the static path real and correct *before* custom distilling.

- **1.1 `Embedder` trait + `StaticEmbedding` backend** behind `VectorEngine`
  (embedding/engine.rs), using the `model2vec` Rust crate (already used
  historically). Select via a new `CodeGraphEmbeddingModel::StaticCode` variant;
  fastembed remains default.
  - **Validate (unit):** loads a model dir; `embed("test")` returns the expected
    dim; `embed_batch == map(embed)`; cosine(x, x) = 1.0; cosine(x, ⟂) ≈ 0.
- **1.2 Identifier-splitting into `build_embed_text`** (ai_query/engine.rs) —
  reuse the existing splitter used by the BM25 text index
  (ai_query/primitives.rs / text_index.rs). Gate it to the static path (and
  optionally the transformer path behind a flag).
  - **Validate (unit):** `build_embed_text` for `authenticate_user` yields tokens
    containing `authenticate` and `user`; snapshot 5 representative symbols.
- **1.3 Eval with off-the-shelf `potion-retrieval-32M`** through the new path
  (with and without id-splitting).
  - **Validate (task):** scoreboard rows for {potion-retrieval, ±id-split} vs the
    BGE baseline and the potion-base-8M floor. This proves the plumbing *and*
    quantifies how much *better-general-static + id-splitting* alone recovers.

**Gate to Phase 2:** static path is correct end-to-end; we know the id-split
delta and whether a better general static helps. (If it already ≈ BGE, great —
distillation is upside, not a requirement.)

---

## Phase 2 — Distill Jina-Code → static (offline, Python, M4 CPU)
**Objective:** a code-teacher static model, validated against the teacher before
it touches codegraph.

- **2.1 Env + teacher** — `pip install model2vec[distill]`; download
  `jinaai/jina-embeddings-v2-base-code`.
  - **License:** the teacher is **Apache-2.0** (verified). The distilled model is
    a derivative; Apache-2.0 is permissive, so the static model is redistributable
    under Apache-2.0 with attribution (§4 — credit the teacher in NOTICE / the
    model card). Use the **v2** family only — `jina-embeddings-v3` is
    CC-BY-NC-4.0 (non-commercial) and must not be the teacher.
  - **Validate:** load the teacher in Python; embed a code snippet → 768d;
    confirms it runs locally on the M4 (CPU/MPS).
- **2.2 Distill → 256d** (same dim/speed as the old potion, to isolate one
  variable: *code teacher vs generic teacher*).
  `distill(model_name="jinaai/jina-embeddings-v2-base-code", pca_dims=256)`;
  `save_pretrained("jina-code-static-256")`.
  - **Validate (distillation-level):** on a held-out set of ~500 code-snippet
    pairs, compute Spearman correlation between *static* pairwise cosines and
    *teacher* pairwise cosines. Require ρ ≥ ~0.6 (the static model preserves the
    teacher's geometry). Also measure embed throughput → confirm ~static speed
    (thousands/sec), independent of Jina's slowness.
  - **Caveat:** Jina ships custom modeling (ALiBi, `trust_remote_code=True`); if
    distillation fights it, fall back to BGE-small as the teacher (still the
    model we trust; re-validate ρ).
- **2.3 Eval `jina-code-static-256` in codegraph** (via Phase-1 plumbing,
  id-split on).
  - **Validate (task):** scoreboard vs the potion-base-8M-256 floor at **equal
    dim and speed**. A meaningful win here proves *the teacher was the problem*.

**Gate to Phase 3:** code-static-256 beats generic-static-256 on the eval.

---

## Phase 3 — Quality levers, each measured marginally
**Objective:** close the gap to the transformer baseline; keep only levers that
pay. One change → one scoreboard row.

- **3.1 Dimension → 512d** (re-distill `pca_dims=512`).
  - **Validate:** recall@k delta vs 256d; index-size/search-latency delta. Keep
    if the quality gain justifies the larger index (embed throughput barely
    changes — cost is tokenization, not pooling width).
- **3.2 SIF pooling + drop top principal component** (Rust `StaticEmbedding`
  post-process, or model2vec weighting).
  - **Validate:** marginal recall delta; keep if positive.
- **3.3 (Optional, GPU-helped) Tokenlearn corpus adaptation** on a code corpus
  (the teacher-over-corpus forward is the only GPU-helped step; CPU/overnight ok).
  - **Validate:** marginal delta vs the plain distill — is the corpus step worth
    the cost?
- **3.4 (Optional, ceiling) tiny learned pooling head** distilled to match
  Jina-Code's sentence embedding (one shallow attention/conv layer).
  - **Validate:** how much of the remaining gap to the Jina *transformer* it
    closes, at what added inference cost (must stay ≫ transformer-fast).

**Gate to Phase 4:** a chosen config meets the quality bar (define vs BGE, e.g.
≥95% recall@5) or levers plateau.

---

## Phase 4 — Integration + migration + large-repo validation
**Objective:** ship the swap safely; prove the real-world speedup.

- **4.1 Default/opt-in wiring + DB migration** — new `migration.rs` version
  (delete `vec:`, clear embeddings, re-embed on load — mirror v3→v4/v4→v5).
  - **Validate:** migration unit test (old-vectors DB migrates, re-embeds, search
    still returns results) — mirror the existing `test_migration_with_json_data`.
- **4.2 Index a large real repo on the static path** end-to-end.
  - **Validate:** index wall-clock + peak RAM vs the ONNX path (target: large
    index-time drop, no 1.5 GB gate, RAM down); eval recall holds vs Phase-3;
    confirm the `ort`/fastembed dependency is gone from the build (bonus: this is
    what unblocks the in-process Warp linking).
- **4.3 A/B + soak** — run static vs BGE on the same repo; spot-check real
  developer queries; check the *other* embedding consumers
  (`find_duplicates`, `cluster_symbols`, `find_similar`).
  - **Validate:** qualitative parity on real queries; no regression in the other
    consumers.

---

## Phase 5 — Decision gate
Final scoreboard: `static-jina-final` vs `BGE` vs `Jina-transformer` across
{recall@k, MRR, index-time, embed-throughput, RAM, index-size, deps}. Decide:
static as default? opt-in? keep ONNX as a `--embedding-model max-quality`
fallback? Record the call.

## Scoreboard template (the through-line)
| config | recall@1 | recall@5 | recall@10 | MRR | embed/s | index-time | RAM | dim | deps |
|---|---|---|---|---|---|---|---|---|---|
| BGE-small (baseline) | 0.591 | 0.824 | 0.873 | 0.691 | 327 | | | 384 | ort |
| Jina-Code (baseline) | | | | | | | | 768 | ort |
| potion-base-8M (floor) | 0.378 | 0.616 | 0.696 | 0.488 | 32986 | | | 256 | — |
| **jina-code-static-256** | 0.379 | 0.598 | 0.685 | 0.480 | 23037 | | | 256 | — |
| jina-code-static-512 | | | | | | | | 512 | — |
| + tokenlearn / + head | | | | | | | | | — |

> recall@k from the **965-way pure-semantic eval** (`embed_eval`, doc→symbol).
> The real hybrid (40% BM25 + 60% semantic) system would score higher for every
> row — this isolates *embedding* quality, where static lands ~65% of BGE R@1.

## Safety / rollback
- fastembed/ONNX stays selectable throughout; nothing is removed until Phase 5.
- Each phase is independently revertable; the eval scoreboard makes any
  regression visible immediately.
- Distillation artifacts are reproducible from the one-line `distill` command;
  no opaque state.

## First actions (no Python, no GPU needed)
1. Phase 0.1–0.3: eval harness + baselines.
2. Phase 1.1–1.2: `StaticEmbedding` trait/backend + id-splitting in
   `build_embed_text`.
3. Phase 1.3: off-the-shelf `potion-retrieval-32M` through the eval.
These isolate the cheapest wins and stand up the measuring stick before any
distillation.
