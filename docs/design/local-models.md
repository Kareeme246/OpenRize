# Local AI models

Status: design, reviewed 2026-11-03 (first draft 2026-11-02). Target: 0.10.0 (a new system, so a
minor bump). Windows first, macOS on the same code.

## Why

Categorisation on Windows is rules-only today: no LLM, and no embeddings either, so kNN and the
personal model (T1) are missing too, and every `embed` job fails its three attempts
(`ai/worker.rs` `embed_only`). macOS has Apple's on-device model through the Swift sidecar, which
does not exist anywhere else.

This pass gives every platform a real model **that runs on this machine**: a short, curated list
of small quantized models, chosen in-app the way dictation apps offer Whisper sizes, run by a
runtime we bundle. On a Mac, the user can switch between Apple's model and a local one. Windows
and macOS use the same code.

What does not change: the worker loop, the three tiers, the arbiter, calibration, the prefill and
auto-approve thresholds, and the Swift sidecar's protocol. This design adds providers beneath the
worker; it does not redesign how votes become suggestions.

### Scope

| In this pass | Later, designed for but not built now |
| --- | --- |
| Bundled `llama-server` runtime | Bring-your-own OpenAI-compatible endpoint (see "Later") |
| Curated catalog, picked by the golden-set benchmark | Importing your own GGUF file |
| One active LLM, Apple or local, switchable | Decision models (Jev, Clef, Kev) for ranking labels |
| Bundled fallback models (embedder, personal model) | Remote catalog refresh (not planned) |
| Model step in onboarding | |

## The one decision everything follows from

**The local runtime is `llama-server` (llama.cpp), run as a sidecar that speaks the OpenAI API on
loopback.** To the rest of the app a local model is then an OpenAI-compatible base URL, a model
name and a key: one HTTP client, one prompt, one JSON schema, one validator. That is also what
keeps a later bring-your-own endpoint cheap: it is the same client pointed at a different URL, so
the architecture stays compatible with it without building it now.

Why llama.cpp: MIT; GPU on both platforms through one codebase (Metal on Apple Silicon, Vulkan on
Windows integrated and discrete GPUs, CPU fallback everywhere); ~25-40 MB per platform; GGUF is
the format every small model ships in; `response_format: json_schema` is enforced by grammar, so
output is constrained like Apple's guided generation; `/v1/embeddings` covers T1.

Rejected: `llama-cpp-2` in-process (CMake/Vulkan SDK in our build, a model crash kills the
tracker, and it would not share the HTTP client a later endpoint needs); mistral.rs and candle (no
iGPU path on Windows); ONNX Runtime GenAI / Windows ML / Foundry Local (Windows-only, ONNX models,
Microsoft catalog); Phi Silica / Aion (Copilot+ NPUs or 6 GB VRAM only, an API in flux until
2027); MLX (Mac-only); Ollama as a dependency (not something we install).

## Architecture

```
                 ai-worker (unchanged loop, rules, arbiter, calibration)
                         |                                  |
           LLM: trait Provider (T2)              embedder + personal model (T1)
       one active, chosen by the user             bundled, never a user choice
          +--------------+--------------+                   |
          |              |              |                   |
    AppleProvider  OpenAiProvider   (none: rules + T1)      |
  (Swift sidecar,  (HTTP, reqwest)                          |
   mac, unchanged)       |                                  |
                         +-----------------+----------------+
                                           |
                          RuntimeManager -> llama-server on 127.0.0.1:<random>,
                                            --api-key <random>
                                           |
                          ModelStore (bundled catalog, downloads, sha256-verified,
                                      <app local data>/models)
```

### `Provider` trait (Rust, app crate)

The seam already exists: the worker only needs `classify`, `embed` and capabilities
(`worker.rs` `llm()` / `vector_for()` / `probe()`). Make it a trait, keep the vote contract the
arbiter already consumes:

```rust
pub trait Provider: Send {
    fn capabilities(&mut self) -> Capabilities;          // llm, embed (+dimension, model tag), personal, egress
    fn classify(&mut self, req: &LlmRequest, samples: u8, temperature: f32) -> Result<ClassifyOut, ProviderError>;
    fn embed(&mut self, texts: &[String]) -> Result<Embeddings, ProviderError>;
    fn id(&self) -> ProviderId;                          // feeds model_version, see "Versioning"
}
```

- `AppleProvider` wraps today's `Sidecar` unchanged (protocol version untouched).
- `OpenAiProvider` implements classify by running `samples` requests and counting votes exactly as
  the Swift side does (unknown name = no vote, unknown project = `"none"`). In this pass it only
  ever talks to our own `llama-server`.
- **One LLM is active at a time**, and the user chooses it: on a Mac, Apple's model or a local
  one; on Windows, a local one or none. Two LLMs never run together.
- **Fallback models are bundled, not chosen.** The embedder behind kNN and, from Phase 4, the
  personal classifier ship with the app. They are part of the engine, not a setting: the user picks
  an LLM and nothing else. Internally the worker still holds the embedder separately from the LLM,
  so switching LLMs never touches the vectors.
- The personal model (`train`/`personal_predict`) is CreateML-only today. Phase 4 replaces it with
  a pure-Rust classifier over embeddings in `openrize-core` so T1 is identical on both platforms;
  until then `Capabilities.personal` is false off the Apple sidecar and the arbiter already
  re-weights.

## Classification prompt and output contract

### One prompt and one schema, in Rust

Port the instructions and prompt from `swift/Sources/openrize-ml/Language.swift` into
`openrize-core/src/ai/prompt.rs` so every provider (including, later, the Swift one via its
`classify` params) sends the same words. The prompt carries a `PROMPT_VERSION` constant; any
wording change bumps it, and it is part of `model_version`, so `metrics.rs` can compare accept
rates before and after a prompt change.

Inputs (all already built by the worker today, unchanged):

| Input | Source | Limit |
| --- | --- | --- |
| Block features | `features::extract` (apps by share, titles, domains, local time, previous category) | as today |
| Categories | live, unarchived; `name`, `description`, `ai_prompt` | `MAX_LLM_CATEGORIES` = 16, shortlisted by `shortlist()` |
| Projects (optional) | live, active; `name`, `description`, `ai_hints` | `MAX_LLM_PROJECTS` = 15, plus "No project" |
| Few-shot examples | nearest approved entries, corrections first | `FEW_SHOT_EXAMPLES` = 4 |
| Custom instructions | `settings.ai_custom_prompt` | `MAX_CUSTOM_PROMPT_CHARS` |

Duplicate display names get the same `" (2)"` suffixes as `uniqueNames` in Swift, so a name maps
back to exactly one id.

Output schema (OpenAI strict-mode compatible: flat, all required, `additionalProperties: false`):

```json
{ "description": "string", "category": { "enum": ["<unique category names>"] },
  "project": { "enum": ["<unique project names>", "No project"] } }
```

`project` is omitted from the schema when projects are not requested. `description` keeps the
Swift field description (3 to 15 words, past-tense verb, one specific thing).

### Request and validation

`POST /v1/chat/completions`, `stream: false`, `temperature` 0 (0.9 for the disagreement re-ask),
`max_tokens` 200, `response_format: {type: "json_schema", ...}`, and for Qwen-family models
`chat_template_kwargs: {enable_thinking: false}`. This is the subset every OpenAI-compatible server
accepts, so a later endpoint needs no second request shape.

Never trust the server, even our own. Each sample goes through one validator:

| What came back | What we do |
| --- | --- |
| Valid JSON, names in the live lists | One vote per field, mapped back to ids |
| `<think>...</think>` before the JSON | Strip, then validate |
| JSON wrapped in prose or a code fence | Take the first balanced `{...}`, then validate |
| A category name not in the list | No category vote for this sample (as Swift does) |
| A project name not in the list | Counts as "No project" (as Swift does) |
| Description empty, over 15 words or over 120 characters | Dropped; the template description stays |
| Schema rejected (400) | Retry once with `json_object` plus the names listed in the prompt |
| Unparseable twice, timeout, 5xx, refused | Provider error: this job falls back to T0 + T1, the existing backoff applies |

Votes are counted exactly as from the `ClassifyResult` the Swift sidecar returns today
(`categoryVotes`, `projectVotes`, `description`, `samples`, `latencyMs`), so the arbiter's
weighting and the adaptive sampling in `classify()` behave as they do now.

## Choosing the models: the golden-set benchmark first

**No model is picked until the benchmark has run.** The catalog's contents, the default and the
bundled embedder all come from it.

- **Golden set:** real time blocks (consented, scrubbed of anything identifying) with the
  category and project a person chose, across a spread of category lists and work styles.
- **Harness:** an `#[ignore]`d cargo test that runs every candidate through the real worker path
  (features, shortlist, few-shot, prompt, validator, votes, arbiter) and records category accuracy,
  project accuracy, calibration error, description acceptability, latency per entry and peak RAM,
  on a reference Windows laptop (iGPU and CPU-only) and an Apple Silicon Mac. No new test framework.
- **Baseline: Apple's Foundation Model**, through `AppleProvider`, on the same blocks. A local
  model earns a catalog place by being close to or better than Apple's model on labels at a size
  and speed that fits its tier.
- The same harness picks the bundled embedder (kNN accuracy on approved neighbours) and later
  checks every model, prompt or llama.cpp bump in the PR that makes it.

Candidates going in (all Apache-2.0 or MIT, GGUF Q4_K_M for chat): Qwen3.5-0.8B (0.5 GB),
Qwen3.5-2B (1.3 GB), Qwen3-4B-Instruct-2507 (2.5 GB); embedders bge-small-en-v1.5 (Q8_0, 37 MB)
and others of similar size. Not candidates: Llama 3.2 (community licence terms), Gemma 3 /
EmbeddingGemma (Gemma Terms carry redistribution obligations).

## Model management (`ModelStore`)

### Catalog

`catalog.json` bundled with the app: for each model `id`, `revision`, display name, Hugging Face
repo **pinned to a commit**, file name, exact byte size, **SHA-256**, licence, quantisation, RAM
estimate (see Memory), context length, notes such as "thinking off", and the golden-set scores
from the last run. Hosting stays on Hugging Face (no bandwidth bill, no redistribution question);
Apache-2.0 models can be mirrored later.

**The catalog is bundled only.** A model change ships as an app update, so every entry has passed
the golden set with the exact llama.cpp build in that release. There is no remote catalog.

The catalog lists LLMs only. The embedder is not a download: it ships inside the app (below).

### Downloads

- Download over HTTP Range into `<file>.partial` (resumable across launches), verify size and
  SHA-256, then rename. Progress follows the updater pattern: a throttled snapshot in state plus a
  `model-download` event read with `useTauriEvent`; single-flight (one download at a time); cancel
  keeps the `.partial` for resume, delete removes it.
- Before starting: free space must cover the file plus 1 GB, or the download does not start and
  the picker says how much space is missing.
- Needs `reqwest`'s `stream` feature and `sha2`. Shared rustls provider setup becomes one helper
  (the updater currently installs it inline).
- Only Hugging Face's resolve URL for the pinned commit is contacted, over `https`; redirects to
  its CDN are followed, nothing else. Model downloads are the only network calls this system makes.

### Bundled fallback models

The embedder (37 MB class) ships in the installer as a Tauri resource, beside `llama-server`, so
kNN works on Windows from the first approved entry with no download and no choice to make. It is
started in its own small `llama-server` process when T1 needs it. On macOS, Apple's NLEmbedding
stays in use until the golden set shows the bundled embedder is at least as good; then both
platforms switch to it and re-embed (see Embeddings), so T1 is the same everywhere.

### Versioning

- A model's identity is `<catalog id>@r<revision>-<sha8>`.
- `model_version` (already stored on every suggestion) becomes
  `<provider>:<model identity> p<PROMPT_VERSION> emb:<embedder identity> pm:<artifact> cal:<artifact>`,
  for example `local:qwen3.5-2b@r1-3fa9c2e1 p1 emb:bge-small@r1-9c0d11aa pm:none cal:none`.
  `metrics.rs` already groups by `model_version`, so per-model accept rates come for free.
- When an app update ships a new revision of a downloaded model, the picker shows
  "Update available (1.3 GB)". Nothing downloads on its own. The new file downloads beside the old,
  the switch happens only after it verifies, and the old file is offered for deletion.
- A pinned llama.cpp bump is a code change: it re-runs the golden set in the same PR.

### Disk use

- Models live in `<app local data dir>/models/` (Local AppData on Windows, never roaming), shared
  by every local account rather than copied into `accounts/<id>`. Like `config/`, the folder is
  not data: `data_location` never counts, moves or removes it, and it is not part of any export.
- Settings shows total disk used by models and the size of each, with Delete per model. Deleting
  the active model switches to "none" (rules + T1) and says so.
- At startup, `.partial` files with no matching catalog entry, and `.partial` files older than
  30 days, are removed.
- Only one LLM needs to be on disk; after switching, the picker offers to delete the previous one.

## Local runtime (`RuntimeManager`)

### Process

- **Bundled in the installer** as a Tauri `externalBin` per platform (`tauri.windows.conf.json` /
  `tauri.macos.conf.json`), staged by `build.rs` like the `rize` sidecar: Windows x64 Vulkan
  build (+ its DLLs as `resources`), macOS arm64 Metal build; +~40 MB on Windows, +~17 MB on
  macOS. The runtime is never downloaded: executable code fetched at run time would need its own
  signing, and it must match the llama.cpp build the catalog was tested against. CUDA is not
  bundled. Pin one tested llama.cpp build number and bump it deliberately.
- Started on demand by the worker, bound to `127.0.0.1` on a random free port with a random
  per-launch `--api-key`, `--parallel 1`, `--ctx-size 4096`, `--sleep-idle-seconds` so an idle
  tracker gives the memory back (time tracking is bursty: a classify every few minutes).
- One process for the LLM and a second small one for the bundled embedder (simpler to size and
  restart than router mode). Health via `/health`; the existing `Sidecar` backoff and idle-stop
  rules (`IDLE_STOP_AFTER`) carry over; pids feed `AiRuntime::sidecar_pid` so energy attribution
  keeps working.
- Lifetime: killed on app exit (`flush_on_exit`), and tied to the app so a crash cannot orphan
  it: a Windows Job Object with kill-on-close, and on macOS a watchdog that exits the child when
  its parent goes away.
- Background manners: process priority below normal (`BELOW_NORMAL_PRIORITY_CLASS` on Windows,
  `nice 10` on macOS), `--threads` at half the physical cores (min 1) so the foreground app stays
  responsive, never `--mlock`, mmap left on so the OS can reclaim pages.
- Timeouts: first load can take 10-30 s on CPU, so the first request after a start gets 120 s;
  later classify requests use today's 90 s `TIMEOUT_CLASSIFY`.

### Quantisation

The catalog pins exactly one file per model, so there is never a quantisation choice in the UI:

- LLMs: **Q4_K_M**. The usual size/quality knee for 1-4B models; Q5/Q6 cost 20-40% more RAM for
  gains the golden set must show before we pay them. Below 4 bits (Q2/Q3, IQ2/IQ3) small models
  lose label accuracy quickly, so they are not offered.
- Embedder: **Q8_0**. It is tiny, and quantisation error shows up directly in cosine similarity.

### Memory limits

- RAM estimate per model, stored in the catalog: file size + KV cache at `--ctx-size 4096` (from
  the GGUF header: layers x KV heads x head dim x 2 x 2 bytes x 4096) + 300 MB runtime overhead.
  For a 2B Q4_K_M model that is about 1.9 GB.
- Load preflight: available memory from the OS (`GlobalMemoryStatusEx` on Windows,
  `host_statistics64` on macOS) must exceed the estimate plus 1.5 GB of headroom. If not, the
  runtime does not start, status says `llm: unavailable (insufficient_memory)` with the numbers,
  and the job runs on T0 + T1. The next job checks again; nothing thrashes swap.
- On Apple Silicon and Windows iGPUs the model shares system memory, so the same check covers
  GPU offload. On discrete GPUs, `-ngl` full offload is tried first; a load failure retries once
  on CPU.

### GPU and power

- Vulkan/Metal by default, a "Use GPU" switch (CPU only) for broken iGPU drivers, and an automatic
  CPU retry when the GPU backend fails to load. A CPU fallback is reported, never silent.
- Runs under the power gate: background classification already respects idle/AC/battery saver
  for retraining (`power.rs`); local inference adds a "pause local model on battery saver" check.
  Paused jobs stay queued.

### Privacy

Apple's sidecar and our `llama-server` are both on device, so both get today's full prompt,
including the titles of private windows (redacted from storage after classification,
`core/src/activity/redact.rs`). Every provider still reports an egress class (`on_device` here),
so the endpoint rules under "Later" slot in without touching the prompt builder again. The "nothing
here makes a network call" note in `ai/mod.rs` becomes "nothing here makes a network call except
model downloads".

## Embeddings and kNN

Embeddings come from the bundled embedder (`/v1/embeddings`) or, on a Mac until the switch, the
Apple sidecar. Vectors are tagged with their model (`entry_embeddings.model`, already stored); kNN
already skips mismatched dimensions, so switching embedders orphans old vectors. On a change,
re-embed in the background (oldest-first, capped per run, under the power gate) and keep using
rules + LLM until enough vectors exist. With no embedder, `embed` jobs are not queued at all
(fixes today's wasted retries on Windows).

## With no LLM installed (fallback)

The app must stay fully useful with no LLM at all:

| Setup | Tiers that run | What the user sees |
| --- | --- | --- |
| No LLM chosen | T0 rules + kNN (bundled embedder) | "Suggestions use your rules and similar past entries" |
| LLM temporarily unavailable (out of memory, paused on battery, runtime failed) | T0 + T1 | The model's row shows the reason; entries handled in fallback are re-queued when it recovers (`store::requeue_fallback`, as when Apple's model becomes ready today) |
| LLM ready | T0 + T1 + T2 | Normal suggestions |

Rules: never block tracking or the UI on a model; never switch to a different LLM on the user's
behalf; every fallback is visible in status, and entries are re-checked when a better tier comes
back, within the existing `BACKFILL_WINDOW_MS`.

## Model picker and honest capability reporting

### Status

`AiStatus` drops Apple vocabulary: `provider` (`apple` | `local` | `none`), `llm` (`ready` |
`loading` | `downloading` | `paused` | `unavailable` + reason code), `embed` (`ready` |
`reembedding` + progress | `none`), `personal`, `runtime` (`running` | `stopped` |
`unavailable`), `device` (`gpu` | `cpu` + fallback reason). Reason codes are an enum the frontend
maps to sentences, like `llmUnavailableReason` does today. The frontend's `onDeviceMlUnavailable`
becomes "no LLM", which also re-enables category AI hints and custom instructions whenever any LLM
is active (today they are hidden whenever the sidecar is missing).

### Settings > AI

Settings: `ai_llm` (`apple` | `local` | `none`), `local_model`, `use_gpu`,
`pause_local_on_battery_saver`. Flat, `#[serde(default)]`, like the rest.

UI: one LLM picker (Apple's model on supported Macs, the catalog models, or None), the active one
marked, with download progress, cancel and delete. Platform differences go through
`src/lib/platform.ts` records. Each model row states, without marketing:

| Field | Source | Shown as |
| --- | --- | --- |
| Where it runs | egress class | "On this device" |
| What it does | capabilities | Category, Project, Descriptions (each yes/no) |
| Size on disk | catalog | "1.3 GB" |
| Memory | RAM estimate vs available now | "Needs about 1.9 GB, 6.2 GB free" or "Won't fit right now" |
| Benchmark | golden set, against Apple's model | "Matched 84% of categories in our test set (Apple's model: 86%)" |
| Speed on this machine | self-test after download | "About 2 s per entry (GPU)" or "About 9 s per entry (CPU)" |
| Your results | `metrics.rs`, per `model_version` | Accept rate once 20+ suggestions were reviewed |
| Licence | catalog | "Apache-2.0" |

The **Recommended** badge is a hint, not a gate: the catalog model the golden set names for this
machine's memory and GPU. Every model can still be chosen; a model that does not fit says so
before download, not after.

The self-test runs once after download (and after a GPU setting change): load time, one classify
against the live categories, and the device actually used. Its result is the speed line above.

### Onboarding and defaults

- **New Windows users get a model step in onboarding**: the catalog with the Recommended model
  first, its size and memory need, and a "Not now" that leaves them on rules + kNN. Nothing
  downloads without a click; the download continues in the background after onboarding ends.
  Macs that can never run Apple's model get the same step.
- Existing Windows users, who never saw that step, get a one-time banner pointing to Settings > AI.
- macOS with Apple Intelligence: Apple's model stays the default (unchanged); local models are a
  choice in Settings > AI.

## Later: bring your own endpoint (designed, not built in this pass)

Not built now: the privacy, security, convenience and cost trade-offs of sending blocks to a
server we do not run need their own decision. The design keeps it a small addition:
`OpenAiProvider` already speaks the request subset every OpenAI-compatible server accepts, so an
endpoint is an `Endpoint::Custom` beside `Endpoint::Local`, plus settings and UI.

- Settings: base URL, model name, JSON-mode fallback, display name. The API key in the OS
  credential store (`keyring-core` with `windows-native-keyring-store` /
  `apple-native-keyring-store`), never in the webview, the settings file, the database or a log.
  A "Test connection" button sends one tiny classify and shows latency and whether structured
  output worked; `GET /v1/models` fills a model dropdown where the server offers it.
- Network policy: only the configured base URL is contacted; `https` required except for loopback
  hosts; explicit timeouts. An endpoint that fails is never silently replaced by another provider.
- **Privacy: endpoint prompts are stripped by default.** Every configured endpoint, `localhost`
  included, gets the egress class `endpoint`: private-window detail is removed from its prompt
  (app name only), and failed jobs hold no private detail for retries. An **advanced, per-endpoint
  "this runs on my device" switch, off by default**, lets someone who runs their own local server
  send the full prompt, behind clear warning text ("only turn this on if you know this server
  never forwards what it receives"). The endpoint form says plainly what is sent and that the
  provider's privacy policy then applies.

## Later: decision models (Jev, Clef, Kev)

Not part of this pass. Decision models take a `state` plus typed `choice` questions with the
labels and return a probability for every label from one forward pass, with no text generated.
TypeSafe's Jev is closed and hosted; Cloudflare's Clef / Clef-Flash (Apache-2.0, 27B / 9B, Q4
GGUF 6.7 GB for Flash) and the smaller Kev (0.8B / 4B) are open. llama.cpp serves them through
`/v1/systemone` (ggml-org/llama.cpp#29818, #29831, both merged), so the bundled runtime could run
a local one later. They write no descriptions, and no published result is on time-tracking data.

The likely future role is **ranking** rather than replacing the LLM: ordering the live categories
and projects for `shortlist()` before the LLM sees them, or as one more tier the arbiter weighs.
If one is ever offered as a user choice, it goes through the same golden set as the LLMs.

## Where it lives

Both sides: Rust owns providers, the runtime, downloads and all network I/O (`src-tauri/src/ai/*`;
pure logic such as prompt/schema/validation and the catalog types in `openrize-core`); React owns
the onboarding step, the picker and status (`src/pages/Settings.tsx` AI section,
`src/components/Onboarding.tsx`, a new `src/components/ai/*`). The `rize` CLI gets read-only
`rize ai status` and `rize ai models` (and `COMMANDS.md`).

## Phases

1. **Seam, runtime and benchmark.** `Provider` trait, `AppleProvider`, Rust prompt/schema/
   validator, `OpenAiProvider` against the bundled `llama-server`, `RuntimeManager`, generalized
   `AiStatus`, no `embed` jobs without an embedder, and the golden-set harness with Apple's model
   as baseline. The benchmark run picks the catalog and the embedder.
2. **Fallback models.** The bundled embedder, embeddings through it, re-embed on change, kNN on
   Windows.
3. **Local LLMs for users.** `ModelStore` with the benchmarked catalog, verified downloads, the
   picker with honest rows and the self-test, the onboarding step, memory preflight, GPU/CPU
   fallback, power gate.
4. **T1 everywhere.** Pure-Rust personal classifier replacing CreateML on both platforms.

Later, each its own decision: GGUF import (after the catalog has proven out), bring-your-own
endpoint, decision models for ranking.

## Risks

- Small models can return valid JSON with the wrong label: the arbiter's calibration caps
  confidence, prefill needs 0.60, and the golden set decides which models are offered at all.
- The benchmark is only as good as the golden set: it needs enough real, varied, consented blocks
  before it can pick anything.
- Qwen thinking mode must stay off, or it burns tokens and fights the grammar.
- Vulkan driver bugs on old iGPUs: automatic CPU retry plus the switch.
- Packaging: llama-server's DLLs, the bundled embedder, antivirus false positives on an unsigned
  installer (already an open issue), and a model file of up to a few GB per user.
- Memory on 8 GB machines: small tier, preflight, idle unload.
- llama.cpp moves daily: pin, test, bump on purpose.

## Decisions (settled in review, 2026-11-03)

1. **`llama-server` is bundled in the installer**, never downloaded. Models are the only download.
2. **Endpoints (later) get stripped prompts by default**, with an advanced per-endpoint opt-in to
   the full prompt behind clear warning text.
3. **Local models first; bring-your-own endpoints later**, and not in this pass. The architecture
   stays compatible with adding them.
4. **Fallback models (the kNN embedder, the personal model) are bundled and not a user choice.**
   Only the LLM is user-selectable, from a curated set chosen after testing.
5. **One active LLM at a time**; the user switches between Apple's model and a local one.
6. **The catalog is bundled with the app only.**
7. **GGUF import comes later**, after the catalog has proven out.
8. **No decision models in this pass**; a possible later use is ranking labels.
9. **New Windows users get a model step in onboarding.**
10. **No model is picked until the golden-set benchmark runs**, with Apple's models as the
    baseline.

## Open questions (not blocking 0.10.0)

- When the bundled embedder matches NLEmbedding, switch macOS to it so T1 is identical everywhere?
- CUDA builds for NVIDIA users as an optional download: only after Vulkan numbers are in.
- Mirroring catalog models on our own storage once usage justifies it.

## Delivery plan

One PR per row, merged in order, each verified with `pnpm verify`, unit tests and a driven run
on Windows (and the macOS paths it touches reviewed by hand). Each user-facing PR bumps the patch
version; the first one is the 0.10.0 minor bump.

| PR | Version | Contents |
| --- | --- | --- |
| 1 | - | `Provider` trait; `AppleProvider` wrapping today's sidecar unchanged; prompt (`PROMPT_VERSION`), schema and validator ported to `openrize-core`; `OpenAiProvider` and a minimal `llama-server` process handle; golden-set harness (`#[ignore]`d cargo test, `ai/bench.rs`) with Apple's model as baseline. No user-facing change |
| 2 | 0.10.0 | Generalized `AiStatus` + frontend; stop queueing `embed` jobs with no embedder |
| 3 | 0.10.1 | `RuntimeManager` and the bundled `llama-server` (Windows Vulkan + CPU, macOS Metal): Job Object / watchdog lifetime, priority, idle unload, GPU/CPU fallback; `OpenAiProvider` wired into the worker. The golden-set run on the real set picks the catalog and the embedder |
| 4 | 0.10.2 | Bundled embedder, embeddings through it, background re-embed on change, kNN on Windows |
| 5 | 0.10.3 | `ModelStore`: benchmarked catalog, resumable SHA-256-verified downloads, free-space check, progress events, delete, `.partial` cleanup; memory preflight; power gate |
| 6 | 0.10.4 | LLM picker in Settings > AI with honest rows, self-test, onboarding model step, one-time banner for existing Windows users. Local models usable end to end from here |
| 7 | 0.10.5 | Pure-Rust personal classifier over embeddings in `openrize-core`, replacing CreateML for T1 on both platforms |
