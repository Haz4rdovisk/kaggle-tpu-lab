# kaggle-tpu-lab

**Serve Qwen3.8-27B — a frontier-class 27B hybrid-attention model — on Kaggle's free
TPU v5e-8, with a public OpenAI-compatible endpoint for Claude Code, Codex CLI,
opencode, or anything else that speaks the OpenAI API.**

> **Fork** of [ARahim3/kaggle-tpu-lab](https://github.com/ARahim3/kaggle-tpu-lab)
> with a **Desktop Companion** (Windows tray app) added. Upstream recipe, datasets
> and measurements are ARahim3's — see Credits.

No paid GPU, no cloud account, no quantization. Full bf16 weights, up to the model's
native **262,144-token context**, and real speed:

| What | Measured (TPU v5e-8, bf16, TP=8) |
|---|---|
| Decode, single stream | **~130 tok/s** with MTP speculative decoding (78 without) |
| Decode, 8 concurrent streams | **~540 tok/s aggregate** (~107 tok/s each, MTP on) |
| Decode, 16 concurrent streams | **~900 tok/s aggregate** (with `--mtp 0` — see tuning note) |
| Prefill | **10,300 tok/s** — a 105k-token prompt in ~10 s |
| Native 262k context | works — 225k-token prompt prefilled in ~28 s |
| Time to live endpoint | **~22 min** with the env dataset (~12 with `--text-only`, ~6 with `--fast-start`; ~40 without) |
| Output correctness with MTP | **verified lossless** — 12/12 greedy prompts match non-speculative |

Tuning note: speculative decoding pays off up to ~8 concurrent streams and fades beyond
that. Serving many users? Launch with `--max-model-len 131072 --max-num-seqs 16 --mtp 0`.

## Why this works (the one-paragraph version)

Qwen3.8-27B is a hybrid: 48 of its 64 layers are **gated-DeltaNet linear attention**,
only 16 are classic full attention — so its KV cache is tiny (~64 KB/token) and 131k+
contexts fit on 8×16 GB TPU chips. [vllm-tpu](https://github.com/vllm-project/tpu-inference)
0.28.0 shipped native Pallas kernels for the DeltaNet layers, and this repo is the recipe
that puts it together on Kaggle's free tier: pinned runtime, pre-mirrored weights,
pre-built XLA compile cache, MTP speculative decoding, and a tunnel to the outside world.

## Quick start A — as a Kaggle notebook

**Copy & Edit** the published notebook and Run it —
[**kaggle.com/code/rahim3/qwen3-8-27b-bf16-on-kaggle-tpu-130-tok-s-api**](https://www.kaggle.com/code/rahim3/qwen3-8-27b-bf16-on-kaggle-tpu-130-tok-s-api)
— or upload [`notebook/qwen38-tpu-serve.ipynb`](notebook/qwen38-tpu-serve.ipynb) yourself.
Set **Accelerator = TPU VM v5e-8**, **Internet = ON**, attach the two datasets named in
the first cell, and run top to bottom. The last cell *is* the server.

## Quick start B — from your terminal

You need Python 3.9+ and a Kaggle account with **TPU access** (free tier: ~20 TPU
hours/week; phone-verify in Settings if needed).

```bash
pip install kaggle   # one-time; place kaggle.json from Settings → API
git clone https://github.com/Haz4rdovisk/kaggle-tpu-lab
cd kaggle-tpu-lab
python launch.py serve
```

The launcher pushes a script kernel to your account and streams progress; the banner
prints the endpoint URL, API key (`qwen3.8-27b`, context 262144) when live:

```
[14:06]  Kaggle: queued — waiting for a TPU v5e-8 slot...
[14:15]  XLA compile cache restored — fast start. Weights mounted.
[14:35]  Server is HEALTHY after 20 min.
  base URL : https://xxxx-yyyy.trycloudflare.com/v1
  API key  : sk-....
```

`Ctrl-C` detaches without stopping. `python launch.py status -f` re-attaches;
`python launch.py stop` kills the session. Useful flags (default: full 262k, 4 seqs):

```bash
python launch.py serve --max-model-len 131072 --max-num-seqs 16  # ~900 tok/s aggregate
python launch.py serve --reasoning-effort medium
python launch.py serve --keepalive-min 120
python launch.py serve --text-only    # ~10 min faster, no image inputs
python launch.py serve --fast-start   # live in ~6 min
```

## Using it with coding agents

Standard OpenAI API with tool calling (`qwen3_coder` parser) and the `qwen3` reasoning
parser. Any client works with `OPENAI_BASE_URL` + `OPENAI_API_KEY` (model `qwen3.8-27b`).
**Claude Code**: vLLM also serves Anthropic-compatible `/v1/messages` (verified
end-to-end, thinking as proper blocks) — authenticate with `ANTHROPIC_AUTH_TOKEN`
(Bearer-only server), **not** `ANTHROPIC_API_KEY`.

Reasoning levels: **xhigh** (default), **medium**, **low** — per request via
`{"chat_template_kwargs": {"reasoning_effort": "low"}}`, off via
`{"enable_thinking": false}`, or server default via `--reasoning-effort`.

## Desktop companion (Windows tray app)

Native **Windows system-tray app** (Tauri 2 + Rust + React/TypeScript, `src/` +
`src-tauri/`). It reuses `launch.py` for every action and attaches to the session
that's already up. Window: **420×680 default, resizable 400–440 × 520–1200**.

Live panel: **phase** (Queued → … → Ready, queued is *not* an error), **uptime**/
**remaining** (`~` = estimated), **decode tok/s**, **context**, endpoint with
`LIVE`/`RESERVED` badge + copy, **Pi sync** (backup + validation + rollback, never
touches other providers), activity log, **Start** (idle/stopped/failed) / **Stop**
(confirm dialog; re-attaches instead of double-launching).

Also in this fork: **light/dark theme** (header toggle + System/Light/Dark selector in
Settings; follows the OS by default, persists in localStorage), **self-hosted
Inter/Roboto Mono** (offline, no CDN), a 3rd metric square showing **QUEUE** (live time
in queue) then **WAITED FOR** (total until READY, frozen). State comes from Kaggle
status + ntfy events + a direct `GET {endpoint}/v1/models` probe (ntfy down never marks
a running kernel dead). **The instance is only submitted when you press Start** —
opening the app just reads/re-attaches, never fabricates a submission. **The API key
never leaves Rust** (absent from UI payloads, redacted from logs).

```bash
npm install
npm run tauri dev      # dev: tray icon + panel
npm run tauri build    # → release exe + NSIS/.msi under src-tauri\target
```

## What's actually in this repo

```
launch.py / kernel/serve_qwen38.py   CLI + Kaggle kernel: runtime → cache → weights → vLLM → tunnel
notebook/qwen38-tpu-serve.ipynb      the same flow as a notebook
patches/mtp-rollback-v0280.diff      GDN state-rollback fix (port of tpu-inference PR #3178)
tools/embed_patch.py                 re-embeds the patch after edits
src/, src-tauri/, package.json       desktop companion (see above)
```

Datasets the kernel attaches: **`rahim3/qwen3-8-27b-bf16`** (55.6 GB weight mirror,
skips the HF download) and **`rahim3/qwen38-tpu-env-v5e8`** (XLA compile cache for the
documented configs + `cloudflared` + build manifest; resolution pinned to its build
date, cache ignored — never broken — on drift). Rebuild with
`python launch.py build-env` (~2 h) and version the dataset from the kernel Output tab.

## Good to know / limits

- **One TPU session at a time**, 9 h cap, ~20 TPU-hours/week; auto-stop after
  `--keepalive-min` protects your quota. Endpoint is public URL + generated key —
  treat the pair as secret; relaunches rotate both.
- **Prefix caching off** (vllm-tpu 0.28.0 disables it for hybrid models on TPU; a
  near-future bump should enable it). Multi-turn sessions re-prefill (~5 s/50k tokens).
- **Images work, one-time cost per size**: first image at a new resolution takes ~1 min
  (may HTTP-524 — retry); later ones are instant. `--text-only` drops images + ~8 min.
- **MTP is on and lossless with our patch** (12/12 greedy match, +34% decode; k=4 won't
  start). If the patch ever fails to apply, the script disables MTP rather than serve
  corruption. `--mtp 0` turns it off.
- **Harmless log noise**: `Unable to poll the TPU GCE Metadata`, `Failed to import
  from vllm._C`, `0 active Triton driver(s)`, hugepages warnings — all ignorable; full
  output goes to `vllm.log` (`--verbose` streams it).

## Credits

- **ARahim3** ([upstream repo](https://github.com/ARahim3/kaggle-tpu-lab)) — the original
  recipe, datasets, notebook and measurements this fork builds on.
- [vLLM](https://github.com/vllm-project/vllm) / [tpu-inference](https://github.com/vllm-project/tpu-inference) teams — GDN Pallas kernels and the MTP drafter.
- [Qwen](https://huggingface.co/Qwen) for Qwen3.8-27B (Apache-2.0) with a native MTP head.
- Kaggle for the free TPUs.

## License

MIT for everything in this repo. Model weights follow the upstream
[Qwen3.8-27B license](https://huggingface.co/Qwen/Qwen3.8-27B) (Apache-2.0).
