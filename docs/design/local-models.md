# Local and bring-your-own AI models

Status: design, for review (first draft 2026-11-02, completed 2026-11-03). Target: 0.10.0 (a new
system, so a minor bump). Windows first, macOS on the same code.

## Why

Categorisation on Windows is rules-only today: no LLM, and no embeddings either, so kNN and the
personal model (T1) are missing too, and every `embed` job fails its three attempts
(`ai/worker.rs` `embed_only`). macOS has Apple's on-device model through the Swift sidecar, which
does not exist anywhere else. We want two ways to get a real model on every platform:

1. **Bring your own endpoint.** Any OpenAI-compatible server: a remote provider with the user's API
   key, or a local server the user already runs (Ollama, LM Studio, vLLM).
2. **Download a model we picked.** A short, curated list of small quantized models, chosen in-app
   the way dictation apps offer Whisper sizes, run fully on this machine. A user can also import a
   model file they already have.

Both must share one code path, and macOS must use the same path as Windows (Apple's model stays
available on Macs that have it, as one choice among several).

What does not change: the worker loop, the three tiers, the arbiter, calibration, the prefill and
auto-approve thresholds, and the Swift sidecar's protocol. This design adds providers beneath the
worker; it does not redesign how votes become suggestions.

## The one decision everything follows from

**The local runtime is `llama-server` (llama.cpp), run as a sidecar that speaks the OpenAI API on
loopback.** Then "a downloaded model" and "the user's endpoint" are the same thing to the rest of
the app: an OpenAI-compatible base URL, a model name and a key. One HTTP client, one prompt, one
JSON schema, one validator.

Why llama.cpp: MIT; GPU on both platforms through one codebase (Metal on Apple Silicon, Vulkan on
Windows integrated and discrete GPUs, CPU fallback everywhere); ~25-40 MB per platform; GGUF is
the format every small model ships in; `response_format: json_schema` is enforced by grammar, so
output is constrained like Apple's guided generation; `/v1/embeddings` covers T1.

Rejected: `llama-cpp-2` in-process (CMake/Vulkan SDK in our build, a model crash kills the
tracker, and it would not share the BYO client); mistral.rs and candle (no iGPU path on Windows);
ONNX Runtime GenAI / Windows ML / Foundry Local (Windows-only, ONNX models, Microsoft catalog);
Phi Silica / Aion (Copilot+ NPUs or 6 GB VRAM only, an API in flux until 2027); MLX (Mac-only);
Ollama as a dependency (fine as a BYO endpoint, not something we install).

## Architecture

```
                    ai-worker (unchanged loop, rules, arbiter, calibration)
                                   |
             chat: trait Provider          embed: trait Provider
              (src-tauri/src/ai/provider.rs, chosen independently)
               +-------------------+--------------------+
               |                   |                    |
        AppleProvider        OpenAiProvider        (none: rules only)
     (Swift sidecar, mac)   (HTTP, reqwest)
                                   |
                 +-----------------+------------------+
                 |                                    |
         Endpoint::Custom                     Endpoint::Local
   (user URL + model + key)        RuntimeManager -> llama-server on
                                   127.0.0.1:<random>, --api-key <random>
                                                      |
                                       ModelStore (catalog, downloads, imports,
                                       <app local data>/models, sha256-verified)
```

### `Provider` trait (Rust, app crate)

The seam already exists: the worker only needs `classify`, `embed` and capabilities
(`worker.rs` `llm()` / `vector_for()` / `probe()`). Make it a trait, keep the vote contract the
arbiter already consumes:

```rust
pub trait Provider: Send {
    fn capabilities(&mut self) -> Capabilities;          // llm, embed (+dimension, model tag), personal, egress
    fn classify(&mut self, req: &LlmRequest, samples: u8, temperature: f32) -> Result<ClassifyOut, ProviderError>;
    // ClassifyOut: a distribution per field (HashMap<Label, f64> + samples) and an optional description
    fn embed(&mut self, texts: &[String]) -> Result<Embeddings, ProviderError>;
    fn id(&self) -> ProviderId;                          // feeds model_version, see "Versioning"
}
```

- `AppleProvider` wraps today's `Sidecar` unchanged (protocol version untouched).
- `OpenAiProvider` implements classify by running `samples` requests and counting votes exactly as
  the Swift side does (unknown name = no vote, unknown project = `"none"`), then reports the vote
  shares as the distribution. Today's `Votes` become one way to fill a distribution, so a
  decision model (see below) can report its probabilities through the same type.
- **Chat and embeddings are two slots, filled independently.** The worker holds a chat provider
  (T2) and an embed provider (T1 kNN). Typical pairings: Apple + Apple on a Mac with Apple
  Intelligence; Local + Local on Windows; Endpoint (chat) + Local (embed), so a remote LLM never
  sees the vectors' source text a second time and kNN keeps working when the endpoint is down.
  "Alongside the Apple model" means exactly this: a Mac can keep Apple's chat model and add a
  local embedder, or keep Apple embeddings and use a local chat model. One chat provider is active
  at a time; an ensemble of two LLMs is not in scope (see Open questions).
- The personal model (`train`/`personal_predict`) is CreateML-only. Phase 3 replaces it with a
  pure-Rust classifier over embeddings in `openrize-core` so T1 is identical on both platforms;
  until then `Capabilities.personal` is false off the Apple provider and the arbiter already
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

Request subset that works everywhere (OpenAI, OpenRouter, llama-server, Ollama, LM Studio, vLLM):
`POST /v1/chat/completions`, `stream: false`, `temperature` 0 (0.9 for the disagreement re-ask),
`max_tokens` 200, `response_format: {type: "json_schema", ...}`, and for Qwen-family models
`chat_template_kwargs: {enable_thinking: false}`. Optional `GET /v1/models` fills a model dropdown
for custom endpoints; free text otherwise.

Never trust the server. Each sample goes through one validator:

| What came back | What we do |
| --- | --- |
| Valid JSON, names in the live lists | One vote per field, mapped back to ids |
| `<think>...</think>` before the JSON | Strip, then validate |
| JSON wrapped in prose or a code fence | Take the first balanced `{...}`, then validate |
| A category name not in the list | No category vote for this sample (as Swift does) |
| A project name not in the list | Counts as "No project" (as Swift does) |
| Description empty, over 15 words or over 120 characters | Dropped; the template description stays |
| Schema rejected by the server (400) | Retry once with `json_object` plus the names listed in the prompt; remember per endpoint |
| Unparseable twice, timeout, 5xx, refused | Provider error: this job falls back to T0 + T1, the existing backoff applies |

A per-endpoint "JSON mode" fallback toggle covers servers that accept `json_schema` but ignore it.
Votes are counted exactly as from the `ClassifyResult` the Swift sidecar returns today
(`categoryVotes`, `projectVotes`, `description`, `samples`, `latencyMs`), so the arbiter's
weighting and the adaptive sampling in `classify()` behave as they do now.

## Model management (`ModelStore`)

### Catalog

`catalog.json` bundled with the app: for each model `id`, `revision`, display name, `kind`
(`chat` | `embed` | `decision`), Hugging Face repo **pinned to a commit**, file name, exact byte size,
**SHA-256**, licence, quantisation, RAM estimate (see Memory), context length, notes such as
"thinking off", and the golden-set scores from the last evaluation. Hosting stays on Hugging Face
(no bandwidth bill, no redistribution question); Apache-2.0 models can be mirrored later.

The catalog is bundled only: a model change ships as an app update, so every catalog entry has
been through the golden set with the exact llama.cpp build in that release. A remote catalog is
deliberately left out (see Proposed decisions).

Initial shortlist (to be confirmed by the golden-set evaluation below; all Apache-2.0 or MIT):

| Tier | Chat model (GGUF Q4_K_M) | Size | For |
| --- | --- | --- | --- |
| Small | Qwen3.5-0.8B | 0.5 GB | 8 GB machines, older CPUs |
| **Default** | Qwen3.5-2B | 1.3 GB | most laptops |
| Quality | Qwen3-4B-Instruct-2507 | 2.5 GB | 16 GB+, discrete or recent iGPU |
| Embeddings | bge-small-en-v1.5 (Q8_0, 384-d) | 37 MB | kNN, see Fallback |

Not offered: Llama 3.2 (community licence terms), Gemma 3 / EmbeddingGemma (Gemma Terms carry
redistribution obligations). Purpose-built classifiers are evaluated separately below.

### Downloads

- Download over HTTP Range into `<file>.partial` (resumable across launches), verify size and
  SHA-256, then rename. Progress follows the updater pattern: a throttled snapshot in state plus a
  `model-download` event read with `useTauriEvent`; single-flight (one download at a time); cancel
  keeps the `.partial` for resume, delete removes it.
- Before starting: free space must cover the file plus 1 GB, or the download does not start and
  the gallery says how much space is missing.
- Needs `reqwest`'s `stream` feature and `sha2`. Shared rustls provider setup becomes one helper
  (the updater currently installs it inline).
- Only Hugging Face's resolve URL for the pinned commit is contacted, over `https`; redirects to
  its CDN are followed, nothing else.

### Importing a model file

"Add a model file..." accepts a local `.gguf`. The app reads the GGUF header only (architecture,
parameter count, quantisation, context length, whether a chat template is present) with a small
reader in `openrize-core` (the header is a documented key-value table; no new dependency), then:

- rejects files with no chat template (embedding-only GGUFs can be imported as `embed`), or
  whose architecture the pinned llama.cpp build cannot load (found by a test load);
- references the file in place rather than copying it (no doubled disk use), recording path, size,
  mtime and a SHA-256 computed in the background; a moved or changed file shows "File missing" or
  "File changed" and is not loaded;
- labels it "Imported, not tested by OpenRize": no golden-set score, RAM estimate from the header.

Imports are for people who already run local models. They use exactly the same runtime, prompt and
validator as catalog models.

### Versioning

- A model's identity is `<catalog id>@r<revision>-<sha8>`; an import is `import:<file stem>-<sha8>`.
- `model_version` (already stored on every suggestion) becomes
  `<provider>:<model identity> p<PROMPT_VERSION> emb:<embed identity> pm:<artifact> cal:<artifact>`,
  for example `local:qwen3.5-2b@r1-3fa9c2e1 p1 emb:local:bge-small@r1-9c0d11aa pm:none cal:none`.
  `metrics.rs` already groups by `model_version`, so per-model accept rates come for free.
- When an app update ships a new revision of a downloaded model, the gallery shows
  "Update available (1.3 GB)". Nothing downloads on its own. The new file downloads beside the old,
  the switch happens only after it verifies, and the old file is offered for deletion.
- A pinned llama.cpp bump is a code change: it re-runs the golden set for every catalog model in
  the same PR.

### Disk use

- Models live in `<app local data dir>/models/` (Local AppData on Windows, never roaming), shared
  by every local account rather than copied into `accounts/<id>`. Like `config/`, the folder is
  not data: `data_location` never counts, moves or removes it, and it is not part of any export.
- Settings shows total disk used by models and the size of each, with Delete per model. Deleting
  the active model switches the chat slot to "none" (rules + T1) and says so.
- At startup, `.partial` files with no matching catalog entry, and `.partial` files older than
  30 days, are removed.
- Only one chat model needs to be on disk; after switching, the gallery offers to delete the
  previous one.

## Local runtime (`RuntimeManager`)

### Process

- Ships as a Tauri `externalBin` per platform (`tauri.windows.conf.json` /
  `tauri.macos.conf.json`), staged by `build.rs` like the `rize` sidecar: Windows x64 Vulkan
  build (+ its DLLs as `resources`), macOS arm64 Metal build. CUDA is not bundled; it can be an
  optional download later. Pin one tested llama.cpp build number and bump it deliberately.
- Started on demand by the worker, bound to `127.0.0.1` on a random free port with a random
  per-launch `--api-key`, `--parallel 1`, `--ctx-size 4096`, `--sleep-idle-seconds` so an idle
  tracker gives the memory back (time tracking is bursty: a classify every few minutes).
- One process for the chat model and, when the embed slot is local too, a second small process
  for the embedding model (simpler to size and restart than router mode). Health via `/health`;
  the existing `Sidecar` backoff and idle-stop rules (`IDLE_STOP_AFTER`) carry over; pids feed
  `AiRuntime::sidecar_pid` so energy attribution keeps working.
- Lifetime: killed on app exit (`flush_on_exit`), and tied to the app so a crash cannot orphan
  it: a Windows Job Object with kill-on-close, and on macOS the child exits when its stdin closes
  (a small watchdog thread in our launch wrapper, or `--parent-pid` style polling).
- Background manners: process priority below normal (`BELOW_NORMAL_PRIORITY_CLASS` on Windows,
  `nice 10` on macOS), `--threads` at half the physical cores (min 1) so the foreground app stays
  responsive, never `--mlock`, mmap left on so the OS can reclaim pages.
- Timeouts: first load can take 10-30 s on CPU, so the first request after a start gets 120 s;
  later classify requests use today's 90 s `TIMEOUT_CLASSIFY`.

### Quantisation

The catalog pins exactly one file per model, so there is never a quantisation choice in the UI:

- Chat models: **Q4_K_M**. The usual size/quality knee for 1-4B models; Q5/Q6 cost 20-40% more
  RAM for gains the golden set must show before we pay them. Below 4 bits (Q2/Q3, IQ2/IQ3) small
  models lose label accuracy quickly, so they are not offered.
- Embedding models: **Q8_0**. They are tiny, and quantisation error shows up directly in cosine
  similarity.
- Imports run whatever quantisation the file has; the gallery shows it.

### Memory limits

- RAM estimate per model, stored in the catalog and computed for imports:
  file size + KV cache at `--ctx-size 4096` (from the header: layers x KV heads x head dim x
  2 x 2 bytes x 4096) + 300 MB runtime overhead. For the default model that is about 1.9 GB.
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

## Bring-your-own endpoint

Settings stores base URL, model name, JSON-mode fallback and display name. **The API key goes in
the OS credential store** (`keyring-core` with `windows-native-keyring-store` /
`apple-native-keyring-store`): Windows Credential Manager, macOS Keychain. Commands are
`set_endpoint_key` / `clear_endpoint_key` / `endpoint_key_present`; the key never reaches the
webview, the settings file, the database or a log. A "Test connection" button sends one tiny
classify against the live categories and shows latency and whether structured output worked.

Network policy: only the configured base URL is ever contacted; `https` required except for
loopback hosts; explicit connect/request timeouts (generous for a local server's first model load).
An endpoint that fails is never silently replaced by another provider: its jobs fall back to
T0 + T1 and the status says why.

### Privacy: egress class

Every provider reports an egress class, and it changes what the prompt contains:

- **On device** (Apple sidecar, our llama-server): today's prompt, including the titles of private
  windows (they are redacted from storage after classification, `core/src/activity/redact.rs`).
- **Endpoint** (anything the user configured, even `localhost`, since we cannot see where it
  forwards): `features::extract` builds the prompt with private-window detail removed (app name
  only), and failed jobs on such a provider do not hold private detail for retries.

The endpoint form says plainly what is sent (app names, window titles, domains, category and
project names, your custom instructions) and that the provider's privacy policy then applies.
Nothing is sent until the user saves an endpoint. The existing "nothing here makes a network
call" note in `ai/mod.rs` becomes "only to the endpoint you configured, and model downloads".

## Purpose-built decision models (Jev, Clef and similar)

A new kind of model appeared in September-October 2026: **decision models** ("System One"). Instead
of generating text, they take a `state` (text or JSON) and typed `questions` (`choice` over a
label set supplied per request, `score` on a rubric, `noul` true/false) and return a probability
for every option from one forward pass. No decoding, no JSON to parse, no grammar. That is
exactly the shape of our problem: the block is the state, "category" and "project" are two
`choice` questions whose criteria are the live names with their descriptions and hints.

| | Jev (TypeSafe AI) | Clef / Clef-Flash (Cloudflare) | Smaller open decision models |
| --- | --- | --- | --- |
| Weights | Closed, hosted only, early access | Apache-2.0; Clef 27B, Clef-Flash 9B (Qwen3.5-9B + a joint schema head) | Kev (Qwen3.5-0.8B/4B base), Laya (ModernBERT, 421M, not zero-shot), others |
| Runs locally | No | Clef-Flash GGUF Q4_K_M is 6.7 GB | Kev-0.8B is in our size budget |
| API | `/v1/systemone`, not OpenAI-compatible | Same contract; also on Workers AI (`@cf/cloudflare/clef-flash`) | Same contract |
| llama.cpp | n/a | Supported (text-only), ggml-org/llama.cpp#29831, merged | `/v1/systemone` route and laya, julia-1, lev, openjev, kev models, ggml-org/llama.cpp#29818, merged 2026-10-02 |
| Fit for us | An endpoint option only; sends every block off-device | Too heavy for a default; a "Quality" tier at most on 32 GB machines | The realistic local candidate, quality unproven |

Benchmarks are vendor-reported and none are on time-tracking data (Kev's card itself reports a
large out-of-domain drop and option-order sensitivity), so nothing here is adopted on paper.

**How it fits without changing the plan.** The bundled `llama-server` already exposes
`/v1/systemone` for these models, so a decision model is one more catalog `kind` (`decision`)
served by the same runtime, loopback port, key, memory preflight and power gate. What changes is
one type: `Provider::classify` returns a distribution per field (`HashMap<Label, f64>` plus a
sample count) rather than raw vote counts. Generative providers produce it from votes, as today;
a decision provider fills it from `probabilities` in one call (no adaptive re-sampling needed).
The arbiter keeps its calibration step, because a model's own confidence is not our calibrated
confidence. A hosted Jev or Workers AI Clef is a `SystemOneProvider` (a second request shape
beside OpenAI's, same key storage, same egress class "Endpoint").

**What a decision model does not do:** it writes no description. With a decision model in the
chat slot, descriptions stay the template sentence, or come from a generative model run only for
the description when one is also installed. That trade-off is the main thing the golden set must
weigh: faster, cheaper, calibrated labels versus losing the AI-written name.

**Recommendation:** shape the `Provider` contract around distributions now (free in Phase 1), and
put Kev-0.8B and Clef-Flash into the Phase 4 golden set beside the Qwen chat models, scored on
category accuracy, project accuracy, calibration error and latency on the same machines. Adopt a
decision model as the default only if it beats the default chat model on labels by a clear margin
at similar or lower RAM. Jev stays a possible endpoint, not a dependency: it is closed and in
early access.

## Embeddings and kNN

Embeddings come from the embed slot (`/v1/embeddings` on a local or custom endpoint, or the Apple
sidecar). Vectors are tagged with their model (`entry_embeddings.model`, already stored); kNN
already skips mismatched dimensions, so switching embedders orphans old vectors. On a change,
re-embed in the background (oldest-first, capped per run, under the power gate) and keep using
rules + LLM until enough vectors exist. With no embedder, `embed` jobs are not queued at all
(fixes today's wasted retries on Windows).

## With no model installed (fallback)

The app must stay fully useful with no LLM at all, which is where every Windows user starts:

| Setup | Tiers that run | What the user sees |
| --- | --- | --- |
| Nothing chosen, nothing downloaded | T0 rules | "Suggestions use your rules" and a one-time banner offering the recommended model with its size |
| Embedder only (bundled or downloaded) | T0 + kNN | "Suggestions use your rules and similar past entries"; kNN warms up as entries are approved |
| Chosen provider temporarily failing (endpoint down, out of memory, paused on battery) | T0 + T1 | The provider's row shows the reason; entries handled in fallback are re-queued when it recovers (`store::requeue_fallback`, as when Apple's model becomes ready today) |
| Chat model ready | T0 + T1 + T2 | Normal suggestions |

Rules: never block tracking or the UI on a model; never switch to a different provider on the
user's behalf (an endpoint failing must not start a download, and a local model failing must not
send data to an endpoint); every fallback is visible in status, and entries are re-checked when a
better tier comes back, within the existing `BACKFILL_WINDOW_MS`.

## Model picker and honest capability reporting

### Status

`AiStatus` drops Apple vocabulary: `provider` (`apple` | `local` | `endpoint` | `none`), `llm`
(`ready` | `loading` | `downloading` | `paused` | `unavailable` + reason code), `embed`
(`ready` | `reembedding` + progress | `none`), `personal`, `runtime` (`running` | `stopped` |
`unavailable`), `device` (`gpu` | `cpu` + fallback reason). Reason codes are an enum the frontend
maps to sentences, like `llmUnavailableReason` does today. The frontend's `onDeviceMlUnavailable`
becomes "no provider", which also re-enables category AI hints and custom instructions for any
provider (today they are hidden whenever the sidecar is missing).

### Settings > AI

Settings: `ai_chat_provider`, `ai_embed_provider`, `local_chat_model`, `local_embed_model`,
`use_gpu`, `pause_local_on_battery_saver`, `endpoint_url`, `endpoint_model`,
`endpoint_json_mode`. Flat, `#[serde(default)]`, like the rest.

UI: a provider picker (Apple on supported Macs, Local model, Your endpoint, Rules only); a model
gallery; the endpoint form with Test and the privacy notice. Platform differences go through
`src/lib/platform.ts` records.

Each gallery card states, without marketing:

| Field | Source | Shown as |
| --- | --- | --- |
| Where it runs | egress class | "On this device" or "Sent to <host>" |
| What it does | capabilities | Category, Project, Descriptions, Similar entries (each yes/no) |
| Size on disk | catalog / file | "1.3 GB" |
| Memory | RAM estimate vs available now | "Needs about 1.9 GB, 6.2 GB free" or "Won't fit right now" |
| Test-set score | golden set, catalog models only | "Matched 84% of categories in our test set"; imports and endpoints: "Not tested by OpenRize" |
| Speed on this machine | self-test after install | "About 2 s per entry (GPU)" or "About 9 s per entry (CPU)" |
| Your results | `metrics.rs`, per `model_version` | Accept rate once 20+ suggestions were reviewed |
| Licence | catalog | "Apache-2.0" |

The **Recommended** badge is a hint, not a gate: the largest catalog chat model whose RAM estimate
fits available memory with headroom, preferring Default. Every model can still be chosen; a model
that does not fit says so before download, not after.

The self-test runs once after download or import (and after a GPU setting change): load time, one
classify against the live categories, and the device actually used. Its result is the speed line
above, and a failure here is the point where an import is rejected.

### Defaults

- Windows: Rules only (plus kNN if an embedder is bundled, see Proposed decisions) until the user
  picks a model or endpoint; a one-time banner offers the recommended download.
- macOS: Apple's model when available (unchanged); Local and Endpoint are available too, and the
  banner that today points to Apple Intelligence settings also offers a local model on Macs that
  can never run Apple's.

## Where it lives

Both sides: Rust owns providers, the runtime, downloads, imports, key storage and all network I/O
(`src-tauri/src/ai/*`, pure logic such as prompt/schema/validation, the GGUF header reader and the
catalog types in `openrize-core`); React owns the picker, gallery, endpoint form and status
(`src/pages/Settings.tsx` AI section, a new `src/components/ai/*`). The `rize` CLI gets read-only
`rize ai status` and `rize ai models` (and `COMMANDS.md`).

## Phases

1. **Provider seam + endpoint (0.10.0).** `Provider` trait with chat and embed slots, Rust
   prompt/schema/validator, `OpenAiProvider`, keyring storage, endpoint settings and UI,
   egress-class prompt rendering, generalized `AiStatus`, stop queueing `embed` without an
   embedder. Windows gets an LLM the day this ships (via the user's endpoint).
2. **Local models (0.10.x).** `ModelStore`, catalog, resumable verified downloads, imports,
   `RuntimeManager` with bundled `llama-server`, gallery UI with honest cards and the self-test,
   idle unload, memory preflight, GPU/CPU fallback.
3. **T1 everywhere.** Embeddings through the embed slot, re-embed on change, pure-Rust personal
   classifier replacing CreateML.
4. **Quality.** A golden set of real (consented, scrubbed) blocks and an `#[ignore]`d cargo
   evaluation that scores each catalog model and each purpose-built classifier candidate, used to
   lock the shortlist and to check any model, prompt or llama.cpp bump. No new test framework.

## Risks

- Small models can return valid JSON with the wrong label: the arbiter's calibration caps
  confidence, prefill needs 0.60, and the golden set decides the shortlist.
- Qwen thinking mode must stay off, or it burns tokens and fights the grammar.
- Vulkan driver bugs on old iGPUs: automatic CPU retry plus the switch.
- Packaging: llama-server's DLLs, antivirus false positives on an unsigned installer (already an
  open issue), and a few hundred MB of model files per user.
- Memory on 8 GB machines: small default, preflight, idle unload.
- llama.cpp moves daily: pin, test, bump on purpose.
- Imports are untested by definition: they are labelled so, and the self-test catches files that
  will not load.

## Proposed decisions (confirm or change in review)

1. **Bundle `llama-server` in the installer** (+~40 MB on Windows, +~17 MB on macOS) rather than
   downloading it with the first model. Downloading executable code at run time is a new
   security-sensitive path (it would need its own signing, not just a checksum), the runtime must
   match the app's pinned llama.cpp build anyway, and bundling keeps the first model usable
   offline once downloaded. Models (data, SHA-256 pinned) are the only thing downloaded.
2. **Every configured endpoint is treated as off-device, `localhost` included.** We cannot see
   where a local server forwards, so private-window titles are always removed from its prompt.
   Our own `llama-server` is the only local runtime that gets the full prompt.
3. **Phase 1 ships alone as 0.10.0.** Windows gets a real model the day it ships (through an
   endpoint), the provider seam lands and is exercised before the runtime is added on top, and
   each later phase is its own patch release (0.10.x), per the versioning rule: a new system is a
   minor bump, each fix after it a patch.
4. **Bundle the 37 MB embedding model too**, so kNN works on Windows from the first approved entry
   with no download. Without it, a Windows user who never picks a model stays on rules alone
   forever. Costs 37 MB in the installer; it is Apache-2.0/MIT, so redistribution is clean.
5. **Chat and embeddings are separate slots**, so an endpoint user keeps local embeddings and a Mac
   can mix Apple and local. The alternative, one provider for both, is simpler to explain but
   leaves endpoint users sending every block twice and losing kNN when the endpoint is down.
6. **The catalog is bundled only**, no remote catalog refresh. A new model ships as an app update
   that has passed the golden set with that release's llama.cpp. A remote catalog would let a
   model change reach users without that check.
7. **Allow importing a local GGUF**, referenced in place and labelled untested. The alternative is
   catalog-only, which is safer to support but turns away the users most likely to want a local
   model.
8. **Decision models (Jev, Clef, Kev) are a Phase 4 evaluation candidate, not a 0.10
   dependency**, but the `Provider` contract returns distributions from Phase 1 so adopting one
   later is a catalog entry rather than a refactor. Alternatives: adopt Kev-0.8B as the default
   now (unproven on our data, and loses AI descriptions), or ignore decision models entirely.
9. **Windows starts on rules (plus kNN) and offers the recommended download once**, in the AI
   banner, with its size. No model downloads without a click. The alternatives are a download step
   in onboarding (heavier first run) or downloading the recommended model automatically (1.3 GB
   nobody asked for, on metered connections too).
10. **Qwen3.5-2B is the default chat model** going into the golden set, with 0.8B and 4B as the
   other tiers. The golden set can still replace it; this only fixes what Phase 2 builds and tests
   against first.

## Open questions (not blocking 0.10.0)

- Should two LLMs ever vote together (Apple + local)? Only if the golden set shows a gain worth the
  doubled latency and energy.
- CUDA builds for NVIDIA users as an optional download: only after Vulkan numbers are in.
- Mirroring catalog models on our own storage once usage justifies it.

## Delivery plan

One PR per row, merged in order, each verified with `pnpm verify`, unit tests and a driven run
on Windows (and the macOS paths it touches reviewed by hand).

| PR | Version | Contents |
| --- | --- | --- |
| 1 | 0.10.0 | Phase 1. `Provider` trait with chat and embed slots; `AppleProvider` wrapping today's sidecar unchanged; prompt (`PROMPT_VERSION`), schema and validator ported to `openrize-core`; `OpenAiProvider` (chat completions, json_schema, retry, `<think>` stripping); endpoint settings; keyring-backed key commands; egress-class prompt rendering; endpoint form with Test and the privacy notice; generalized `AiStatus` + frontend; stop queueing `embed` jobs with no embedder |
| 2 | 0.10.1 | `ModelStore`: bundled catalog, resumable SHA-256-verified downloads, free-space check, progress events, gallery UI with Recommended badge, delete, `.partial` cleanup |
| 3 | 0.10.2 | `RuntimeManager` and the bundled `llama-server` (Windows Vulkan + CPU, macOS Metal): loopback port + per-launch key, Job Object / watchdog lifetime, priority and threads, memory preflight, idle unload, GPU switch with CPU fallback, power gate, self-test. Local models work end to end from here |
| 4 | 0.10.3 | Embeddings through the embed slot (bundled embedder if decision 4 holds), background re-embed on model change, kNN on Windows |
| 5 | 0.10.4 | GGUF import: header reader, in-place reference, background hash, untested label |
| 6 | 0.10.5 | Pure-Rust personal classifier over embeddings in `openrize-core`, replacing CreateML for T1 on both platforms |
| 7 | - | Golden-set evaluation (`#[ignore]`d cargo test over scrubbed real blocks) for catalog models and purpose-built classifier candidates, and the final model shortlist |
