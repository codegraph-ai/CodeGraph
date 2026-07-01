#!/usr/bin/env python3
"""Server-side retrieval eval: drive the real codegraph-server MCP and score
`symbol_search` (the actual BM25+semantic hybrid) for static vs BGE.

For each model: spawn an isolated server (temp HOME, symlinked fastembed cache),
MCP-handshake, force-reindex, wait for embeddings, then run doc->symbol queries
through `codegraph_symbol_search` and score recall@{1,5,10} + MRR.

    python scripts/server_eval.py [N_QUERIES=300]
"""
import json, os, shutil, subprocess, sys, tempfile, time
from pathlib import Path

REPO = "/Users/anvanster/projects/codegraph"
SERVER = f"{REPO}/target/release/codegraph-server"
CORPUS = "/tmp/codegraph_eval_corpus.json"
CACHE = os.path.expanduser("~/.codegraph/fastembed_cache")
N = int(sys.argv[1]) if len(sys.argv) > 1 else 300


def run_eval(label, model, static_path=None):
    home = tempfile.mkdtemp(prefix="cgse.")
    os.makedirs(f"{home}/.codegraph", exist_ok=True)
    os.symlink(CACHE, f"{home}/.codegraph/fastembed_cache")
    env = dict(os.environ, HOME=home, FASTEMBED_CACHE_DIR=CACHE, RUST_LOG="info")
    if static_path:
        env["CODEGRAPH_STATIC_MODEL"] = static_path
    logf = open(f"{home}/server.log", "w+")
    proc = subprocess.Popen(
        [SERVER, "--mcp", "-w", REPO, "--embedding-model", model, "--full-body-embedding"],
        stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=logf, env=env, text=True, bufsize=1,
    )

    def send(o):
        proc.stdin.write(json.dumps(o) + "\n"); proc.stdin.flush()

    def recv(want, timeout=120):
        t0 = time.time()
        while time.time() - t0 < timeout:
            line = proc.stdout.readline()
            if not line:
                return None
            line = line.strip()
            if not line:
                continue
            try:
                m = json.loads(line)
            except Exception:
                continue
            if m.get("id") == want:
                return m
        return None

    send({"jsonrpc": "2.0", "id": 1, "method": "initialize",
          "params": {"protocolVersion": "2024-11-05", "capabilities": {}, "clientInfo": {"name": "eval", "version": "1"}}})
    recv(1)
    send({"jsonrpc": "2.0", "method": "notifications/initialized"})
    send({"jsonrpc": "2.0", "id": 2, "method": "tools/call",
          "params": {"name": "codegraph_reindex_workspace", "arguments": {"force": True}}})
    recv(2, timeout=120)

    print(f"[{label}] indexing + embedding...", flush=True)
    t0 = time.time()
    while time.time() - t0 < 1800:
        if "embedding generation complete" in open(f"{home}/server.log").read():
            break
        time.sleep(5)
    embed_secs = time.time() - t0
    print(f"[{label}] embeddings ready in ~{embed_secs:.0f}s", flush=True)

    corpus = json.load(open(CORPUS))[:N]
    r1 = r5 = r10 = mrr = 0
    nid = 100
    for item in corpus:
        nid += 1
        send({"jsonrpc": "2.0", "id": nid, "method": "tools/call",
              "params": {"name": "codegraph_symbol_search",
                         "arguments": {"query": item["doc"], "limit": 10, "compact": True}}})
        m = recv(nid, timeout=60)
        names = []
        if m and "result" in m:
            res, data = m["result"], None
            try:
                if isinstance(res, dict) and "content" in res:
                    data = json.loads(res["content"][0]["text"])
                elif isinstance(res, dict) and "results" in res:
                    data = res
                elif isinstance(res, str):
                    data = json.loads(res)
            except Exception:
                data = None
            if data:
                names = [r.get("symbol", {}).get("name") for r in data.get("results", [])]
        rank = next((i + 1 for i, nm in enumerate(names) if nm == item["id"]), None)
        if rank:
            r1 += rank <= 1
            r5 += rank <= 5
            r10 += rank <= 10
            mrr += 1.0 / rank
    n = len(corpus)
    proc.terminate()
    try:
        proc.wait(timeout=10)
    except Exception:
        proc.kill()
    shutil.rmtree(home, ignore_errors=True)
    print(f"RESULT {label}: R@1 {r1/n:.3f}  R@5 {r5/n:.3f}  R@10 {r10/n:.3f}  "
          f"MRR {mrr/n:.3f}  (n={n}, embed {embed_secs:.0f}s)", flush=True)


if __name__ == "__main__":
    run_eval("BGE-small", "bge-small")
    run_eval("static", "static", static_path=os.path.expanduser("~/.codegraph/static_models/jina-code-static-256"))
