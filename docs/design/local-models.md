# Local and bring-your-own AI models

Status: design, 2026-11-02. Target: 0.10.0 (a new system, so a minor bump). Windows first, macOS
on the same code.

## Why

Categorisation on Windows is rules-only today: no LLM, and no embeddings either, so kNN and the
personal model (T1) are missing too, and every `embed` job fails its three attempts
(`ai/worker.rs` `embed_only`). macOS has Apple's on-device model through the Swift sidecar, which
does not exist anywhere else. We want two ways to get a real model on every platform:

1. **Bring your own endpoint.** Any OpenAI-compatible server: a remote provider with the user's API
   key, or a local server the user already runs (Ollama, LM Studio, vLLM).
2. **Download a model we picked.** A short, curated list of small quantized models, chosen in-app
   the way dictation apps offer Whisper sizes, run fully on this machine.

Both must share one code path, and macOS must use the same path as Windows (Apple's model stays
available on Macs that have it).

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
                         trait Provider  (src-tauri/src/ai/provider.rs)
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
                                       ModelStore (catalog, downloads,
                                       <data>/models, sha256-verified)
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
    fn id(&self) -> ProviderId;                          // feeds model_version: "oa:local:qwen3.5-2b-q4km@<sha8>"
}
```

- `AppleProvider` wraps today's `Sidecar` unchanged (protocol version untouched).
- `OpenAiProvider` implements classify by running `samples` requests and counting votes exactly as
  the Swift side does (unknown name = no vote, unknown project = `"none"`).
- The personal model (`train`/`personal_predict`) is CreateML-only. Phase 3 replaces it with a
  pure-Rust classifier over embeddings in `openrize-core` so T1 is identical on both platforms;
  until then `Capabilities.personal` is false off the Apple provider and the arbiter already
  re-weights.

### One prompt and one schema, in Rust

Port the system/user prompt from `swift/.../Language.swift` into `openrize-core/src/ai/prompt.rs`
so every provider (including, later, the Swift one via its `classify` params) sends the same words.
Output schema (OpenAI strict-mode compatible: flat, all required, `additionalProperties: false`):

```json
{ "description": "string", "category": { "enum": ["<unique category names>"] },
  "project": { "enum": ["<unique project names>", "No project"] } }
```

Request subset that works everywhere (OpenAI, OpenRouter, llama-server, Ollama, LM Studio, vLLM):
`POST /v1/chat/completions`, `stream: false`, `temperature` 0 (0.9 for the disagreement re-ask),
bounded `max_tokens`, `response_format: {type: "json_schema", ...}`, and for Qwen-family models
`chat_template_kwargs: {enable_thinking: false}`. Never trust the server: strip `<think>` blocks,
parse, validate against the live names, retry once with `json_object`, then give up for this job
(the existing backoff applies). A per-endpoint "JSON mode" fallback toggle covers odd servers.
Optional `GET /v1/models` fills a model dropdown for custom endpoints; free text otherwise.

### Local runtime (`RuntimeManager`)

- Ships as a Tauri `externalBin` per platform (`tauri.windows.conf.json` /
  `tauri.macos.conf.json`), staged by `build.rs` like the `rize` sidecar: Windows x64 Vulkan
  build (+ its DLLs as `resources`), macOS arm64 Metal build. CUDA is not bundled; it can be an
  optional download later. Pin one tested llama.cpp build number and bump it deliberately.
- Started on demand by the worker, bound to `127.0.0.1` on a random free port with a random
  per-launch `--api-key`, `--ctx-size` sized for our prompts (~4k), `--sleep-idle-seconds` so an
  idle tracker gives the memory back (time tracking is bursty: a classify every few minutes).
- One process for the chat model; embeddings use llama-server's router mode or a second small
  process with the embedding model. Health via `/health`; the existing `Sidecar` backoff and
  idle-stop rules carry over; its pid feeds `AiRuntime::sidecar_pid` so energy attribution keeps
  working. Killed on app exit (`flush_on_exit`).
- GPU: Vulkan/Metal by default, a "Use GPU" switch (CPU fallback) for broken iGPU drivers, and an
  automatic CPU retry when the GPU backend fails to load.
- Runs under the power gate: background classification already respects idle/AC/battery saver
  for retraining; local inference adds a "pause local model on battery saver" check.

### Model catalog and downloads (`ModelStore`)

- `catalog.json` bundled with the app (and refreshable from the release manifest later): for each
  model `id`, display name, purpose (`chat` | `embed`), Hugging Face repo **pinned to a commit**,
  file name, exact byte size, **SHA-256**, license, RAM estimate, context length, notes such as
  "thinking off". Hosting stays on Hugging Face (no bandwidth bill, no redistribution question);
  Apache-2.0 models can be mirrored later.
- Download over HTTP Range into `<model>.partial` (resumable across launches), verify size and
  SHA-256, then rename. Stored in `<local data dir>/models/` (Local AppData on Windows, never
  roaming). Progress follows the updater pattern: a throttled snapshot in state plus a
  `model-download` event read with `useTauriEvent`; single-flight; cancel and delete.
- Needs `reqwest`'s `stream` feature and `sha2`. Shared rustls provider setup becomes one helper
  (the updater currently installs it inline).
- Hardware hint, not a gate: total RAM and GPU kind pick the "Recommended" badge.

Initial shortlist (to be confirmed by the golden-set evaluation below; all Apache-2.0):

| Tier | Chat model (GGUF Q4_K_M) | Size | For |
| --- | --- | --- | --- |
| Small | Qwen3.5-0.8B | 0.5 GB | 8 GB machines, older CPUs |
| **Default** | Qwen3.5-2B | 1.3 GB | most laptops |
| Quality | Qwen3-4B-Instruct-2507 | 2.5 GB | 16 GB+, discrete or recent iGPU |
| Embeddings | bge-small-en-v1.5 (Q8, 384-d) | 37 MB | kNN (downloaded with any chat model) |

Not offered: Llama 3.2 (community licence terms), Gemma 3 / EmbeddingGemma (Gemma Terms carry
redistribution obligations).

### Bring-your-own endpoint

Settings stores base URL, model name, JSON-mode fallback and display name. **The API key goes in
the OS credential store** (`keyring-core` with `windows-native-keyring-store` /
`apple-native-keyring-store`): Windows Credential Manager, macOS Keychain. Commands are
`set_endpoint_key` / `clear_endpoint_key` / `endpoint_key_present`; the key never reaches the
webview, the settings file, the database or a log. A "Test connection" button sends one tiny
classify against the live categories and shows latency and whether structured output worked.

Network policy: only the configured base URL is ever contacted; `https` required except for
loopback hosts; explicit connect/request timeouts (generous for a local server's first model load).

### Privacy: egress class

Every provider reports an egress class, and it changes what the prompt contains:

- **On device** (Apple sidecar, our llama-server): today's prompt, including the titles of private
  windows (they are redacted from storage after classification, `activity/redact.rs`).
- **Endpoint** (anything the user configured, even `localhost`, since we cannot see where it
  forwards): `features::extract` builds the prompt with private-window detail removed (app name
  only), and failed jobs on such a provider do not hold private detail for retries.

The endpoint form says plainly what is sent (app names, window titles, domains, category and
project names, your custom instructions) and that the provider's privacy policy then applies.
Nothing is sent until the user saves an endpoint. The existing "nothing here makes a network
call" note in `ai/mod.rs` becomes "only to the endpoint you configured".

### Embeddings and kNN

Embeddings come from the provider (`/v1/embeddings`) or the Apple sidecar. Vectors are tagged with
their model (`entry_embeddings.model`, already stored); kNN already skips mismatched dimensions, so
switching embedders orphans old vectors. On a change, re-embed in the background (oldest-first,
capped per run, under the power gate) and keep using rules + LLM until enough vectors exist. With
no embedder, `embed` jobs are not queued at all (fixes today's wasted retries on Windows).

### Status and settings

- `AiStatus` drops Apple vocabulary: `provider` (`apple` | `local` | `endpoint` | `none`), `llm`
  (`ready` | `loading` | `downloading` | `unavailable` + reason code), `embed`, `personal`,
  `runtime` (`running` | `stopped` | `unavailable`). The frontend's `onDeviceMlUnavailable`
  becomes "no provider", which also re-enables category AI hints and custom instructions for any
  provider (today they are hidden whenever the sidecar is missing).
- Settings: `ai_provider`, `local_chat_model`, `local_embed_model`, `use_gpu`, `endpoint_url`,
  `endpoint_model`, `endpoint_json_mode`. Flat, `#[serde(default)]`, like the rest.
- UI (Settings > AI): a provider picker (Apple on supported Macs, Local model, Your endpoint, Rules
  only); a model gallery with size, RAM, speed/quality and a Recommended badge, download progress,
  cancel and delete; the endpoint form with Test and the privacy notice. Platform differences go
  through `src/lib/platform.ts` records.

### Defaults

- Windows: Rules only until the user picks a model or endpoint; a one-time prompt in the AI banner
  offers the recommended download.
- macOS: Apple's model when available (unchanged); Local and Endpoint are available too.

## Where it lives

Both sides: Rust owns providers, the runtime, downloads, key storage and all network I/O
(`src-tauri/src/ai/*`, pure logic such as prompt/schema/validation in `openrize-core`); React owns
the picker, gallery, endpoint form and status (`src/pages/Settings.tsx` AI section, a new
`src/components/ai/*`). The `rize` CLI gets read-only `rize ai status` (and `COMMANDS.md`).

## Phases

1. **Provider seam + endpoint (0.10.0).** `Provider` trait, Rust prompt/schema/validator,
   `OpenAiProvider`, keyring storage, endpoint settings and UI, egress-class prompt rendering,
   generalized `AiStatus`, stop queueing `embed` without an embedder. Windows gets an LLM the day
   this ships (via the user's endpoint).
2. **Local models (0.10.x).** `ModelStore`, catalog, resumable verified downloads, `RuntimeManager`
   with bundled `llama-server`, gallery UI, idle unload, GPU/CPU fallback.
3. **T1 everywhere.** Embeddings through the provider, re-embed on change, pure-Rust personal
   classifier replacing CreateML.
4. **Quality.** A golden set of real (consented, scrubbed) blocks and an `#[ignore]`d cargo
   evaluation that scores each catalog model, used to lock the shortlist and to check any model or
   llama.cpp bump. No new test framework.

## Risks

- Small models can return valid JSON with the wrong label: the arbiter's calibration caps
  confidence, prefill needs 0.60, and the golden set decides the shortlist.
- Qwen thinking mode must stay off, or it burns tokens and fights the grammar.
- Vulkan driver bugs on old iGPUs: automatic CPU retry plus the switch.
- Packaging: llama-server's DLLs, antivirus false positives on an unsigned installer (already an
  open issue), and a few hundred MB of model files per user.
- Memory on 8 GB machines: small default, idle unload.
- llama.cpp moves daily: pin, test, bump on purpose.

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

## Delivery plan

One PR per row, merged in order, each verified with `pnpm verify`, unit tests and a driven run
on Windows (and the macOS paths it touches reviewed by hand).

| PR | Version | Contents |
| --- | --- | --- |
| 1 | 0.10.0 | Phase 1. `Provider` trait; `AppleProvider` wrapping today's sidecar unchanged; prompt, schema and validator ported to `openrize-core`; `OpenAiProvider` (chat completions, json_schema, retry, `<think>` stripping); endpoint settings; keyring-backed key commands; egress-class prompt rendering; endpoint form with Test and the privacy notice; generalized `AiStatus` + frontend; stop queueing `embed` jobs with no embedder |
| 2 | 0.10.1 | `ModelStore`: bundled catalog, resumable SHA-256-verified downloads, progress events, gallery UI with Recommended badge, delete |
| 3 | 0.10.2 | `RuntimeManager` and the bundled `llama-server` (Windows Vulkan + CPU, macOS Metal): loopback port + per-launch key, idle unload, GPU switch with CPU fallback, power gate. Local models work end to end from here |
| 4 | 0.10.3 | Embeddings through the provider, background re-embed on model change, kNN on Windows |
| 5 | 0.10.4 | Pure-Rust personal classifier over embeddings in `openrize-core`, replacing CreateML for T1 on both platforms |
| 6 | — | Golden-set evaluation (`#[ignore]`d cargo test over scrubbed real blocks) and the final model shortlist |
