# tuclaw desktop: the v3 client library in `tuclaw-core`

## Overview

Step C2 of tuclaw v2 (tuclaw repo, `docs/plans/20261002-v2-architecture-proposal.md`, S4 "The client protocol"), cut down on 2026-10-03 to the client library only: Pavel has not decided how the desktop UI will look, so this plan builds the `/api/v3` client in `tuclaw-core` with a mock of the daemon built in, and leaves every UI change to small later tasks he asks for himself. The daemon side (C1, tuclaw repo, `docs/plans/20261003-v2-c1-client-api.md`) ships `/api/v3` REST, the event socket and bearer auth.

The contract both sides implement is `docs/contracts/v3-client-contract.md` in this repository (a verbatim copy of tuclaw's `docs/plans/20261003-v3-client-contract.md`, frozen 2026-10-03). Every wire shape in this plan is that file's; where this plan and the contract disagree, the contract wins and this plan is wrong.

End state: `tuclaw_core::v3` exposes `Client`, `Transport`, `HttpTransport`, `MockTransport`, `Connection`, `Frame`, `ClientFrame`, `Run` and the wire types, every public item with rustdoc, all four gates green, `app/` untouched and the app's behavior unchanged. A later UI task picks the library up through the seams listed in Post-Completion.

What the library does:

1. **Wire types and frame decoding** for every REST body and socket frame of the contract, pinned by golden JSON fixtures copied from it, tolerant of unknown types, kinds and fields.
2. **One async transport seam, two transports**: `HttpTransport` (async HTTP + WebSocket on a small tokio runtime owned by `core`, bearer token, one socket task) for the daemon and `MockTransport` (in-process, scripted, no network) for development and tests - both answer the same JSON and both feed the same decoder. Every call returns an executor-agnostic future (zed's `reqwest_client` pattern), so the app awaits it on GPUI's executors and tests with `futures::executor::block_on`.
3. **A typed `Client`** over the seam with every call of the contract, and a **run reducer** that folds frames into a `Run` (streamed segment, steps, status) with the contract's replay, snapshot and ordering rules pinned by tests.

### Non-goals (this plan)

- No change under `app/`: no link loop, no state changes, no views, no composer changes, no `mise run dev-mock`. The app keeps opening local mode exactly as today.
- No change to `core/src/model.rs`, `store.rs`, `schema.rs`, `fixtures.rs`, `grouping.rs` or `paths.rs`: the v3 types live under `core/src/v3/` with their own ids and shapes; mapping them onto the app's `Channel`/`Agent`/`Message` is a later UI task, once the UI's needs are known.
- No Markdown parsing or rendering, no message cache, no client state file, no unread computation.
- Nothing the contract lists under "Out of v3.0" (channels and DMs, threads, cards and dialogs, attachments, message edit, search, task and wiring management, agent create/update/delete, a thinking stream).
- No older-history paging in the client (`before` has no caller yet; the `messages` call fetches the newest page).
- No persisted event cursor; `since` is a value the caller keeps for reconnects.

### Rejected alternatives

- **A mock HTTP+WebSocket server process instead of an in-process mock transport.** A server would need a second binary, a port and a `tungstenite` server loop, and would test the network stack rather than the client; the seam Pavel asked for is the client's own, and the in-process mock drives the same decoder and the same reducer with zero network. The HTTP transport is still exercised against a canned loopback server in its own tests.
- **A mock that emits on a real-time thread in tests.** GPUI's test scheduler at the pinned revision fails a test woken from a foreign thread, and the later UI tasks will test against this mock. The mock therefore has two pacings: real time for a future `dev-mock`, stepped (the test drives every emission) for tests - including this plan's own.
- **A blocking client (`ureq` + one `std::thread` per socket with a read timeout).** The first version of this plan. Rejected by Pavel on 2026-10-03 after reading zed's `crates/client`, `http_client` and `reqwest_client`: zed is async end to end, and a blocking client would make every UI caller park a background thread per call and keep the heartbeat on a hand-rolled read-timeout loop.
- **A runtime-free async stack (`smol`/`async-io`, `async-tungstenite` with its async-std backend, an `isahc`-style HTTP client).** It would avoid tokio, but these are the less-used options; zed itself runs `reqwest` and `async-tungstenite` on tokio and bridges the futures, which is the pattern copied here.
- **Depending on `gpui_tokio`.** It is a zed crate that lives on the GPUI side; `core` must not depend on `gpui`, so `core` owns its runtime (as zed's `reqwest_client::runtime()` does) and returns futures that need no runtime in the caller.
- **Mapping v3 types onto `model.rs` now.** The app's domain was shaped for the fixtures; which of its fields survive the v3 UI is Pavel's open design. Converting now would guess the UI and touch every view.
- **Keeping the uncommitted 2026-09-03 polling iteration as the base.** It polls REST every 5 s and cannot stream; a farm session cannot see uncommitted files anyway. Recommended to Pavel: discard it (see Context).

## Skills to invoke

Load each skill below with the Skill tool and follow its conventions before implementing any task in this plan.

- `rust-style` - the code style every file in this repository follows (summarized in `CLAUDE.md`, "Code style"); the gate below quotes it
- `rustdoc` - `///` docs on every public item of `tuclaw-core`

## Toolchain

As `README.md`: Rust 1.98.1 through mise, `mise run` for everything, never a bare `cargo`. The app is never launched by this plan. `cargo tree -p tuclaw-core` (through `mise exec -- cargo tree`) must not mention `gpui` after any task.

Dependencies at HEAD: the workspace has `anyhow`, `rusqlite`, `serde`, `serde_json`, `time` (`formatting`, `macros`), `gpui`, `gpui_platform`; `tuclaw-core` has no `test-support` feature and no `testing` module. This plan adds, each in the task that first needs it, with `mise exec -- cargo add` at the current release (no versions from memory):

- `time` gains the `parsing`, `serde` and `serde-well-known` features (RFC 3339 timestamps decoded straight into `OffsetDateTime` with `time::serde::rfc3339`), Task 1
- `uuid` (`v4`) in `core` (`ClientMessageId::random()`), Task 1
- `tokio` (`rt-multi-thread`, `net`, `time`, `sync`, `macros`), `reqwest` with `default-features = false` and `json` (no TLS: the daemon is plain `http://`/`ws://` on LAN and Tailscale), `tokio-tungstenite` with default features (no TLS feature), `futures` (`BoxFuture`, the channels in `Connection`), and the `test-support` feature on `tuclaw-core` gating `pub mod testing`, Task 3

`reqwest` does not turn 4xx/5xx into errors unless `error_for_status` is called; the transport reads status and body itself and maps them.

## Context (from discovery, this repository @ `tuclaw-desktop-v1` HEAD c3ed5b6)

Line numbers deliberately absent; grep by name. **This plan is written against clean HEAD.**

- **The uncommitted 2026-09-03 iteration.** The working tree also carries uncommitted files from a read-only polling client over `/api/v2` and `/api/v1`: `core/src/daemon.rs`, `core/src/testing.rs`, `app/src/sync.rs`, plus edits to `core/src/store.rs`, `core/src/lib.rs`, `core/Cargo.toml`, `Cargo.toml`, `app/src/*.rs`, `CLAUDE.md`, `README.md`. It is to be discarded before the farm starts (Pavel's call, see Post-Completion). On clean HEAD this plan CREATES `core/src/testing.rs` (a new `FakeDaemon` for v3, Task 3) and never creates or touches `core/src/daemon.rs` or anything under `app/`; if the iteration's `core/src/testing.rs` is still in the tree when the farm starts, that is the signal the tree is not clean - stop and report, do not merge the two.
- **Branch state.** PR #1 (v1) is merged into `master`; this plan runs on `feat/v3-client`, cut from `master` on 2026-10-03 after the 2026-09-03 iteration was discarded (saved as a patch outside the repo). The plan, `docs/contracts/v3-client-contract.md` and `core/testdata/v3/*.json` are committed before the farm starts (they are untracked as written).
- `core/src/lib.rs` exports `grouping`, `model`, `paths`, `store`; `fixtures` and `schema` are private. `core/Cargo.toml` has one feature section to grow (`test-support`). `core` tests are plain `#[test]`; `app` tests are `#[gpui::test]` - none are touched here.
- The daemon side as of this writing: tuclaw `main` has B1 (surfaces, agents, sessions per agent, `messages` identity columns, `runs`/`run_steps`); B2 (event log + bus, Telegram as a subscriber) is on the farm; C1 is planned. The agent edge emits a `text.delta` for every assistant text block (intermediate thoughts and the answer alike) and a `step.text` when a block completes, so a run's streamed text and its text steps describe the same content at two granularities - the contract's `run.snapshot.text` is therefore the in-progress segment only. `send_to_agent` wakes a second agent on the same surface while the asker's run continues, so a surface can have more than one live run.
- Four gates: `mise run fmt-check`, `mise run lint`, `mise run test`, `mise run build`.

## Development Approach

- **testing approach**: Regular (code first, then tests in the same task).
- Complete each task fully before the next; every task ends with the four gates green. All tasks are `core`-only and need no window.
- **CRITICAL: every task MUST include new/updated tests** - success and error paths.
- **CRITICAL: existing tests are kept green, never loosened.** Nothing in `app/` or in the existing `core` modules changes.
- **CRITICAL: all four gates must pass before starting the next task.**
- **CRITICAL: update this plan file when scope changes during implementation.**
- `core` must never depend on `gpui`.

## Code-Quality Rules (verify before marking each task complete)

### Rust (from the `rust-style` skill, as `CLAUDE.md` states it)

Non-negotiable; the gate for marking any task complete. If a rule is violated the task is not done - refactor, re-test, then mark complete.

- `for` loops with mutable accumulators, not `.iter().filter().map().collect()` chains.
- `let ... else` for early returns; the happy path stays unindented.
- Shadow variables through transformations; no `raw_` / `parsed_` / `trimmed_` prefixes.
- **No comments.** No inline explanations, no trailing comments, no section dividers, no TODOs, no commented-out code. The only exceptions are `///` doc comments on `tuclaw-core`'s public items and `//!` module docs.
- Newtypes over bare `String` where the string carries meaning; enums over `bool` parameters.
- Never a wildcard `_ =>` match; match every variant. Avoid `matches!`.
- Every `match` on `Frame` lists every variant, including the empty arms.
- Public items in `core` carry `///` docs with an `# Examples` block where one makes sense (`rustdoc` skill).
- No deprecated API under `-D warnings` (for example `futures` channels' `try_next` is deprecated; use `try_recv`).

**Per-task gate (before marking a checkbox `[x]`):**
1. `mise run fmt-check`, `mise run lint` (clippy, `-D warnings`), `mise run test`, `mise run build` all green.
2. `cargo tree -p tuclaw-core` (through `mise exec -- cargo tree`) does not mention `gpui`.
3. Grep the new code for comments (`//` outside `///` and `//!`), `_ =>`, `matches!`, iterator chains with `collect`; none.
4. Only after 1-3 pass: mark complete.

## Testing Strategy

- **Golden fixtures** (`core/testdata/v3/*.json`, copied from the contract's examples; Task 1 completes them with one fixture per frame type, the focus client frame included): every decoder test reads a fixture, so a contract change that is not mirrored here fails a test rather than the first live connection.
- **The mock transport in stepped pacing is the test double for everything above the wire**: its futures are ready at once and its frames go through in-process channels, so the reducer and client tests drive scripted scenarios by calling the mock's `step()`/`play_all()` from the test thread and polling with `futures::executor::block_on`. No test is woken from the runtime's thread except the one `Realtime` smoke test.
- **The HTTP transport is tested against a canned loopback server** (`FakeDaemon`, `core/src/testing.rs` under the `test-support` feature, running on the same `core` runtime): auth header, status mapping, the WebSocket handshake with `since` and the bearer header, scripted lines, a server that closes the socket, an idle server (heartbeat deadline), a server that never answers the handshake (connect timeout), dropping `Connection` and dropping a pending call (the tokio task is aborted).
- **Reducer tests** (`core/src/v3/run.rs`): every frame type applied to a run in every state, replay idempotency (the same `seq` twice; a snapshot's `as_of_seq`), `run.reset`, `run.snapshot` over an existing run, the text model (segment cleared by its `step.text`), the terminal mapping.
- **A headless end-to-end test** (`core/tests/v3_mock.rs`) drives `Client` over `MockTransport` through the contract's fresh-start sequence, a post, an interrupt, a reconnect with `since` and a `gap`, applying every frame through the reducer - the executable version of the contract.

## Progress Tracking

- Mark completed items with `[x]` immediately when done.
- Add newly discovered tasks with ➕ prefix; document blockers with ⚠️ prefix.
- Update this plan if implementation deviates from scope.

## Solution Overview

```
  core/src/v3/ (no gpui, no model.rs)
  +-----------------------------------------------------------+
  | dto.rs       wire shapes (serde), ids, enums with fallback |
  | frames.rs    Frame + ClientFrame, decode/encode (tolerant) |
  | runtime.rs   the core-owned tokio runtime, spawn + abort   |
  |              on drop                                       |
  | transport.rs trait Transport (object-safe, BoxFuture),     |
  |              Connection, ApiError, Backoff                 |
  | http.rs      HttpTransport: reqwest + tokio-tungstenite,   |
  |              bearer, ONE socket task                       |
  | mock.rs      MockTransport: MockWorld, scripts,            |
  |              Pace {Realtime, Stepped}                      |
  | client.rs    Client { Arc<dyn Transport> }: every call     |
  | run.rs       Run reducer: frames -> Run (segment, steps)   |
  +-----------------------------------------------------------+
  core/src/testing.rs (test-support): FakeDaemon loopback server
  core/tests/v3_mock.rs: headless end-to-end over the mock
```

- **One async seam, two transports.** `Transport` has three methods returning `BoxFuture<'static, Result<_, ApiError>>`: `get(path) -> Json`, `post(path, body) -> Json`, `connect(since) -> Connection {frames: Receiver<Frame>, control: Sender<ClientFrame>}`. `HttpTransport` talks to the daemon; `MockTransport` answers from scripted scenarios. `Client` holds an `Arc<dyn Transport>`, is `Clone`, and owns every path string and every DTO decode, so the mock and the daemon are decoded by the same code.
- **Runs are a reducer.** `Run` is a value with `apply(frame)`; the reducer is where every ordering, replay and snapshot rule of the contract is pinned by tests. A later UI keeps the live runs in a map keyed by `RunId` and renders from them.
- **Fresh start exactly as the contract says** and testable headless: connect without `since`, take `hello{head}`, fetch `surfaces`, `agents` and the shown page, then apply the frames buffered meanwhile.

## Technical Details

### Wire types (`core/src/v3/dto.rs`)

- Ids as newtypes: `SurfaceId(i64)`, `AgentId(i64)`, `MessageId(i64)`, `InputId(i64)`, `RunId(String)`, `ClientMessageId(String)` with `ClientMessageId::random()` (uuid v4), `Seq(i64)` - all `serde(transparent)`.
- One `serde` struct per contract body, field names exactly the contract's (`snake_case`), `#[serde(default)]` on every optional field, unknown fields ignored: `Surface {id, kind, name, sort_order, last_message_at, lead_agent_id, agents: Vec<Wiring {agent_id, role, listens}>, bindings: Vec<Binding {channel, external_id, mirror}>, live_run: Option<LiveRun {run_id, agent_id}>}`, `Agent {id, name, ident, description, bot_username, model, state, live_run: Option<LiveRun {run_id, surface_id}>, home_surface_id}`, `MessagesPage {messages, has_more}`, `Message {id, surface_id, kind, author: Author {kind, agent_id}, addressed_agent_id, reply_to_message_id, text, run_id, origin, channel, client_message_id, created_at, run_summary: Option<RunSummary {status, step_count, tool_count, duration_ms}>}`, `Post {text, addressed_agent_id, client_message_id}`, `Posted {message_id, input_id, agent_id}`, `RunDetail {run, steps}`, `RunRow {id, agent_id, surface_id, origin, kind, status, terminal_reason, error, started_at, finished_at, usage, context}`, `StepRow {seq, kind, tool_use_id, name, input: Option<serde_json::Value>, output, status, started_at, finished_at}` (the contract's one generic row for all four kinds; per kind the contract says what `name`/`output`/`status` carry), `ErrorBody {error: {code, message}}`.
- Timestamps are `OffsetDateTime` through `time::serde::rfc3339` (`Option` variants through `rfc3339::option`).
- Enum-valued fields (`MessageKind`, `AuthorKind`, `Channel {Telegram, Desktop}`, `Role`, `RunStatus`, `StepKind`, `AgentState`) decode with an `#[serde(other)]` `Unknown` variant, so a value this build does not know never fails a whole page.

### Frames (`core/src/v3/frames.rs`)

- `Frame` is the decoded server frame: `Hello {head, floor, server_time, capabilities}`, `Gap {floor}`, `RunSnapshot {run_id, agent_id, surface_id, started_at, as_of_seq, text, steps: Vec<StepRow>}`, `RunStarted {seq, surface_id, run_id, at, agent_id, input_ids, origin, turn_kind}`, `StepText {seq, run_id, text}`, `StepToolStarted {seq, run_id, tool_use_id, name, input, parent_tool_use_id}`, `StepToolFinished {seq, run_id, tool_use_id, is_error, error, summary}`, `StepTask {seq, run_id, task_id, task_type, state, description, summary}`, `StepStatus {seq, run_id, status, detail}`, `RunReset {seq, run_id}`, `RunFinished {seq, run_id, is_error, error, terminal_reason, usage, context_usage}`, `MessageCreated {seq, message}`, `TextDelta {surface_id, run_id, text}`, `InputAccepted {input_id, surface_id, agent_id}`, `Unknown {type_name}`. Every variant with a `seq` also carries the envelope's `surface_id`/`at`; the accessor `Frame::seq() -> Option<Seq>` tells persisted from ephemeral.
- `decode(line: &str) -> Result<Frame, DecodeError>` reads the envelope first (`v`, `type`, optional `seq`, `surface_id`, `run_id`, `at`) and then the payload by type; an unknown `type` or an unknown `step.*` becomes `Unknown`, never an error; a malformed line is an error the caller logs and skips.
- `ClientFrame::Focus {surface_ids}` encodes with `encode(&ClientFrame) -> String` to the contract's client frame - the server envelope without `seq`: `{"v": 1, "type": "focus", "payload": {"surface_ids": [...]}}`.
- The `input` of `step.tool_started` stays a `serde_json::Value`; a `{"truncated": "..."}` object and `summary`/`error` texts of exactly 2 KB are the agent's clamps (the contract) - the library exposes `StepToolStarted::is_truncated()` and leaves rendering to the UI.
- Golden fixtures: `core/testdata/v3/{surfaces,agents,messages_page,post_message,run,event_frame}.json` exist (copied from the contract; `run.json`'s text step carries its segment in `output`); Task 1 adds `frames/<type>.json` for every frame type with a realistic payload, a `hello`, and `focus.json`. The decoder tests round-trip every fixture.

### Run reducer (`core/src/v3/run.rs`)

- `Run {id, agent_id, surface_id, status: RunState {Queued, Running, Stopping, Ok, Error, Interrupted}, started_at, finished_at, segment: String, steps: Vec<Step>, error: Option<String>, input_ids: Vec<InputId>, last_seq: Option<Seq>}`; `Step {seq, kind: StepKind}` with `StepKind::Text {text}`, `Tool {tool_use_id, name, input: serde_json::Value, output: Option<String>, status: StepStatus {Running, Ok, Error}, started_at, finished_at}`, `Task {task_id, task_type, state, description, summary}`, `Status {status, detail}`, `Other {name, output}` (a step row of a kind this build does not know). `Step::from_row(StepRow)` maps the generic row by kind.
- **The text model.** `segment` is the text block in progress only: `text.delta` appends to it; `step.text` pushes a `Text` step and clears `segment` (the agent emits one `step.text` per completed block, and every block streams through `text.delta` first); `run.reset` clears `segment` and the text steps and keeps tool steps. A `run.snapshot` replaces `segment` and `steps` wholesale and seeds `last_seq` from its `as_of_seq`, so the persisted frames the server sends afterwards with `seq <= as_of_seq` (already in the snapshot) are replays.
- `Run::apply(&mut self, frame: &Frame) -> Applied {Changed, Unchanged, NotMine}`: a frame of another run is `NotMine`; a persisted frame whose `seq` is `<=` `last_seq` is `Unchanged`; `step.tool_started` pushes a running tool step; `step.tool_finished` finds the step by `tool_use_id` (a finish for an unknown id pushes a finished step, so a snapshot that lost the start still renders); `step.task`/`step.status` push; `run.finished` sets `finished_at`, `error`, and `status` by `terminal_reason == "interrupted"` -> `Interrupted`, else `is_error` -> `Error`, else `Ok`; `Stopping` is set by the caller after a successful interrupt call and overwritten by `run.finished`. Every applied persisted frame advances `last_seq`.
- Constructors: `Run::from_started(&RunStarted)`, `Run::from_snapshot(&RunSnapshot)`, `Run::queued(InputAccepted)` (the placeholder between `input.accepted` and `run.started`; `Run::start(&mut self, &RunStarted)` turns it into a running run when its `input_id` is in `input_ids`). Derived: `Run::summary() -> RunSummary` (status, step and tool counts, duration), `Run::elapsed(now)`.
- The ordering facts a caller must handle, documented on the module (`//!`) and pinned by the end-to-end test: a `text.delta` can precede its run's `run.started` (the daemon's persisted and ephemeral paths are not ordered against each other) - keep it aside until the run exists; an `input.accepted` can arrive after `run.started` - ignore it when the `input_id` already belongs to a run; a `message.created` carrying a `run_id` arrives BEFORE that run's `run.finished` for user and a2a runs, after it for scheduled runs; an interrupted run never gets an answer message.

### Runtime, transport, HTTP and client (`core/src/v3/runtime.rs`, `transport.rs`, `http.rs`, `client.rs`)

- **The runtime** (`runtime.rs`, private): a lazily built tokio runtime (`OnceLock`, multi-thread, ONE worker, `enable_all`), as zed's `reqwest_client::runtime()`; `HttpTransport::new` takes its `Handle`, `HttpTransport::with_handle` takes a caller's. `spawn(handle, future) -> BoxFuture<'static, T>` runs the future on the runtime and returns a future that awaits its `JoinHandle` and ABORTS the tokio task when dropped (zed's `gpui_tokio::Tokio::spawn` guard), so every future the library returns needs no runtime in the caller and cancels its I/O when the caller gives up.
- `trait Transport: Send + Sync { fn get(&self, path: &str) -> BoxFuture<'static, Result<Value, ApiError>>; fn post(&self, path: &str, body: Value) -> BoxFuture<'static, Result<Value, ApiError>>; fn connect(&self, since: Option<Seq>) -> BoxFuture<'static, Result<Connection, ApiError>>; }` - object-safe, held as `Arc<dyn Transport>`. `Connection {frames: futures::channel::mpsc::UnboundedReceiver<Frame>, control: futures::channel::mpsc::UnboundedSender<ClientFrame>}` plus `Connection::focus(&self, surface_ids)`; dropping `Connection` closes `control`, which ends the socket task, which closes `frames`. `ApiError {Unauthorized, NotFound, Invalid(String), Conflict, Unavailable, Transport(String), Decode(String)}` is mapped from the contract's error envelope (status + body) and from I/O, timeouts included.
- `Backoff` (pure, `transport.rs`): zed's reconnect policy - starts at 500 ms, doubles to a 30 s cap, `next_delay(jitter: f64) -> Duration` adds `jitter * delay` for a caller-supplied fraction in `[0, 1)` (a random one in the app, a fixed one in tests), `reset()` after a successful `hello`. The waiting itself is the caller's (in the app a GPUI timer, so UI tests can `advance_clock`).
- `HttpTransport::new(base_url, token)`: a `reqwest::Client` with a 5 s timeout and `Authorization: Bearer` as a default header; `get`/`post` to `{base}/api/v3{path}` through `spawn`; statuses mapped by hand (`2xx` with an empty body -> `Value::Null`; `401` -> `Unauthorized`; an envelope -> its code; a non-envelope non-2xx body -> `Transport`). `connect` builds the upgrade from `"{ws_base}/api/v3/events[?since=N]".into_client_request()` (tungstenite fills in `Sec-WebSocket-Key` and the rest) plus the `Authorization` header, bounds `tokio_tungstenite::connect_async` by a 5 s `tokio::time::timeout`, and spawns ONE socket task that `select!`s over the socket stream, `control`, and a heartbeat deadline: a text message is decoded and sent into `frames` (a malformed line is skipped); every inbound message, pings included, pushes the deadline out; a `ClientFrame` from `control` is encoded and sent; the task closes the socket and returns when nothing arrived for `heartbeat_deadline` (60 s; `HttpTransport::with_heartbeat_deadline` for tests), when `control` closes (`Connection` dropped), when the server closes, or on any error - returning drops `frames`'s sender, which is how a caller learns to reconnect.
- `Client {transport: Arc<dyn Transport>}`, `Clone`, `Client::new(transport)`, `Client::http(base_url, token)` and `Client::mock(scenario, pace)` as the two constructors; every call is `async fn`: `surfaces() -> Vec<Surface>`, `agents() -> Vec<Agent>`, `messages(surface, limit) -> MessagesPage` (the newest page), `post(surface, Post) -> Posted`, `run(id) -> RunDetail`, `interrupt(run_id) -> Result<(), ApiError>` (`Conflict` when the run is not live), `connect(since) -> Connection`. Paths and query strings are built in one place; the futures are `Send + 'static` (they own an `Arc` of the transport), so the app can hand them to `cx.background_spawn`.
- `FakeDaemon` (`core/src/testing.rs`, `#[cfg(any(test, feature = "test-support"))]`): a canned loopback server on the `core` runtime (`tokio::net::TcpListener`) answering routes with a status code and a JSON body, recording request headers, and upgrading `/api/v3/events` through `tokio_tungstenite::accept_hdr_async` (recording the upgrade's headers) to send scripted lines, stay silent, never finish the handshake, or close - built for this plan's transport tests and the later UI's.

### Mock (`core/src/v3/mock.rs`)

- `MockWorld` (three surfaces: General with two wired agents, two single-agent topics; four agents with idents; 60 messages of mixed kinds and origins across them, one surface with a live run at connect) behind a `Mutex`; `Scenario::default()` plus knobs `gap_once`, `unavailable_once`; `Pace {Realtime, Stepped}`.
- `get`/`post` answer by serializing the same `dto.rs` structs, and every frame the mock emits is produced as a JSON line and pushed through `frames::decode` before entering `frames`, so the mock exercises the decoder exactly as the socket does.
- `connect` pushes `hello` and one `run.snapshot` (with `as_of_seq`) per live run synchronously before returning; a `focus` received on `control` queues a `run.snapshot` per live run of a newly focused surface; a reconnect with `since` replays the persisted frames after it from a ring of the last 1000 and never an ephemeral one; with `since` below the ring a `gap` is sent.
- `post` appends the user message and QUEUES the scripted frames of a canned run for `addressed_agent_id` when it is set and wired on the surface, else the surface's lead (the `202` names that `agent_id`; an unwired `addressed_agent_id` is `Invalid`, like the daemon's `400`): `input.accepted`, `message.created{user}` (`channel: desktop`, the echoed `client_message_id`), `run.started`, several `text.delta`s, `step.text`, `step.tool_started`/`step.tool_finished` for a `Bash` call with a multi-line input and a 2 KB-clamped summary, a `step.task`, more deltas, `step.text`, `message.created{answer}` with a Markdown answer (heading, list, code block, a table), then `run.finished{ok}`. A repeated `client_message_id` returns the first `Posted` and queues nothing.
- `post /runs/{id}/interrupt` on a live run drops the rest of the queue and emits `run.finished{terminal_reason: interrupted, is_error: false}` with NO `message.created{answer}` (the contract's rule); on a finished run it answers `Conflict`.
- `telegram_tick()` queues one scripted Telegram user post into General (`channel: telegram`); `play_a2a()` queues two runs on one surface at once (the lead's run plus a `send_to_agent` target's run).
- The mock's futures resolve at once (`futures::future::ready`), with no runtime involved. `Realtime`: a task on the `core` runtime plays the queue with `tokio::time::sleep` between frames (`text.delta` every 50 ms); `Stepped`: nothing plays until the caller's `step()` (one frame) or `play_all()` (the queue). `MockTransport` is `Clone` (shared world), so a test holds a handle while `Client` holds another.

## What Goes Where

- **Implementation Steps** (`[ ]`): code, tests and docs in this repository, all under `core/`.
- **Post-Completion** (no checkboxes): Pavel's decisions, the C1-side items, and the wiring note for the later UI tasks.

## Implementation Steps

### Task 1: Wire types and frame decoding with golden fixtures

**Files:**
- Create: `core/src/v3/mod.rs`, `core/src/v3/dto.rs`, `core/src/v3/frames.rs`, `core/testdata/v3/frames/*.json`
- Modify: `core/src/lib.rs` (`pub mod v3`), `Cargo.toml` (`time` features `parsing`, `serde`, `serde-well-known`), `core/Cargo.toml` (`uuid` with `v4`)
- Existing: `core/testdata/v3/{surfaces,agents,messages_page,post_message,run,event_frame}.json`

- [x] `dto.rs`: the ids, every body struct with the contract's field names, `#[serde(default)]` on optionals, `#[serde(other)]` fallbacks on enum fields, the generic `StepRow`, the error envelope, RFC 3339 timestamps as `OffsetDateTime`, `ClientMessageId::random()`
- [x] `frames.rs`: the `Frame` enum with `seq()`, `decode(line)` reading envelope then payload, `Unknown` for unknown types and step kinds, `ClientFrame::Focus` with `encode`
- [x] complete the fixtures: replace the contract's `"..."` placeholders with realistic values in the six existing files and add `frames/hello.json`, `gap.json`, `run_snapshot.json` (with `as_of_seq`, a `task` and a `status` step row), `run_started.json`, `step_text.json`, `step_tool_started.json` (one with a `truncated` input), `step_tool_finished.json`, `step_task.json`, `step_status.json`, `run_reset.json`, `run_finished.json` (one `ok`, one `interrupted`), `message_created.json`, `text_delta.json`, `input_accepted.json`, `unknown.json`, `focus.json`
- [x] tests: every fixture decodes to the expected variant and fields; unknown type and unknown step kind decode to `Unknown`; an unknown enum value (message `kind`, step `kind`, `channel`) decodes to the fallback without failing the page; a malformed line is an error; an envelope without `seq` is ephemeral (`seq()` is `None`); extra fields are ignored; `ClientFrame::Focus` encodes byte-equal to `focus.json`; `ClientMessageId::random()` is a UUID and unique across calls
- [x] run the four gates - must pass before next task
- the workspace gates first failed in `gpui_apple`'s build script (Xcode 27.0 shipped without the Metal Toolchain); after `xcodebuild -downloadComponent MetalToolchain` all four are green
- ➕ naming as built: the dto step-row enum is `RowKind` (run.rs keeps `StepKind`); the surface and agent live-run shapes are `SurfaceRun`/`AgentRun`; persisted run frames are `Frame::{RunStarted, StepText, ToolStarted, ToolFinished, Task, Status, RunReset, RunFinished}(RunEvent<Body>)` with bodies `RunStarted`, `StepText`, `ToolStarted`, `ToolFinished`, `TaskUpdate`, `StatusUpdate`, `RunReset`, `RunFinished`; `Frame::run_id()` joins `seq()`; `run.finished`'s `usage` (`Usage`, which gained `num_turns`/`duration_api_ms`) and `context_usage` (`ContextUsage {total_tokens, max_tokens, percentage, model}`) are typed after the 2026-10-03 contract clarification (the daemon renames the agent edge's camelCase), and `Posted.input_id` is an `Option` (a stored message whose wake could not be queued); a run event without `run_id` or `seq` is a `DecodeError::Missing`; `time` gets `parsing` + `serde` (the `serde-well-known` feature is not needed); fixtures: `event_frame.json` carries a real user message, extra `frames/step_tool_started_truncated.json` and `frames/run_finished_interrupted.json`


### Task 2: The run reducer

**Files:**
- Create: `core/src/v3/run.rs`
- Modify: `core/src/v3/mod.rs`

- [x] `Run`, `Step`, `StepKind` (incl. `Other`), `RunState`, `ToolStatus`, `Step::from_row`, the constructors, `Run::start`, `Run::apply` with the text model, the `as_of_seq` seeding and the terminal mapping per Technical Details, `Run::elapsed(now)`, `Run::summary()`; the module doc states the ordering facts
- [x] tests (table-driven over the fixtures of Task 1): `apply` for every frame in every state; `text.delta` then `step.text` leaves one text step and an empty segment; replay of the same `seq` is `Unchanged`; a persisted frame with `seq <= as_of_seq` after a snapshot is `Unchanged`; a frame of another run is `NotMine`; `run.reset` keeps tool steps; `tool_finished` for an unknown id pushes a finished step; `snapshot` replaces wholesale; `run.finished` maps interrupted/error/ok and overwrites `Stopping`; a `queued` run started by a `run.started` naming its input; `summary()` counts; `from_row` for a `task`, a `status` and an unknown kind
- [x] run the four gates - must pass before next task
- ➕ as built: `Step {started_at: Option<OffsetDateTime>, kind}` instead of a `seq` (a row's per-run `seq` and an event `seq` are different counters, and dedup lives in `Run::last_seq`); `Run.id` is `Option<RunId>` (`None` while queued); `Run::mark_stopping()` sets `Stopping` on a live run; `Run.terminal_reason` kept; a snapshot older than `last_seq` and any snapshot of a finished run are `Unchanged`; a `text.delta` after the finish is `Unchanged`; `message.created` of the run is `Unchanged`; a failed tool's output is its `error` when present, else its `summary`; a `task` row's `output` is parsed as the `step.task` body, falling back to `summary = output`; the golden loader moved to `core/src/v3/golden.rs` (`#[cfg(test)]`)

### Task 3: The async transport seam, the HTTP transport and the typed client

**Files:**
- Create: `core/src/v3/runtime.rs`, `core/src/v3/transport.rs`, `core/src/v3/http.rs`, `core/src/v3/client.rs`, `core/src/testing.rs` (`FakeDaemon`; new on clean HEAD, see Context)
- Modify: `core/Cargo.toml` (`tokio`, `reqwest`, `tokio-tungstenite`, `futures`, the `test-support` feature), `core/src/lib.rs` (`#[cfg(any(test, feature = "test-support"))] pub mod testing`), `core/src/v3/mod.rs`

- [x] `runtime.rs`: the lazily built one-worker runtime and `spawn` with abort-on-drop
- [x] `Transport` (`BoxFuture` methods), `Connection` (with `focus`), `ApiError`, `Backoff` per Technical Details
- [x] `HttpTransport::new(base_url, token)`/`with_handle`: reqwest with the bearer default header and a 5 s timeout, status mapping by hand, `connect` with `into_client_request` + `Authorization`, the 5 s connect timeout, the one socket task with `select!` over socket, `control` and the heartbeat deadline, `with_heartbeat_deadline` for tests
- [x] `Client` with every call of the contract as `async fn`, `Clone` over `Arc<dyn Transport>`, the `http` constructor (the `mock` constructor lands in Task 4); paths and query strings built in one place
- [x] `FakeDaemon` per Technical Details
- [x] tests against `FakeDaemon` (driven with `futures::executor::block_on`): the `Authorization` header is sent on REST and on the upgrade; `401` -> `Unauthorized`; `404`/`409`/`400`/`503` envelopes -> the matching `ApiError`; a non-envelope error body -> `Transport`; an empty `202` -> `Null` and `interrupt` returns `Ok(())`; `post` sends the body and decodes `202`; `connect(since)` puts `since` in the URL; scripted lines arrive as frames in order and a malformed line is skipped; a `focus` sent through `control` reaches the server as the contract's JSON; the server closing the socket closes `frames`; a silent server closes `frames` after the (shortened) heartbeat deadline; a server that never finishes the handshake fails `connect` with a timeout; dropping `Connection` ends the socket task while the server sends nothing; dropping a pending `get` aborts its request; `Backoff` doubles from 500 ms to the 30 s cap, adds the jitter fraction, and `reset()` starts over
- [x] run the four gates - must pass before next task
- ➕ as built: `Transport::post` takes `Option<Value>` (interrupt sends no body); the token is a `ClientToken` newtype whose `Debug` hides it; `HttpTransport::new` accepts only `http://` URLs (the `ws://` events URL is derived from it) and disables proxies; `ApiError::from_status` is public (documented, used by the handshake mapping too), a non-envelope `401` is `Unauthorized`; a rejected upgrade maps through the same envelope rules; the request starts when a `Client` call is made, not on first poll; `FakeDaemon` gained `Reply::Hang`, `Events::{Lines, LinesThenClose, Reject, NoHandshake}`, `sockets_closed()` and `dropped_requests()`; its upgrade callback is a `Callback` impl, not a closure (clippy's `result_large_err` on tungstenite's dictated `ErrorResponse`); reqwest 0.13.5, tokio 1.53.2, tokio-tungstenite 0.30.0, futures 0.3.34 with no TLS crate in the tree

### Task 4: The mock transport

**Files:**
- Create: `core/src/v3/mock.rs`
- Modify: `core/src/v3/mod.rs`, `core/src/v3/client.rs` (`Client::mock`)

- [x] `MockWorld`, `Scenario`, `Pace`, `MockTransport::new(scenario, pace)` per Technical Details: DTO serialization, every frame through `frames::decode`, the scripted run on every post routed by `addressed_agent_id`/lead, idempotent repeat, snapshots on connect and on focus, interrupt, `telegram_tick`, `play_a2a`, the `gap` and `unavailable` knobs, the replay ring, `step()`/`play_all()` for `Stepped`
- [x] tests (all `Stepped`, polled with `block_on`, no runtime): `Client::mock` fetches the world and every body decodes through `dto.rs`; the frames of a posted message arrive in the contract's order (`input.accepted`, `message.created{user}`, `run.started`, deltas, `step.text`, tool steps, task, `message.created{answer}`, `run.finished`) with `message.created{answer}` before `run.finished`; a post with `addressed_agent_id` runs that agent and the `202` names it, a post without it runs the lead, an unwired one is `Invalid`; interrupt mid-run ends with `interrupted` and no answer frame follows; interrupt after the end is `Conflict`; `play_a2a` yields two runs on one surface; a `focus` on a surface with a live run yields its `run.snapshot` with `as_of_seq`; a reconnect with `since` replays exactly the missed persisted frames and no ephemeral ones; `since` below the ring yields `gap`; the `unavailable` knob fails one `get`; a repeated `client_message_id` returns the first ids and queues nothing; `Realtime` is exercised by one `#[test]` that waits on the stream with a timeout
- [x] run the four gates - must pass before next task
- ➕ as built: `Client::mock(&MockTransport)` takes the transport the test keeps (instead of `(scenario, pace)`); `MockTransport` adds `pending()`, `head()`, `pump_control()` (applies queued `focus` frames, which `step()` also does) and `disconnect_all()`; the world is General (Jarvis lead + Magnet Feed on mention), Magnet Feed (with a parked live run that only snapshots) and Smart Home, four agents (Scout wired nowhere), 60 seeded messages with finished runs behind the answers; a gap is sent when `since < floor`, `floor` being the ring's oldest seq (the head while it is empty); `input.accepted` and `text.delta` reach focused surfaces only; interrupting a run whose `run.started` was not played yet is `Conflict`; lookups use the accumulator form because clippy's `manual_find` rejects an early `return Some` loop

### Task 5: Headless end-to-end over the mock

**Files:**
- Create: `core/tests/v3_mock.rs`

- [x] a `Session` test helper in the test file (not in the library) that models what a UI will do: `connect(None)`, read `hello` and set `last_seq = head`, fetch `surfaces`/`agents`/the first page, drain the buffered frames, keep `runs: BTreeMap<RunId, Run>`, keep aside deltas of unknown runs, ignore `input.accepted` of known inputs, drop a run from the map when a message with its `run_id` arrives, reconnect with `last_seq` on a closed channel
- [x] tests: the fresh-start sequence applies a frame emitted between `hello` and the end of the fetch exactly once; a post yields one run whose final `Run` has one tool step, two text steps, an empty segment and `RunState::Ok`, and the answer message carries its `run_id` and `client_message_id`; a delta played before `run.started` ends up in the segment; an interrupt leaves the run `Interrupted` with its text and no answer message; dropping the connection and reconnecting with `last_seq` replays nothing twice; the `gap` knob leads to a refetch and `last_seq = head`; `focus` on a surface with a live run yields a snapshot the reducer applies with no duplicate steps; `play_a2a` keeps two runs on one surface
- [x] run the four gates - must pass before next task
- ➕ as built: the `Session` keeps answered runs in a second map (`answered`) instead of dropping them, because the answer's `message.created` precedes `run.finished` and the test checks the final `Run`; a gap clears the runs and messages and refetches; the delta-before-start case is driven by applying a recorded batch in a swapped order, since the mock always plays `run.started` first

### Task 6: Verify acceptance criteria

- [x] every public item of `tuclaw_core::v3` and `tuclaw_core::testing` has rustdoc with an example where one makes sense; `mise exec -- cargo doc -p tuclaw-core --no-deps` is warning-free
- [x] `git diff --stat` shows no change under `app/` and none to `core/src/{model,store,schema,fixtures,grouping,paths}.rs`; `core/src/daemon.rs` does not exist
- [x] every route and frame of `docs/contracts/v3-client-contract.md` has a fixture and a decoder test; the contract file is unchanged
- [x] `cargo tree -p tuclaw-core` has no `gpui`; the four gates green: `mise run fmt-check`, `mise run lint`, `mise run test`, `mise run build`
- ➕ as verified: every public item of `v3` and `testing` has rustdoc (`cargo rustdoc -- -W missing-docs` reports only the crate-level doc, missing since v1) and `cargo doc` is warning-free; `core/src/{model,store,schema,fixtures,grouping,paths}.rs` are unchanged against `master` and `core/src/daemon.rs` does not exist; `app/` did change on this branch, but only in two commits Pavel asked for separately (`15ac580` the app bundle and menu, `83f6876` the toolbar and sidebar), none from this plan; the contract copy changed once, mirroring tuclaw's 2026-10-03 clarifications (`input_id` may be null, typed `usage`/`context_usage`); ➕ `posted.json`, `posted_without_input.json` and `error.json` fixtures were added so every route has one

### Task 7: [Final] Update documentation

- [x] `CLAUDE.md`: the crate split gains `v3/` (one line per file) and `testing.rs`; a short "The v3 client" section: the async seam, the core-owned runtime and its abort-on-drop futures, the single socket task, the reducer's text model and ordering facts, the mock's two pacings and the test rule (stepped in tests, never a foreign thread)
- [x] `README.md`: one paragraph naming `tuclaw_core::v3` and the contract file
- [x] move this plan to `docs/plans/completed/`

## Post-Completion

*Items requiring manual intervention or external systems - informational only*

**Before the farm starts (Pavel):** discard the uncommitted 2026-09-03 iteration (`git checkout -- . && git clean -fd` on `tuclaw-desktop-v1` after saving anything wanted - the plan assumes clean HEAD and stops if `core/src/testing.rs` from the iteration is present); decide whether PR #1 is merged first or this branch is cut from `tuclaw-desktop-v1`; commit this plan, `docs/contracts/v3-client-contract.md` and `core/testdata/v3/`.

**C1-side items this library relies on** (tuclaw repo, `docs/plans/20261003-v2-c1-client-api.md`): `messages.client_message_id` and `messages.input_id` so idempotent retries return the first ids; `run.snapshot` with `as_of_seq` and the in-progress segment as `text`, empty `text` for a run adopted after a daemon restart, and a snapshot per live run on `focus`; an interrupted run producing no answer message; the golden JSON in `internal/api/testdata/v3/` kept identical to `core/testdata/v3/` here (both copied from the contract, never imported across repos).

**How a later UI task wires it** (the seams, so the small tasks Pavel asks for can start from here; none of this is built in this plan):
- **Fresh start**: `Client::connect(None)` FIRST, read `hello` from `frames` and keep `head` as `last_seq`, then `surfaces()`/`agents()`/`messages()` off the UI thread, then drain the frames buffered in the same `Connection` through the reducer (idempotent by message id and run id) - never open a second socket for the UI; hand the one `Connection` to the loop that owns it. The `Session` helper in `core/tests/v3_mock.rs` is the reference implementation.
- **The loop**: one foreground task (`cx.spawn`) that selects between `Connection::frames` and a command channel the UI state holds (focus, load a surface's page), as zed's `Client` drives its `incoming` stream; REST futures are awaited in `cx.background_spawn`; on a closed `frames` reconnect with `connect(Some(last_seq))` after `Backoff::next_delay` waited on `cx.background_executor().timer(..)` (so tests `advance_clock` through it), with a connection status the status bar shows (zed: `Connecting`, `Connected`, `ConnectionLost`, `Reconnecting`, `ReconnectionError {next_reconnection}`); on `gap` refetch the snapshots, set `last_seq = head`, and drop every live run the following `run.snapshot`s do not re-send.
- **Focus**: `Connection::focus([selected])` after every `hello` and on every selection change; the daemon answers with a `run.snapshot` per live run of a newly focused surface.
- **Runs**: a `BTreeMap<RunId, Run>`; `Run::queued` on `input.accepted` (unless the input is known), `Run::start`/`from_started` on `run.started`, deltas of unknown runs kept aside, `apply` for the rest; a run leaves the map when a message with its `run_id` arrives (answer or error notice), and a finished run with no such message (interrupted, or an error without its notice) stays visible until the surface's page is refetched - never on a timer. The answer's `run_summary` is partial (the daemon writes it before `run.finished`); refresh it from `Run::summary()`.
- **Posting**: an optimistic local row with `ClientMessageId::random()`; `post` off the UI thread; reconcile on the `Posted` ids and on `message.created` by `client_message_id`; a failed post hands the text back to the composer (GPUI composers clear on a synchronous `Ok`, so the failure path is an event, not a return value). `addressed_agent_id` only for a leading `@ident` of an agent in `Surface.agents` (the daemon answers `400` otherwise).
- **Tests**: `Client::mock(scenario, Pace::Stepped)` and `step()`/`play_all()` from the test thread, then `cx.run_until_parked()`; never `Realtime` or `HttpTransport` under `#[gpui::test]` - GPUI's test scheduler forbids parking on a wake from a foreign thread by default (`forbid_parking`; zed's own escape hatch is `cx.executor().allow_parking()`, for the rare test that must).
