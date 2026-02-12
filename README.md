# Wormhole

A high-performance local proxy that lets [Claude Code](https://docs.anthropic.com/en/docs/claude-code) and the [Claude Agent SDK](https://github.com/anthropics/claude-code-sdk) talk to any supported Claude provider -- Anthropic Direct, AWS Bedrock, Google Vertex AI, or Azure Foundry -- without changing a single line of configuration inside Claude Code itself.

Wormhole sits between Claude Code and the upstream provider.  It accepts the standard Anthropic Messages API on localhost, translates requests to the target provider's format, signs and authenticates, forwards upstream, then translates the response stream back to the Anthropic SSE format that Claude Code expects.

```
Claude Code  ──▶  POST /v1/messages (Anthropic format)
                    │
              ┌─────────────┐
              │   Wormhole   │  127.0.0.1:{port}
              └──────┬───────┘
                     │   translate + authenticate
                     ▼
              ┌─────────────────────────────┐
              │  Anthropic  │  Bedrock  │ … │
              └─────────────────────────────┘
```

---

## Table of Contents

- [Why Wormhole](#why-wormhole)
- [Quick Start](#quick-start)
- [Installation](#installation)
- [Two Modes of Operation](#two-modes-of-operation)
  - [Wrapper Mode](#wrapper-mode-wormhole-claude)
  - [Daemon Mode](#daemon-mode-wormhole-serve)
- [CLI Reference](#cli-reference)
- [Configuration](#configuration)
  - [Config File](#config-file)
  - [Environment Variables](#environment-variables)
  - [Credential Resolution Order](#credential-resolution-order)
- [Provider Details](#provider-details)
  - [Anthropic Direct](#anthropic-direct)
  - [AWS Bedrock](#aws-bedrock)
  - [Google Vertex AI](#google-vertex-ai)
  - [Azure Foundry](#azure-foundry)
- [Session Management](#session-management)
- [Failover and Retry](#failover-and-retry)
- [Latency Characteristics](#latency-characteristics)
- [Architecture Deep-Dive](#architecture-deep-dive)
  - [Request Flow](#request-flow)
  - [Streaming Pipeline](#streaming-pipeline)
  - [Provider Trait](#provider-trait)
  - [Bedrock Event Stream Decoding](#bedrock-event-stream-decoding)
- [Development](#development)

---

## Why Wormhole

Claude Code communicates with the Anthropic API via `ANTHROPIC_BASE_URL`.  Each cloud provider has a different URL scheme, authentication mechanism, request body format, and streaming wire format.  Switching between providers today requires manually juggling environment variables, model IDs, and credential configurations every time you start a session.

Wormhole eliminates that friction:

| Problem | Wormhole Solution |
|---|---|
| Different URLs per provider | Single localhost endpoint |
| Different auth (API key, SigV4, OAuth, Azure key) | Automatic credential resolution and signing |
| Different body schemas (`model` in URL vs body, `stream` removal) | Transparent request transformation |
| Bedrock uses binary Event Stream, not SSE | Real-time binary-to-SSE transcoding |
| Switching providers requires reconfiguring Claude Code | `--provider bedrock` flag or interactive picker |
| No failover between providers | Built-in retry with provider fallback chains |
| No session history across provider switches | Persistent session store with resume |

---

## Quick Start

```bash
# Build from source
cargo build --release

# Start Claude Code through Bedrock
./target/release/wormhole claude --provider bedrock

# Start Claude Code through Anthropic Direct with extra args
./target/release/wormhole claude --provider anthropic -- -p "explain this codebase"

# Interactive provider selection (fuzzy-search picker)
./target/release/wormhole claude

# Resume your last session
./target/release/wormhole claude --resume
```

---

## Installation

### From Source

```bash
git clone <repo-url> && cd wormhole
cargo build --release
# Binary is at ./target/release/wormhole
```

Requires Rust 1.75+. The build links against `aws-lc-rs` for TLS, which needs a C compiler and CMake:

```bash
# Ubuntu/Debian
sudo apt-get install build-essential cmake pkg-config

# macOS
xcode-select --install && brew install cmake
```

### First Run

```bash
# Create config file with interactive wizard
wormhole config init

# Or set credentials for a specific provider
wormhole config set-credentials anthropic
wormhole config set-credentials bedrock
```

---

## Two Modes of Operation

### Wrapper Mode (`wormhole claude`)

Start a proxy, spawn Claude Code connected to it, shut down when Claude exits.  This is the simplest way to use wormhole -- one command does everything.

```bash
wormhole claude --provider bedrock -- -p "fix the bug"
```

What happens behind the scenes:

1. Resolves credentials for the selected provider
2. Binds a TCP listener on `127.0.0.1` with an auto-assigned port
3. Spawns `claude` as a child process with:
   - `ANTHROPIC_BASE_URL=http://127.0.0.1:{port}`
   - `ANTHROPIC_API_KEY=wormhole-proxy` (dummy value; wormhole handles real auth)
4. All Claude Code HTTP traffic flows through the local proxy
5. When Claude Code exits, the proxy saves the session and shuts down

```
┌──────────────────────────────────────────────┐
│  $ wormhole claude --provider bedrock        │
│                                              │
│  ──────────────────────────────────────────  │
│    Provider:  AWS Bedrock                    │
│    Session:   a1b2c3d4                       │
│    Proxy:     http://127.0.0.1:54321         │
│  ──────────────────────────────────────────  │
│                                              │
│  Claude Code starts here...                  │
└──────────────────────────────────────────────┘
```

### Daemon Mode (`wormhole serve`)

Run a long-lived proxy server that multiple Claude Code sessions connect to simultaneously, each with its own provider routing.

```bash
# Terminal 1: Start the daemon
wormhole serve --port 8080

# Terminal 2: Create a session routed to Bedrock
SESSION=$(curl -s -X POST http://localhost:8080/v1/sessions \
  -H "Content-Type: application/json" \
  -d '{"provider": "bedrock", "model": "claude-opus-4-6"}' | jq -r '.id')

# Terminal 2: Connect Claude Code to that session
ANTHROPIC_BASE_URL=http://localhost:8080 \
ANTHROPIC_API_KEY=wormhole-proxy \
ANTHROPIC_CUSTOM_HEADERS="X-Wormhole-Session: $SESSION" \
claude

# Terminal 3: Another Claude Code instance routed to Vertex
SESSION2=$(curl -s -X POST http://localhost:8080/v1/sessions \
  -H "Content-Type: application/json" \
  -d '{"provider": "vertex"}' | jq -r '.id')

ANTHROPIC_BASE_URL=http://localhost:8080 \
ANTHROPIC_API_KEY=wormhole-proxy \
ANTHROPIC_CUSTOM_HEADERS="X-Wormhole-Session: $SESSION2" \
claude
```

Session management REST API:

```
POST   /v1/sessions              Create session (provider, model, fallbacks)
GET    /v1/sessions              List active sessions
GET    /v1/sessions/{id}         Get session details
DELETE /v1/sessions/{id}         Delete a session
```

Requests without an `X-Wormhole-Session` header use the default provider from config.

---

## CLI Reference

### `wormhole claude`

```
wormhole claude [OPTIONS] [-- <CLAUDE_ARGS>...]

Options:
  --provider <PROVIDER>    anthropic | bedrock | vertex | foundry
  --model <MODEL>          Override model ID
  --resume [<SESSION_ID>]  Resume a previous session (no arg = most recent)
  --port <PORT>            Fixed proxy port (default: auto-assign)

Everything after -- is passed through to the claude CLI.
```

**Provider selection priority:**

1. `--provider` flag (explicit)
2. `--resume` (from saved session)
3. `proxy.default_provider` in config file
4. Interactive fuzzy-select picker

### `wormhole serve`

```
wormhole serve [OPTIONS]

Options:
  --port <PORT>   Port to listen on (default: 8080)
  --host <HOST>   Host to bind to (default: 127.0.0.1)
```

### `wormhole sessions`

```
wormhole sessions list             List all sessions (sorted by last active)
wormhole sessions delete <ID>      Delete a session (prefix match supported)
```

### `wormhole config`

```
wormhole config init                          Interactive config wizard
wormhole config show                          Print resolved config (secrets masked)
wormhole config set-credentials <PROVIDER>    Set up credentials for a provider
```

### Global Options

```
--verbose    Enable debug logging (provider requests, timing, failover events)
```

---

## Configuration

### Config File

Location: `~/.wormhole/config.toml`

```toml
[proxy]
host = "127.0.0.1"
port = 0                              # 0 = auto-assign in wrapper mode
default_provider = "anthropic"        # Used when no --provider flag

[failover]
max_retries = 2                       # Total retry attempts across providers
retry_on = [429, 500, 502, 503, 529]  # HTTP codes that trigger failover
backoff_base_ms = 500                 # Exponential backoff base
fallback_order = ["anthropic", "bedrock"]  # Fallback chain after primary

[session]
auto_cleanup_days = 30                # Delete session files older than this

# ── Provider Credentials ─────────────────────────────────────

[providers.anthropic]
api_key = "sk-ant-..."                # Direct value
# api_key_env = "MY_ANTHROPIC_KEY"    # Or reference an env var by name
# base_url = "https://..."            # Override API base URL

[providers.bedrock]
region = "us-east-1"
profile = "default"                   # AWS CLI profile name
# access_key_id = "AKIA..."           # Explicit credentials (optional)
# secret_access_key = "..."

[providers.vertex]
project_id = "my-gcp-project"
region = "us-east5"
# credentials_file = "/path/to/service-account.json"

[providers.foundry]
resource = "my-azure-resource"        # {resource}.services.ai.azure.com
api_key_env = "AZURE_FOUNDRY_API_KEY" # Reference env var
```

Config file permissions are checked.  If the file is world-readable, wormhole prints a warning and recommends `chmod 600`.

### Environment Variables

| Variable | Purpose |
|---|---|
| `ANTHROPIC_API_KEY` | Anthropic Direct API key |
| `AWS_REGION` / `AWS_DEFAULT_REGION` | Bedrock region |
| `AWS_PROFILE` | AWS CLI profile |
| `GOOGLE_CLOUD_PROJECT` | Vertex project ID |
| `GOOGLE_CLOUD_REGION` | Vertex region |
| `GOOGLE_APPLICATION_CREDENTIALS` | Path to GCP service account JSON |
| `AZURE_FOUNDRY_RESOURCE` | Azure Foundry resource name |
| `AZURE_FOUNDRY_API_KEY` | Azure Foundry API key |
| `WORMHOLE_*` | Override any config field (e.g. `WORMHOLE_PROXY_PORT=9090`) |

### Credential Resolution Order

For each provider, wormhole tries credentials in this order:

1. **Config file** -- explicit value in `~/.wormhole/config.toml`
2. **Config file env reference** -- `api_key_env = "MY_VAR"` indirection
3. **Standard environment variables** -- `ANTHROPIC_API_KEY`, `AWS_PROFILE`, etc.
4. **Cloud CLI defaults** -- `~/.aws/credentials`, `gcloud` ADC, `az login`

If all sources fail, wormhole prints a diagnostic listing exactly what it tried and suggests a `wormhole config set-credentials <provider>` command.

---

## Provider Details

### Anthropic Direct

The simplest provider.  Near-transparent passthrough.

| Aspect | Details |
|---|---|
| **URL** | `https://api.anthropic.com/v1/messages` |
| **Auth** | `x-api-key` header |
| **Body transform** | None |
| **Response** | SSE passthrough (zero parsing, zero copying) |
| **Latency overhead** | Minimal -- TCP relay only |

### AWS Bedrock

The most complex provider.  Requires SigV4 signing and binary Event Stream decoding.

| Aspect | Details |
|---|---|
| **URL** | `https://bedrock-runtime.{region}.amazonaws.com/model/{modelId}/invoke-with-response-stream` |
| **Auth** | AWS SigV4 signed headers (computed per-request) |
| **Body transform** | Remove `model` (moved to URL), remove `stream` (endpoint determines), add `anthropic_version: "bedrock-2023-05-31"` |
| **Model ID** | Translated: `claude-opus-4-6` becomes `global.anthropic.claude-opus-4-6-v1` |
| **Response** | Binary AWS Event Stream format decoded and re-emitted as SSE in real-time |
| **Latency overhead** | ~0.5ms for SigV4 signing + per-event decode overhead (see [Bedrock Event Stream Decoding](#bedrock-event-stream-decoding)) |

**Model ID Translation:**

| Anthropic ID | Bedrock ID |
|---|---|
| `claude-opus-4-6` | `global.anthropic.claude-opus-4-6-v1` |
| `claude-sonnet-4-5-20250929` | `global.anthropic.claude-sonnet-4-5-v1` |
| `claude-haiku-4-5-20251001` | `global.anthropic.claude-haiku-4-5-v1` |
| `claude-3-5-sonnet-20241022` | `global.anthropic.claude-3-5-sonnet-20241022-v2` |
| Unknown models | `global.anthropic.{model}-v1` (fallback pattern) |

### Google Vertex AI

| Aspect | Details |
|---|---|
| **URL** | `https://{region}-aiplatform.googleapis.com/v1/projects/{project}/locations/{region}/publishers/anthropic/models/{model}:streamRawPredict` |
| **Auth** | `Authorization: Bearer {oauth_token}` via Application Default Credentials |
| **Body transform** | Remove `model` (moved to URL), remove `stream` (endpoint determines), add `anthropic_version: "vertex-2023-10-16"` |
| **Response** | SSE passthrough (same format as Anthropic Direct) |
| **Latency overhead** | Minimal -- TCP relay after initial OAuth token fetch |

### Azure Foundry

| Aspect | Details |
|---|---|
| **URL** | `https://{resource}.services.ai.azure.com/anthropic/v1/messages` |
| **Auth** | `api-key` header |
| **Body transform** | None (model and stream stay in body) |
| **Response** | SSE passthrough |
| **Latency overhead** | Minimal -- TCP relay only |

---

## Session Management

Every `wormhole claude` invocation creates a session file at `~/.wormhole/sessions/{uuid}.json`:

```json
{
  "id": "a1b2c3d4-e5f6-...",
  "provider": "bedrock",
  "model": "claude-opus-4-6",
  "region": "us-east-1",
  "fallback_providers": ["anthropic"],
  "created_at": "2026-02-12T10:00:00Z",
  "last_active": "2026-02-12T10:30:00Z"
}
```

**Resuming sessions:**

```bash
# Resume most recent session (same provider, same model)
wormhole claude --resume

# Resume by ID prefix (first 4+ chars is usually enough)
wormhole claude --resume a1b2

# List all sessions
wormhole sessions list
```

```
  ID         Provider         Model                    Last Active
  ──────────────────────────────────────────────────────────────────────
  a1b2c3d4   AWS Bedrock      claude-opus-4-6          2026-02-12 10:30:00
  e5f6a7b8   Anthropic Direct claude-sonnet-4-5-20250929  2026-02-11 15:20:00
```

Sessions older than `auto_cleanup_days` (default: 30) are automatically deleted on startup.

---

## Failover and Retry

When a request to the primary provider fails with a retryable error, wormhole automatically tries the next provider in the fallback chain.

**Retryable errors:** `429` (rate limit), `500`, `502`, `503`, `529` (overloaded), and connection failures.

**Non-retryable errors:** `400`, `401`, `403`, `404` -- these propagate immediately.

### How It Works

```
Request ──▶ Primary Provider (e.g. Bedrock)
               │
               ├── 200 OK ──▶ Return response (happy path, zero overhead)
               │
               └── 429 Rate Limited
                    │
                    ├── sleep(500ms)
                    │
                    ▼
               Fallback 1 (e.g. Anthropic)
                    │
                    ├── 200 OK ──▶ Return response
                    │
                    └── 500 Error
                         │
                         ├── sleep(1000ms)
                         │
                         ▼
                    Fallback 2 (e.g. Vertex)
                         │
                         └── ...
```

### Configuration

```toml
[failover]
max_retries = 2                        # Total attempts after primary fails
retry_on = [429, 500, 502, 503, 529]   # Status codes that trigger failover
backoff_base_ms = 500                  # Exponential backoff: 500ms, 1s, 2s, ...
fallback_order = ["anthropic", "bedrock", "vertex"]
```

### Streaming Safety

- If an error occurs **before any response bytes are sent**: the request is retried on the next provider.
- If an error occurs **mid-stream** (after bytes have already been sent to Claude Code): the error propagates to the client. Claude Code handles re-requesting automatically.

---

## Latency Characteristics

Wormhole is designed to add as little latency as possible between Claude Code and the upstream provider. Here is where time is spent:

### Added Latency Per Request

| Component | Overhead | Notes |
|---|---|---|
| **TCP hop through localhost** | ~0.05ms | loopback, no network |
| **Request body JSON parse** (axum) | ~0.1ms | Needed to inspect `stream` field and route |
| **Provider selection** (DashMap lookup) | ~0.001ms | Lock-free concurrent hashmap |
| **Anthropic/Vertex/Foundry transform** | ~0.01ms | Field removal/insertion in JSON |
| **Bedrock body transform + SigV4 signing** | ~0.5ms | SHA-256 hash of body + HMAC chain |
| **Upstream TLS handshake** | ~50-200ms | **Only on first request** (connection pooled) |
| **SSE passthrough** (Anthropic/Vertex/Foundry) | ~0 | Bytes flow through untouched |
| **Bedrock Event Stream decode per event** | ~0.01ms | Zero-copy field extraction, no JSON parse |

**Total added latency:** <1ms per request on a warm connection (all providers).  First request includes TLS handshake, subsequent requests reuse the pooled connection.

### Optimizations Applied

**Connection layer:**
- `TCP_NODELAY` enabled -- disables Nagle's algorithm, SSE events are not delayed
- TCP keepalive at 30-second intervals -- prevents idle connection drops
- Connection pool: up to 32 idle connections per host, 90-second idle timeout
- Connections are reused across requests to the same provider

**Streaming pipeline:**
- **Anthropic, Vertex, Foundry:** zero-copy SSE passthrough. Response bytes from upstream are forwarded to Claude Code with no parsing, no buffering, no transformation. Each chunk from the upstream TCP socket is immediately written to the downstream socket.
- **Bedrock:** The binary Event Stream decoder uses:
  - Pre-allocated reusable buffers (no per-event heap allocation)
  - Byte-level JSON field extraction (no `serde_json::Value` parsing of event payloads)
  - In-place base64 decoding into a reusable buffer
  - Direct byte-slice assembly of SSE frames
  - The event type is extracted by scanning for `"type":"` in the raw bytes -- no DOM construction
- `x-accel-buffering: no` header disables any reverse-proxy buffering
- `cache-control: no-cache` prevents response caching

**Request path:**
- Pre-computed base URLs and host headers (no per-request `format!()` for static strings)
- Pre-computed provider-specific header values stored on the provider struct
- Body bytes are moved (not copied) into the outbound request
- SigV4 signing operates on borrowed slices (`&[(&str, &str)]`), not owned Strings

**Failover fast path:**
- When the primary provider succeeds (the common case), there is zero overhead from the failover wrapper -- no Vec allocation, no iteration setup, no backoff logic evaluated
- The failover check (`is_retryable`) is `#[inline]` and uses a `HashSet<u16>` for O(1) status code lookup

**Server:**
- axum with tower-http `TraceLayer` for structured logging (configurable via `--verbose`)
- No middleware on the hot path beyond routing and tracing
- State is `Arc<AppState>` -- single atomic reference count increment per request

---

## Architecture Deep-Dive

### Request Flow

```
                 Claude Code
                     │
                     ▼
           POST /v1/messages
         Content-Type: application/json
         X-Wormhole-Session: abc123   (optional, daemon mode)
                     │
              ┌──────┴──────┐
              │  axum Router │
              └──────┬──────┘
                     │
              resolve_provider()
              ┌──────┴──────┐
              │ if header has X-Wormhole-Session:
              │   DashMap.get(session_id) -> Arc<dyn Provider>
              │ else:
              │   state.default_provider.clone()
              └──────┬──────┘
                     │
              inspect body["stream"]
              ┌──────┴──────┐
              │ true:  provider.messages_stream(body, headers)
              │ false: provider.messages(body, headers)
              └──────┬──────┘
                     │
         ┌───────────┼───────────┐
         ▼           ▼           ▼
    Anthropic    Bedrock     Vertex/Foundry
    (passthru)   (SigV4 +   (OAuth/key +
                  EventStream  passthru)
                  decode)
         │           │           │
         ▼           ▼           ▼
    upstream      upstream    upstream
    response      response    response
         │           │           │
         └─────┬─────┘───────────┘
               │
         SSE byte stream
               │
               ▼
         Body::from_stream()
         Content-Type: text/event-stream
               │
               ▼
          Claude Code
```

### Streaming Pipeline

For the three SSE-compatible providers (Anthropic, Vertex, Foundry), the streaming pipeline is:

```
upstream TCP socket
    │  reqwest::Response::bytes_stream()
    ▼
 Stream<Item=Result<Bytes>>
    │  .map(|r| r.map_err(...))
    ▼
 SseByteStream (Pin<Box<dyn Stream<Item=Result<Bytes, ProviderError>>>>)
    │  Body::from_stream()
    ▼
 axum Response body
    │  hyper writes to TCP socket
    ▼
 Claude Code
```

Each `Bytes` chunk from upstream is a reference-counted pointer to the kernel's TCP read buffer. No copying occurs -- the same memory is written to the downstream socket.

For Bedrock, the pipeline includes a binary-to-SSE transcoding step:

```
upstream TCP socket (application/vnd.amazon.eventstream)
    │  reqwest::Response::bytes_stream()
    ▼
 BytesMut accumulation buffer
    │  extract complete binary frames
    ▼
 Per-frame: extract "bytes" field (byte scan, no JSON parse)
    │  base64 decode into reusable Vec
    ▼
 Extract "type" field (byte scan)
    │  assemble "event: {type}\ndata: {payload}\n\n" into reusable Vec
    ▼
 Bytes::copy_from_slice() -> yield
    │
    ▼
 SseByteStream -> Body -> Claude Code
```

### Provider Trait

The core abstraction that all four providers implement:

```rust
pub type SseByteStream = Pin<Box<dyn Stream<Item = Result<Bytes, ProviderError>> + Send>>;

#[async_trait]
pub trait Provider: Send + Sync + 'static {
    fn kind(&self) -> ProviderKind;

    async fn messages_stream(
        &self,
        body: serde_json::Value,
        headers: &HeaderMap,
    ) -> Result<SseByteStream, ProviderError>;

    async fn messages(
        &self,
        body: serde_json::Value,
        headers: &HeaderMap,
    ) -> Result<serde_json::Value, ProviderError>;

    async fn count_tokens(
        &self,
        body: serde_json::Value,
        headers: &HeaderMap,
    ) -> Result<serde_json::Value, ProviderError>;

    async fn list_models(&self) -> Result<serde_json::Value, ProviderError>;
}
```

`SseByteStream` returns raw `Bytes` so that passthrough providers (Anthropic, Vertex, Foundry) forward upstream bytes with zero transformation.  Only Bedrock decodes and re-emits.

### Bedrock Event Stream Decoding

AWS Bedrock uses a binary framing protocol called [AWS Event Stream](https://docs.aws.amazon.com/transcribe/latest/dg/streaming-format.html) instead of SSE.  Each binary frame has this layout:

```
 0                   1                   2                   3
 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1
├───────────────────────────────────────────────────────────────────┤
│                         Total Length (4B)                         │
├───────────────────────────────────────────────────────────────────┤
│                        Headers Length (4B)                        │
├───────────────────────────────────────────────────────────────────┤
│                        Prelude CRC (4B)                          │
├───────────────────────────────────────────────────────────────────┤
│                     Headers (variable length)                     │
├───────────────────────────────────────────────────────────────────┤
│                     Payload (variable length)                     │
│   {"bytes": "base64-encoded-event-json"}                         │
├───────────────────────────────────────────────────────────────────┤
│                        Message CRC (4B)                          │
└───────────────────────────────────────────────────────────────────┘
```

Wormhole's decoder:

1. Accumulates bytes in a `BytesMut` buffer (pre-allocated to 16 KB)
2. Reads `total_length` and `headers_length` from the first 8 bytes
3. Waits until the full frame is available in the buffer
4. Extracts the payload region (`[12 + headers_len .. total_len - 4]`)
5. Scans payload bytes for `"bytes":"` to locate the base64 value (no JSON parsing)
6. Decodes base64 in-place into a reusable buffer
7. Scans decoded bytes for `"type":"` to extract the event type
8. Assembles `event: {type}\ndata: {json}\n\n` into a reusable buffer
9. Yields the SSE frame as `Bytes`

This approach eliminates the three-pass pattern (parse payload JSON, parse event JSON, re-serialize event JSON) that a naive implementation would use.  Per-event overhead is dominated by the base64 decode (~0.01ms for typical events).

---

## Development

### Building

```bash
cargo build                 # Debug build
cargo build --release       # Optimized build
```

### Testing

```bash
cargo test                  # Run all 42 tests
cargo test test_anthropic   # Run a specific test suite
cargo test -- --nocapture   # Show test output
```

### Test Suites

| Suite | Tests | Coverage |
|---|---|---|
| `model_map` (unit) | 4 | Model ID translation per provider |
| `test_anthropic` | 7 | Streaming, non-streaming, errors, headers, count_tokens, list_models |
| `test_bedrock` | 7 | Model mapping, body transformation for all providers |
| `test_daemon` | 3 | Session CRUD via API, multi-session routing, invalid session error |
| `test_failover` | 5 | 429/500 failover, non-retryable passthrough, all-fail, streaming failover |
| `test_sessions` | 8 | Create, get, prefix match, list, delete, touch, most_recent, fallbacks |
| `test_streaming` | 4 | Full proxy passthrough (streaming + non-streaming), health, no-provider error |

Integration tests use [wiremock](https://crates.io/crates/wiremock) mock servers that simulate upstream providers.  Each test starts its own mock server and wormhole proxy, verifying end-to-end behavior.

### Project Structure

```
src/
  main.rs                          Entry point, CLI dispatch
  lib.rs                           Library root (enables integration tests)
  types.rs                         ProviderKind enum
  error.rs                         AppError, ProviderError, SessionError
  cli/
    mod.rs                         Cli struct, Command enum (clap derive)
    claude.rs                      Wrapper mode implementation
    serve.rs                       Daemon mode implementation
    sessions.rs                    sessions list/delete commands
    config_cmd.rs                  config init/show/set-credentials
  config/
    mod.rs                         load_config() via figment
    types.rs                       WormholeConfig, provider config structs
    credentials.rs                 Credential resolution chain
  server/
    mod.rs                         build_router(), start_server()
    handlers.rs                    /v1/messages, /v1/messages/count_tokens, /v1/models, /health
    session_api.rs                 /v1/sessions CRUD (daemon mode)
    state.rs                       AppState (DashMap sessions, default provider)
  session/
    mod.rs                         SessionStore: file-based persistence
    types.rs                       SessionId, SessionState
  provider/
    mod.rs                         Provider trait, SseByteStream type
    anthropic.rs                   Anthropic Direct (SSE passthrough)
    bedrock.rs                     AWS Bedrock (SigV4 + EventStream decode)
    vertex.rs                      Google Vertex AI (OAuth + SSE passthrough)
    foundry.rs                     Azure Foundry (api-key + SSE passthrough)
    model_map.rs                   Model ID translation tables
    failover.rs                    FailoverProvider with retry logic
  auth/
    mod.rs
    aws.rs                         SigV4 request signing
    gcp.rs                         GCP OAuth token fetching
tests/
  helpers/mod.rs                   Shared test fixtures and mock data
  test_anthropic.rs                Anthropic provider integration tests
  test_bedrock.rs                  Model mapping and body transform tests
  test_daemon.rs                   Daemon mode session API tests
  test_failover.rs                 Failover and retry tests
  test_sessions.rs                 Session persistence tests
  test_streaming.rs                End-to-end proxy streaming tests
```

### Dependencies

| Layer | Crate | Purpose |
|---|---|---|
| HTTP framework | `axum 0.8` | Proxy server with streaming support |
| HTTP client | `reqwest` (rustls) | Connection pooling, streaming, TLS |
| Runtime | `tokio` | Async runtime |
| Streaming | `futures-util` + `async-stream` | Per-event stream transformation |
| CLI | `clap 4` + `dialoguer` + `console` | Arg parsing + interactive prompts + styled output |
| Config | `figment` (toml + env) | Hierarchical config loading |
| Logging | `tracing` + `tracing-subscriber` | Structured async-aware logging |
| Sessions | `dashmap` + file-based JSON | Concurrent in-memory + disk persistence |
| Secrets | `secrecy` | Zeroize-on-drop, redacted Debug |
| AWS auth | `aws-config` + `aws-sigv4` | Credential chain + SigV4 signing |
| GCP auth | `gcp_auth` | Application Default Credentials |
| Errors | `thiserror` + `anyhow` | Typed errors with IntoResponse |
| Fast byte scan | `memchr` | SIMD-accelerated byte search in Event Stream decoder |
