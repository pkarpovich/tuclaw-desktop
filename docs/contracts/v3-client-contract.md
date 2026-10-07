
Acceptance scenario this contract must carry: a conversation in an existing Telegram topic, from the desktop, without Telegram limits - plain Markdown in and out, real token streaming, the run visible step by step (text segments, tool calls with input and result, background tasks), the final answer without the `<details>Thinking` fold. A desktop message is not echoed to Telegram; the agent's answer is mirrored there as today.

## Transport and auth


Acceptance scenario this contract must carry: a conversation in an existing Telegram topic, from the desktop, without Telegram limits - plain Markdown in and out, real token streaming, the run visible step by step (text segments, tool calls with input and result, background tasks), the final answer without the `<details>Thinking` fold. A desktop message is not echoed to Telegram; the agent's answer is mirrored there as today.

## Transport and auth

- Base: `http://<daemon>:9090/api/v3` (the existing `TUCLAW_HTTP_ADDR` server), event socket `ws://<daemon>:9090/api/v3/events`. LAN and Tailscale only.
- `/api/v3` is always mounted and open to the LAN and Tailscale (decided by Pavel 2026-10-04: no token for now; Authelia may front it later). The daemon keeps one optional knob, `TUCLAW_CLIENT_TOKEN`: when it is set, every v3 request, REST and WebSocket upgrade, must carry `Authorization: Bearer <token>` and a missing or wrong header is `401`; when it is unset, no header is needed and any `Authorization` header is ignored. A client sends the bearer when it has a token configured, and learns the mode from `hello.capabilities.auth`.
- `/health`, `/api/v1`, `/api/v2` and the mini-app stay as they are, unauthenticated, for one release.

## Conventions

- JSON, `snake_case` keys everywhere (REST bodies, event payloads), the same spelling the agent edge already uses.
- ids: `surface_id`, `message_id`, `agent_id`, `input_id` are integers (int64); `run_id` is a string (UUID from the agent); `seq` is an integer.
- times: RFC 3339 UTC strings.
- errors: non-2xx with body `{"error": {"code": "<snake_case_code>", "message": "<human text>"}}`. Codes used in v3.0: `unauthorized`, `not_found`, `invalid_request`, `conflict`, `unavailable`.
- A client ignores unknown fields and unknown event types. `run.finished` is the only terminal signal for a run.

## REST (v3.0)

### `GET /surfaces`

The sidebar. Every surface the daemon knows (today: Telegram topics), ordered by `sort_order`.

```json
[{
  "id": 1,
  "kind": "channel",
  "name": "General",
  "sort_order": 0,
  "last_message_at": "2026-10-03T15:26:00Z",
  "lead_agent_id": 1,
  "agents": [{"agent_id": 1, "role": "lead", "listens": true}, {"agent_id": 3, "role": "mention", "listens": false}],
  "bindings": [{"channel": "telegram", "external_id": "-1001234567890:0", "mirror": "agent_only"}],
  "live_run": {"run_id": "6763eb02-...", "agent_id": 1}
}]
```

- `kind` is `channel` for every surface in v3.0 (B1's `surfaces.kind`, `DEFAULT 'channel'`; `dm` arrives with step D).
- `role`: `lead` = `engage_mode=default`, `mention` = `engage_mode=mention`; `listens` = `ignored_message_policy=accumulate`.
- `live_run` is present when an agent is running a turn whose run is stamped with this surface, else absent.

### `GET /agents`

```json
[{
  "id": 1,
  "name": "Jarvis",
  "ident": "tuclaw_bot",
  "description": "",
  "bot_username": "tuclaw_bot",
  "model": "opus[1m]:medium",
  "state": "idle",
  "live_run": null,
  "home_surface_id": 1
}]
```

- `ident` is what `@mentions` resolve to (`delegate_name -> bot_username -> name`, as `AgentIdent`).
- `state`: `idle` or `running` (a live run anywhere); `live_run` is `{run_id, surface_id}` while running (same shape as on surfaces, so "busy - in #General" needs no second lookup), else `null`. `model` is the effective model (session override, else the daemon default).
- `home_surface_id` = `sessions.surface_id` (one session per agent since B1).

### `GET /surfaces/{id}/messages?before=<message_id>&limit=<n>`

One page, oldest first inside the page; without `before` it is the newest page. `limit` default 50, max 200.

```json
{
  "messages": [{
    "id": 9192,
    "surface_id": 1,
    "kind": "answer",
    "author": {"kind": "agent", "agent_id": 1},
    "addressed_agent_id": null,
    "reply_to_message_id": null,
    "text": "...",
    "run_id": "6763eb02-...",
    "origin": "user",
    "channel": null,
    "client_message_id": null,
    "created_at": "2026-10-03T15:26:13Z",
    "run_summary": {"status": "ok", "step_count": 6, "tool_count": 1, "duration_ms": 13029}
  }],
  "has_more": true
}
```

- `kind`: `user`, `answer`, `post`, `notice`, `a2a` (B2's set; `prompt` rows are never returned).
- `author.kind`: `user`, `agent`, `system`; `agent_id` present for `agent` (and for notices a bot posted for an agent).
- `text` is the answer verbatim, Markdown, no Thinking fold.
- `origin` is B2's `message.created` origin (what caused the message: `user`, `a2a`, `scheduled`, ...); it is `null` on rows written before step B2 (the daemon did not record it then), so a client decodes it as optional. `channel` is where a `user` message was typed: `telegram` or `desktop` (`null` for agent and system messages).
- `client_message_id` echoes the id a v3 client posted the message with (else `null`), so `message.created` can be matched to the client's optimistic row whether or not the `202` arrived first.
- `run_summary` is present on a message with a `run_id` and lets the client draw "6 steps, 1 tool, 13 s" without fetching the run; it is `null` on a message without a run.

### `POST /surfaces/{id}/messages`

```json
{"text": "Лисички появились в магазине...", "addressed_agent_id": null, "client_message_id": "8b0c...-uuid"}
```

- Routes exactly like a Telegram message on that surface: `addressed_agent_id` (a mention) wins, else sticky, else the lead. An `addressed_agent_id` not wired on the surface is not an error: it routes the way Telegram routes a tag of a bot that is not in the topic, and the `202`'s `agent_id` names the agent actually woken. The user row is written with `channel` origin `desktop` and is NOT mirrored to Telegram (`mirror = agent_only`); the answer is.
- `client_message_id` (UUID chosen by the client, required) makes the post idempotent: a retry with the same id returns the first result instead of a second message, and the id is echoed on the message (above). The id is unique across all surfaces: posting an id already used on a DIFFERENT surface answers `409 conflict`.
- `202 {"message_id": 9193, "input_id": 42, "agent_id": 1}`. `input_id` `null` means the message was stored but not queued: the daemon posted an error notice on the surface (as it does for a Telegram message), or it restarted before queuing. `agent_id` is `null` in that case too. A retry with the same `client_message_id` returns the same answer and queues nothing; to try again the client posts again with a NEW `client_message_id`. The reply arrives on the event socket: `message.created` for the user row, then the run's events.
- While the woken agent already has a live run, the input waits behind it as today (held). Steering into the live run is decided in the C1 plan, not here; the client contract does not change either way.

### `GET /runs/{id}`

```json
{
  "run": {"id": "6763eb02-...", "agent_id": 1, "surface_id": 1, "origin": "user", "kind": "user", "status": "ok", "terminal_reason": "success", "error": null, "started_at": "...", "finished_at": "...", "usage": {"input_tokens": 0, "output_tokens": 0, "cache_read_tokens": 0, "cache_creation_tokens": 0}, "context": {"tokens": 323968, "max_tokens": 1000000, "model": "claude-opus-5-5[1m]"}},
  "steps": [{"seq": 1, "kind": "text", "output": "...", "started_at": "..."}, {"seq": 2, "kind": "tool", "tool_use_id": "toolu_...", "name": "Bash", "input": {}, "output": "...", "status": "ok", "started_at": "...", "finished_at": "..."}]
}
```

- `status`: `running`, `ok`, `error`, `interrupted` (B1's `runs.status`).
- A step is one generic row for all kinds: `{seq, kind, tool_use_id?, name?, input?, output?, status?, started_at, finished_at?}`; every field but `seq`, `kind` and `started_at` may be absent, and a client decodes them all as optional. Per `kind` (B1's `run_steps.kind`): `text` - `output` is the finished text segment; `tool` - `tool_use_id`, `name`, `input` (JSON, clamped as in the events), `output` (summary or error, 2 KB), `status` `running|ok|error`; `task` - `name` is the task type, `status` the task state, `output` the `step.task` JSON body as text; `status` - `name` is the status, `output` the detail. For a run a reset cleared, text steps are gone, tool steps remain (B1 behavior).

### `POST /runs/{id}/interrupt`

Stops the live run (the agent's SDK `interrupt()`); `202`, the run then ends with `run.finished{terminal_reason: "interrupted", is_error: false}` and produces NO answer message, even when partial text was streamed (the client keeps the streamed text, marked stopped; Telegram gets nothing). `409 conflict` when the run is not live.

## Event socket

`GET /api/v3/events?since=<seq>` upgrades to a WebSocket. The client stores the last `seq` it applied and passes it on reconnect.

Fresh start (no stored `seq`): connect WITHOUT `since` first -> receive `hello{head}` -> fetch `GET /surfaces`, `GET /agents` and the message pages it shows -> apply the frames that arrived meanwhile, idempotently by message id and run id. Connecting before fetching closes the window in which events between a REST snapshot and the socket would be lost; no replay is sent without `since`.

### Server frames

Every frame is one JSON text message:

```json
{"v": 1, "seq": 1201, "type": "message.created", "surface_id": 1, "run_id": null, "at": "2026-10-03T15:26:00Z", "payload": {}}
```

`seq` is absent on ephemeral frames. Connect sequence:

1. `hello` `{head, floor, server_time, capabilities: {events: [...], ops: [...], auth: "none" | "bearer"}}` - `head` the newest seq, `floor` the oldest still kept (30-day retention). `capabilities.events` lists the persisted and ephemeral event types this daemon sends (the protocol frames `hello`, `gap` and `run.snapshot` are not listed); `capabilities.ops` lists the client frame types it accepts (`focus`); `capabilities.auth` is the auth mode.
2. if `since + 1 < floor` (events the client never saw were pruned) or `since > head` (a seq this log never had, e.g. after a reset): `gap {floor}` - the client refetches `GET /surfaces` and the message pages it shows, then continues from `head`; else every persisted event with `since < seq <= head`, in order.
3. one `run.snapshot` per live run: `{run_id, agent_id, surface_id, started_at, as_of_seq, text, steps: [...]}` - `steps` as in `GET /runs/{id}`; `as_of_seq` is the newest event seq whose effects the `steps` already contain (the server reads both in one database snapshot), and the client ignores that run's persisted frames with `seq <= as_of_seq` so no step is applied twice; `text` is the IN-PROGRESS text segment only (the agent streams every assistant text block and records each finished block as a `step.text`, so a client renders the text steps plus this one current segment, and clears its current segment on every `step.text` and on `run.reset`).
4. live events.

Heartbeat: WebSocket ping/pong every 20 s from the server; the client reconnects with `since` when pongs stop.

### Persisted events (have `seq`, replayable)

The agent's run events pass through unchanged (same names and payloads as the agent edge, `internal/agentproto`), stamped with `seq`, `surface_id` and `run_id`:

- `run.started {agent_id, input_ids, origin, turn_kind}` (the daemon adds `agent_id`; `chat_id`/`topic_id` are dropped)
- `step.text {text}` - one finished text segment of the run
- `step.tool_started {tool_use_id, name, input, parent_tool_use_id?}` - `input` is the tool's JSON input object as sent; over 16 KB the agent replaces it with `{"truncated": "<first 16 KB of the JSON text>"}`
- `step.tool_finished {tool_use_id, is_error, error?, summary}` - `summary` and `error` are plain text clamped to 2 KB by the agent, so a client must not assume they are complete
- `step.task {task_id, task_type, state, description?, summary?}` - background subagents / Bash
- `step.status {status, detail}`
- `run.reset {}` - the client drops the run's streamed text and text steps (tool steps stay)
- `run.finished {is_error, error?, terminal_reason, usage?, context_usage?}` - `result` is not repeated here; the answer arrives as `message.created`. `usage` is `{input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens, num_turns, duration_api_ms}`; `context_usage` is `{total_tokens, max_tokens, percentage, model}` (the daemon renames the agent edge's camelCase fields)
- `message.created {message}` - `message` exactly as in the messages page

Order guarantee (B2): for a user or a2a run with a visible answer, `message.created{run_id}` comes before that run's `run.finished`; a scheduled run's answer may come after.

A finished run may have no answer message at all: a run the user interrupted, a delegated a2a handoff, an a2a turn answering `[SILENT]`, and a user or voice run whose answer is only `[SILENT]` (the agent already said what it had through posts). A `run.finished{is_error: false}` with no answer before it just closes the run; there is nothing to show.

### Ephemeral events (no `seq`, never replayed)

- `text.delta {text}` - streamed answer text, coalesced per client on a 50 ms tick; only for focused surfaces.
- `input.accepted {input_id, surface_id, agent_id}` - the daemon took a posted message; the client may show "queued" until `run.started`.

### Client frames

Same envelope as server frames, without `seq`: `{"v": 1, "type": "<type>", "payload": {...}}`. A malformed client frame closes the socket (status 1003).

- `focus` `{"v": 1, "type": "focus", "payload": {"surface_ids": [1, 3]}}` - which surfaces the client shows; ephemeral frames flow only for those. Persisted events always flow. Default after connect: none focused. For every live run on a surface that becomes focused (newly in the set), the server sends a `run.snapshot` right away, so a surface selected mid-run starts from a whole segment instead of mid-sentence.

## Out of v3.0 (step D or later)

Creating channels and DMs, threads (`thread_root_id`), read state and unread counts, cards and dialogs (`ask`), attachments and artifacts, message edit (`message.updated`), search, task and wiring management over v3 (v2 keeps serving the mini-app), agent create/update/delete, `thinking.delta` (the agent emits no thinking stream today).

## v3.1 additions

Agreed 2026-10-04 (daemon plan `docs/plans/completed/20261004-v3.1-voice-attachments.md`): the original of every voice message, played by the client through the daemon. Additive only; a v3.0 client that ignores unknown fields is unaffected.

- Every message in `GET /surfaces/{id}/messages` and in `message.created` gains `attachments`, an empty list when there are none:

  ```json
  "attachments": [{"id": 1, "kind": "voice", "mime": "audio/ogg", "size_bytes": 1166097, "duration_ms": 294474}]
  ```

  `id` is an integer (int64) like every other v3 id; `kind` is `voice` in v3.1; `mime` is the stored object's type (`audio/ogg` for Telegram voice, `audio/mp4` for the watch); `duration_ms` is `null` when unknown, never `0`.
- `GET /attachments/{id}` (bearer like every v3 route) streams the original: `200` with `Content-Type` = the attachment's `mime` and `Content-Length`; `Range: bytes=a-b` or `bytes=a-` answers `206` with `Content-Range: bytes a-b/<size>` and the range's length as `Content-Length`; `Accept-Ranges: bytes` on both. Any other range (several ranges, a suffix `bytes=-n`, a start past the end) answers `416` with `Content-Range: bytes */<size>`. Unknown id = `404 not_found`; the object missing from storage = `404 not_found` with message "attachment object missing"; storage unreachable = `503 unavailable`.
- Capabilities: unchanged (`capabilities.ops` lists client frame types, not REST routes; a route's presence is its capability).

## v3.2 additions: avatars and the user profile

Agreed 2026-10-04 with the desktop. tuclaw owns the pictures of its agents and of the user; Telegram is at most a one-time seed (`tuclawd --seed-avatars`), never a source the contract knows about. Additive only.

- Every agent in `GET /agents` gains `avatar_url`: a relative, versioned path such as `/api/v3/agents/1/avatar?v=AQADbVsx`, fetched with the same bearer as every v3 call, or `null` when the agent has no picture (the client draws initials). `v` is opaque to the client. A new picture is a new `v`, so a versioned URL may be cached for good.
- `GET /me` -> `{"name": "Pavel", "avatar_url": "/api/v3/me/avatar?v=AgADq2wx"}`. Before anything is set it is `{"name": "You", "avatar_url": null}`, never a 404.
- `GET /agents/{id}/avatar` and `GET /me/avatar` stream the picture with its stored `Content-Type` (`image/png`, `image/jpeg` or `image/webp`) and `Content-Length`. With the current `?v=` the answer carries `Cache-Control: private, max-age=31536000, immutable`, without one `no-cache`. No picture, an unknown agent or a `v` that is no longer current = `404 not_found`: the client keeps the initials, it is never an error banner.
- `PUT /agents/{id}/avatar` and `PUT /me/avatar` take the raw image as the body (png, jpeg or webp, sniffed from the bytes, at most 2 MiB) and answer `200 {"avatar_url": "..."}`. Not an image or too large = `400 invalid_request`; unknown agent = `404 not_found`.
- `DELETE /agents/{id}/avatar` and `DELETE /me/avatar` answer `204`; the agent or the user is back to initials.
- `PATCH /me` `{"name": "Pavel"}` renames the user and answers like `GET /me`; an empty name = `400 invalid_request`.
- A changed picture shows up on the next `GET /agents` or `GET /me`; there is no socket event for it.
- The writes go through the daemon's operations (`avatar.set`, `avatar.clear`, `user.rename`), so an agent tool can later take the same path; today they are user-only.

## v3.3 additions: agent settings

Agreed 2026-10-04 with the desktop (its agent settings panel: description, model and topic wiring, each saved as it changes). Additive only. The writes go through the daemon's operations `agent.update`, `surface.wire` and `surface.unwire`, user-only.

- `PATCH /agents/{id}` `{"description"?: string, "model"?: string|null}`. An absent field stays as it is.
  - `description` is at most 140 characters (runes); an empty one clears it. An existing longer description is still returned as it is.
  - `model` is a spec like `/model` takes (`opus[1m]:high`); `null` or `""` clears the session override, so the agent runs the daemon default. A connected agent switches at once (`set_model` control). Nothing is posted to any chat.
  - Answers `200` with the agent in the `GET /agents` shape. Unknown agent = `404 not_found`; a malformed body or a description over the limit = `400 invalid_request`.
- `PUT /surfaces/{sid}/agents/{aid}` `{"role": "lead"|"mention", "listens": bool}` adds the agent to the surface or changes its wiring; both fields are required.
  - `role: lead` demotes the current lead to `mention` in the same transaction.
  - Answers `200` with the surface in the `GET /surfaces` shape.
  - Unknown surface or agent = `404`. Another role, or a missing or non-boolean `listens` = `400`. Turning the current lead into a mention = `409 conflict`: a surface always has a lead, so another agent becomes lead first.
- `DELETE /surfaces/{sid}/agents/{aid}` answers `204`. Removing the lead = `409 conflict`. An agent that is not wired there = `204` (idempotent). An unknown surface or agent = `404`.
- No socket events: the client refetches `GET /surfaces` and `GET /agents` after a write. An agent's topics come from `surfaces[].agents[]`, its home topic from `home_surface_id`; Undo is the client replaying the previous wiring with `PUT`.

## v3.4 additions: the user's profile, automations, fire marks

Agreed 2026-10-04 with the desktop (Pavel's asks: his own profile, the mini-app's triggers page as an Automations tab, and a visible mark in a channel when an automation fired). Additive only.

- **Profile.** `GET /me` gains `description` (`""` when unset). `PATCH /me` takes `{name?, description?}`: an absent field stays, the name must not be empty, the description is at most 280 characters (an empty one clears it). It answers like `GET /me`. The write goes through the `user.update` operation.
- **Automations** are the scheduled tasks the agents create. The client controls them; it does not create or edit them.
  - `GET /tasks` lists the active and paused tasks; `?status=all` adds the completed and cancelled ones last run (or created) in the past 7 days. Each task:

    ```json
    {"id": "task-1759500000000000000-1a2b3c4d", "agent_id": 3, "surface_id": 4,
     "prompt": "Check the feeds for new releases",
     "schedule": {"type": "cron", "value": "0 9 * * *"}, "recurring": false,
     "condition": "check-feeds.sh", "status": "active",
     "next_run_at": "2026-10-05T07:00:00Z", "last_run_at": "2026-10-04T07:00:00Z",
     "last_outcome": "skipped", "active_from": null, "active_until": null,
     "created_at": "2026-09-20T10:00:00Z"}
    ```

    `id` is a string, the one exception to the integer ids: agents name tasks by it. `schedule.type` is `once`, `cron`, `interval`, `poll_until` or `event`. `agent_id` is the task's session's agent, `null` when that session is gone. `surface_id` is `null` when the task has no surface. `condition` is the pre-check command or `null`. `status` is `active`, `paused`, `completed` or `cancelled`. `last_run_at` and `last_outcome` are the newest attempt's time and outcome (`ran`, `failed` or `skipped`), both `null` before the first; a condition skip is an attempt, so a polled task reads "skipped 20:00".
  - `GET /tasks/{id}` is one task.
  - `GET /tasks/{id}/runs?limit=20` (at most 200) lists its attempts, newest first: `[{"at", "outcome": "ran"|"failed"|"skipped", "duration_ms", "error"?}]`. An attempt is one try; a fire that failed and was re-run is two attempts.
  - `POST /tasks/{id}/pause` and `POST /tasks/{id}/resume` answer `200` with the task. Pausing a task that is not active, or resuming one that is not paused, is `409 conflict`.
  - `DELETE /tasks/{id}` cancels the task and keeps its history: `204`, also for a task already finished.
  - An unknown id is `404` on every route.
- **Fire marks.** After each fire's final outcome (once per fire, never per attempt) the daemon records a persisted `task.fired` event on the task's surface. It is sent as a frame of the same type, with `run_id` `null` in the envelope:

  ```json
  {"v": 1, "seq": 1290, "type": "task.fired", "surface_id": 4, "run_id": null, "at": "2026-10-04T07:00:19Z",
   "payload": {"task_id": "task-1759500000000000000-1a2b3c4d", "outcome": "ran", "run_id": "0b9d2c4e-5a61-4f7e-8c3d-1e2f3a4b5c6d", "message_id": 9301}}
  ```

  - `outcome` is one of:
    - `ran`: `message_id` is the answer it posted, so the client attaches the mark to that answer instead of adding a row;
    - `silent`: it answered `[SILENT]` or nothing visible;
    - `skipped`: its condition said no. A condition task polled every few minutes marks every poll, so the client collapses a task's consecutive skipped marks into one row;
    - `failed`: `message_id` is the failure notice when one was posted, and `error` is the reason.
  - `run_id`, `message_id` and `error` are omitted when absent.
  - A fire cut short by a daemon shutdown leaves no mark. Neither does a run handed over to the agent across a restart: its answer arrives on its own.
- **Marks in history.** Every messages page gains `automations`, the fires of the time the page covers. That time runs from its oldest message (from the beginning when nothing older exists) to just before the oldest message of the next newer page (to now on the newest page), so consecutive pages cover the history with no gap. Each entry is `{task_id, at, outcome, run_id?, message_id?}`, in time order. They come from the event log, which keeps 30 days, so older pages have none.

## v3.5 additions: voice messages from the client

Agreed 2026-10-04 with the desktop (its Talk button records a voice message).

- `POST /surfaces/{id}/voice` takes the raw recording as the body.
  - `Content-Type` is `audio/mp4` (AAC, `.m4a`) or `audio/ogg` (Opus), the two formats the transcription is proven on. Anything else = `400`.
  - The header `X-Client-Message-Id: <uuid>` is required and makes the post idempotent exactly like a text post.
  - An optional `?addressed_agent_id=` routes it like a mention.
- The daemon then does what it does for a Telegram voice message:
  - stores the recording in its voice bucket;
  - transcribes it;
  - posts the transcript through `message.inbound{is_voice: true}`, with the recording as the message's `voice` attachment.

  The message arrives like a Telegram voice: the text is the transcript with the `[Voice message]` header, plus the attachment (`duration_ms` is `null`; the daemon does not parse the recording).
- The call blocks until the transcript exists and the message is posted, then answers `202 {message_id, input_id, agent_id}` like a text post.
- A failed or empty transcription = `502` with the code `transcription_failed` and no message stored. A retry with the same id transcribes again, while a retry after a `202` answers with the first ids.
- An empty body = `400`. A body over 20 MiB = `413` with the code `too_large`. The duration cap (10 minutes) belongs to the client.
- An unknown surface = `404`.

## v3.6 additions: read state

Agreed 2026-10-05 with the desktop; Pavel chose an unread badge per surface plus a "new" divider in the feed. Only the desktop moves the cursor: Telegram keeps its own unread state, and nothing read or posted there counts.

- `GET /surfaces` gains two fields:
  - `last_read_message_id`: the surface's read cursor, `null` before anything was read. The migration starts every existing surface at its newest message.
  - `unread`: the count of messages past the cursor that the user did not write. Scheduler prompts never count; fire marks are not messages and never count. Answers, posts, a2a messages and notices do.
- `POST /surfaces/{id}/read` `{"message_id": 9192}` moves the cursor forward only (it keeps the larger of the two) and answers `200 {"last_read_message_id", "unread"}`.
  - A `message_id` that is missing, not positive, or not a message of this surface = `400`. An unknown surface = `404`.
  - Posting does not move the cursor; the client sends `read` itself.
- Every accepted read, including one that did not move the cursor, records a persisted `surface.read` event. It goes out as a frame (`surface_id` in the envelope, `run_id` null, payload `{last_read_message_id, unread}`), so every open client converges on the same badge.
- Between reads, a client counts new messages itself from `message.created`: one per message not written by the user past its cursor, on a surface it is not showing.

## v3.7 additions: organizing the sidebar

Agreed 2026-10-05 with the desktop (Pavel's asks: groups with a title and an emoji, renaming and archiving channels, a channel browser). Everything here is how the desktop shows surfaces. It never touches the channel: Telegram keeps its topic names, and agents and Telegram keep working in an archived surface.

- **Surfaces.** `GET /surfaces` changes and gains fields:
  - `name` is now the effective name: the desktop's `display_name`, else `topic_name`;
  - `topic_name` is the channel's own name (`General` for topic 0);
  - `display_name` is the desktop's name or `null`;
  - `group_id` is the group or `null` (ungrouped surfaces come first, with no header);
  - `archived_at` is `null` unless the surface is archived.

  `sort_order` is still the position, now within its group. The agents' roster and prompt stamps keep the topic name.
- **`PATCH /surfaces/{id}`** takes `{display_name?, archived?, group_id?}`. An absent field stays as it is.
  - `display_name` `null` or `""` goes back to the topic name; at most 64 characters.
  - `archived` is a boolean. Archiving keeps the first time; a new message never unarchives.
  - `group_id` `null` ungroups the surface.

  It answers `200` with the surface. An unknown surface or group = `404`, a bad field = `400`.
- **`PUT /surfaces/order`** takes `[{"id", "group_id" (null = ungrouped), "sort_order"}]` for drag and drop and answers `200` with the full `GET /surfaces` list.
  - The position is the same order `/api/v1` (the Watch) and the mini-app read.
  - An empty list or an entry without an id = `400`. An unknown surface or group = `404`, and then nothing is changed.
- **Groups.**
  - `GET /groups` returns `[{"id", "name", "emoji" (null), "sort_order"}]` in order.
  - `POST /groups {name, emoji?}` answers `201` with the group, placed last.
  - `PATCH /groups/{id} {name?, emoji?, sort_order?}`: an empty emoji clears it. It answers `200` with the group.
  - `DELETE /groups/{id}` answers `204`; its surfaces become ungrouped.
  - A name is 1 to 64 characters and an emoji at most 8.
- **Events (persisted).**
  - `surface.updated {surface}` carries the whole surface as `GET /surfaces` shows it now. It is sent for every surface a rename, archive, regroup, reorder or group deletion changed.
  - `groups.changed {groups}` carries the whole group list.

  Both are read at send time, so a replay shows the current state. A deleted surface's update is skipped.
- **Client side.** Collapsing a group is per client. An archived surface's `unread` is still computed (the browser can show it), and the client leaves it out of every badge.

## v3.8 additions: mark as unread

Agreed 2026-10-05 with the desktop. Pavel wants a surface to keep a badge until he gets back to it. This is Telegram's semantics: a flag, not a cursor moved back. The v3.6 cursor stays forward-only, so clients never race on it.

- **`GET /surfaces`** gains `marked_unread` (bool) next to `unread` and `last_read_message_id`. The client shows a dot when `unread == 0 && marked_unread`, else the count.
- **`POST /surfaces/{id}/unread`** (no body) sets the flag and leaves the cursor. It answers `200 {"last_read_message_id", "unread", "marked_unread": true}`. An unknown surface = `404`.
- In every read-state answer and `surface.read` frame, `last_read_message_id` is `null` while a surface has no cursor, as in `GET /surfaces`.
- **`POST /surfaces/{id}/read`** now clears the flag, even when the cursor does not move ("I opened it" is the signal). Its answer gains `marked_unread`.
- **`message_id` on `/read` is now optional.** An empty body or `{}` only clears the flag, so a surface with nothing to read can be cleared too. A `message_id` that is given is validated as before.
- **Both writes record a `surface.read` event**, whose payload gains the flag: `{last_read_message_id, unread, marked_unread}`. Every open client converges with no new frame type.
- Telegram never touches the flag, the same as the cursor.

## v3.9 additions: replies to the user

Agreed 2026-10-05 with the desktop (Pavel's pick: the sidebar number counts only the replies to his own questions; everything else unread is a grey dot).

- **`unread_replies`** sits beside `unread`, which stays the total, in four places:
  - `GET /surfaces`;
  - the `surface.read` payload;
  - the `/read` answer;
  - the `/unread` answer.

  It counts the messages past the cursor written by an agent (`author.kind = agent`) in a run the user's own message started (`origin = user`), of kind `answer` or `post`. The posts count because a run may reply partly or wholly through them. a2a handoffs, notices and scheduled or a2a-origin messages do not count.
- **`reply_to_message_id` on an answer** is now the user message the run answered: the message whose input the run claimed first. It stays `null` for scheduled, a2a-origin and task-notification answers. A migration backfills the existing answers whose run's input is still kept (inputs are pruned after 30 days). Telegram is unaffected; it still replies only for a2a handoffs.

## v3.10 additions: one surface's events as server-sent events

Agreed 2026-10-07 with the watch app. watchOS allows a WebSocket only to an app that is streaming audio or is in a VoIP call (Apple TN3135), so the watch cannot use `/api/v3/events`. Plain HTTP streaming works.

- **`GET /api/v3/surfaces/{id}/events?since=<seq>`**, `text/event-stream`. It carries the same frames as the socket, scoped to one surface.
  - Each frame is `event: <type>`, then `id: <seq>` on persisted frames only, then `data: <the same envelope JSON as the socket>`, then a blank line.
  - `since` is the last applied seq, as on the socket. A reconnect may send `Last-Event-ID` instead of `since`.
- **On connect**, frames arrive in this order:
  1. `hello{head, floor, ...}`.
  2. `gap{floor}` under the socket's rule: `since + 1 < floor` or `since > head`. After a gap, the live part starts from the head.
  3. With `since`, the replay of this surface's persisted events in `(since, head]`.
  4. One `run.snapshot` per running run on this surface.
  5. Live frames.
- **Only this surface's persisted events** are sent, so `groups.changed` and other surfaces' events are absent.
- **The surface counts as focused** from the start, so `text.delta` (coalesced per run, as on the socket) and `input.accepted` flow.
- **No client frames.**
- **A `: ping` comment every 20 s** keeps the stream and the client's request timeout alive.
- **Errors before the stream starts** use the usual JSON error body:
  - an unknown surface is `404`;
  - a surface id or `since` that is not an integer is `400`.
- **Answering a voice post:** open the stream first, then `POST /surfaces/{id}/voice`. Match `run.started.input_ids` against the returned `input_id`. Render `text.delta` / `step.text` / `run.reset`. Finish on the run's `message.created` and `run.finished`. This is the socket's "connect before fetching" rule.
- Fixture: `testdata/v3/sse_surface.txt` is a short transcript built from the golden frames: hello, input.accepted, run.started, text.delta, step.text, a keepalive, message.created, run.finished.

## v3.11 additions: push notifications

Agreed 2026-10-07 with the watch app. The daemon pushes an alert to every registered Apple device through APNs, so the watch rings even when the app is not running and Telegram can stay muted.

- **`POST /api/v3/devices`** `{token, platform, bundle_id, environment}` registers a device and answers `204`. It is an upsert by `token`, so the app sends it on every launch.
  - `token` is the APNs device token in hex, 16 to 200 characters.
  - `platform` is `watchos` or `ios`.
  - `bundle_id` is the app's bundle id; it becomes the `apns-topic` of every push to this device.
  - `environment` is `sandbox` (a debug build from Xcode) or `production` (TestFlight and the App Store).
  - Anything else is `400`.
- **`DELETE /api/v3/devices/{token}`** unregisters a device and answers `204`, also for an unknown token.
- **Which messages push:** an agent's `answer` or `post`, and a `notice` an agent authored (a failed run, a failed task). Never the user's own messages, an `a2a` handoff, the daemon's own notices (command replies) or anything on an archived surface. Only live messages push: a message committed while the daemon was down stays unpushed.
- **The payload:**

  ```json
  {
    "aps": {
      "alert": {"title": "General", "subtitle": "magnet_feed", "body": "the message text"},
      "sound": "default",
      "thread-id": "surface-10"
    },
    "surface_id": 10,
    "message_id": 1234,
    "run_id": "…"
  }
  ```

  - `title` is the surface's name.
  - `subtitle` names the author only when it is not the surface's lead agent.
  - `body` is the message text as plain text (Markdown markup dropped, a link shown as its text), cut on a character boundary with `…` so the payload stays within APNs' 4 KB.
  - `thread-id` groups a surface's alerts. `run_id` is absent on a message with no run.
  - `apns-collapse-id` is `message-<id>`, so a repeated push replaces the earlier one.
- **A token APNs rejects** (`410`, `BadDeviceToken`, `Unregistered`, `DeviceTokenNotForTopic`) is dropped from the registry. The app registers it again on its next launch.
- **The daemon pushes only when it is configured with an APNs key**. Without one the device routes still work and nothing is sent.

## v3.12 additions: suggested replies

Agreed 2026-10-07 with the desktop. An agent can attach up to three one-tap replies to its answer, shown as buttons under the message. A tap posts an ordinary user message with the option's text as a reply to that answer, which wakes the agent like any message. The free-text field stays available, and nothing blocks: the agent's turn ends normally.

- **`suggested_replies`** is on every message: the REST pages, `message.created` frames (socket and SSE).

  ```json
  "suggested_replies": {"options": ["Do it", "Skip"], "open": true, "chosen": null}
  ```

  - It is `null` for a message without options. Only an `answer` can carry options; a `post` of the same run never does.
  - `options` are 1 to 3 strings, each 1 to 24 characters, in the agent's order.
  - `open` is `true` until the user writes anything on the surface after this message: a tap, typed text, voice, Telegram, any channel.
  - `chosen` is the option the user picked: the text of the first user message after this one, when that message replies to this one (`reply_to_message_id`) and its text equals an option. Otherwise `null`, also when the replies were closed by a typed message.
  - `open` and `chosen` are computed when the message is read, so a page fetched later shows the current state.
- **`POST /messages/{id}/reply`** `{"option": "Do it", "client_message_id": "<uuid>"}` taps an option.
  - `202 {"message_id", "input_id", "agent_id"}`, the same answer as `POST /surfaces/{id}/messages`. The posted message is a `user` message on the answer's surface with `text` = the option, `reply_to_message_id` = the answer, `channel` = `desktop`, addressed to the answer's author.
  - `client_message_id` makes the tap idempotent like a post. A retried tap with the same id answers with the first post's ids even though the replies are closed by then; an id already used on another surface is `409`.
  - `404`: unknown message.
  - `400`: the message has no suggested replies, `option` is not one of them, or `client_message_id` is not a UUID.
  - `409`: the replies are already closed. Two taps on one answer with different `client_message_id`s are served one after the other, so only the first is posted and the second gets `409`.
- **`reply_to_message_id` on a user message** is now set for a tapped reply (it was always `null` on user messages before).
- **Live closing needs no new frame.** A client receiving `message.created` for a `user` message on a surface closes the open replies of every earlier message on that surface. When that message replies to one of them and its text equals an option, the client marks that option chosen.
- An answer whose run ended `[SILENT]`, interrupted or with no visible answer has no message, so its options are dropped. A scheduled run's answer carries the options its run set. A run that restarts its answer (a crash retry) drops the options its earlier attempt set.
- Telegram shows no buttons; the answer is posted there as before.
- Fixtures: `testdata/v3/messages_page.json` (one answer with open options) and every message fixture gains `"suggested_replies": null`.
