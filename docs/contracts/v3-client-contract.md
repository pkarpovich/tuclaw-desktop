# v3 client contract (step C)

Frozen 2026-10-03 between the C1 (daemon, repo tuclaw) and C2 (desktop, this repo) authors; a verbatim copy of tuclaw's `docs/plans/20261003-v3-client-contract.md`. The two copies are kept identical. Decisions here need both authors' agreement; anything we cannot agree on goes to Pavel as an open question. Both plans cite this file as the single source; the C2 mock speaks exactly this contract.

Source: tuclaw's `docs/plans/20261002-v2-architecture-proposal.md` S4, cut down to step C (rollout row C: `/api/v3` + event socket + bearer auth, the desktop goes online, Pavel's acceptance scenario lands). Everything S4 lists that needs step D machinery (new channels and DMs, threads, cards, dialogs, attachments, read state) is out of v3.0 and listed at the end.

Acceptance scenario this contract must carry: a conversation in an existing Telegram topic, from the desktop, without Telegram limits - plain Markdown in and out, real token streaming, the run visible step by step (text segments, tool calls with input and result, background tasks), the final answer without the `<details>Thinking` fold. A desktop message is not echoed to Telegram; the agent's answer is mirrored there as today.

## Transport and auth

- Base: `http://<daemon>:9090/api/v3` (the existing `TUCLAW_HTTP_ADDR` server), event socket `ws://<daemon>:9090/api/v3/events`. LAN and Tailscale only.
- Every v3 request, REST and WebSocket upgrade, carries `Authorization: Bearer <token>`. The token is the daemon's `TUCLAW_CLIENT_TOKEN` (one static token in v1). Unset token = v3 is not mounted at all (404), so a deployment without it changes nothing. Wrong or missing header = `401`.
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
  "bindings": [{"channel": "telegram", "external_id": "-1003614621196:0", "mirror": "agent_only"}],
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
- `origin` is B2's `message.created` origin (what caused the message: `user`, `a2a`, `scheduled`, ...). `channel` is where a `user` message was typed: `telegram` or `desktop` (`null` for agent and system messages).
- `client_message_id` echoes the id a v3 client posted the message with (else `null`), so `message.created` can be matched to the client's optimistic row whether or not the `202` arrived first.
- `run_summary` is present on a message with a `run_id` and lets the client draw "6 steps, 1 tool, 13 s" without fetching the run.

### `POST /surfaces/{id}/messages`

```json
{"text": "Лисички появились в магазине...", "addressed_agent_id": null, "client_message_id": "8b0c...-uuid"}
```

- Routes exactly like a Telegram message on that surface: `addressed_agent_id` (a mention) wins, else sticky, else the lead. The user row is written with `channel` origin `desktop` and is NOT mirrored to Telegram (`mirror = agent_only`); the answer is.
- `client_message_id` (UUID chosen by the client, required) makes the post idempotent: a retry with the same id returns the first result instead of a second message, and the id is echoed on the message (above).
- `202 {"message_id": 9193, "input_id": 42, "agent_id": 1}`. `input_id` is `null` when the message was stored but its wake could not be queued (the daemon then posts an error notice on the surface, as it does for a Telegram message); a retry with the same `client_message_id` returns the same answer. The reply arrives on the event socket: `message.created` for the user row, then the run's events.
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

1. `hello` `{head, floor, server_time, capabilities: {events: [...], ops: [...]}}` - `head` the newest seq, `floor` the oldest still kept (30-day retention).
2. if `since < floor`: `gap {floor}` - the client refetches `GET /surfaces` and the message pages it shows, then continues from `head`; else every persisted event with `since < seq <= head`, in order.
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

### Ephemeral events (no `seq`, never replayed)

- `text.delta {text}` - streamed answer text, coalesced per client on a 50 ms tick; only for focused surfaces.
- `input.accepted {input_id, surface_id, agent_id}` - the daemon took a posted message; the client may show "queued" until `run.started`.

### Client frames

Same envelope as server frames, without `seq`: `{"v": 1, "type": "<type>", "payload": {...}}`. A malformed client frame closes the socket (status 1003).

- `focus` `{"v": 1, "type": "focus", "payload": {"surface_ids": [1, 3]}}` - which surfaces the client shows; ephemeral frames flow only for those. Persisted events always flow. Default after connect: none focused. For every live run on a surface that becomes focused (newly in the set), the server sends a `run.snapshot` right away, so a surface selected mid-run starts from a whole segment instead of mid-sentence.

## Out of v3.0 (step D or later)

Creating channels and DMs, threads (`thread_root_id`), read state and unread counts, cards and dialogs (`ask`), attachments and artifacts, message edit (`message.updated`), search, task and wiring management over v3 (v2 keeps serving the mini-app), agent create/update/delete, `thinking.delta` (the agent emits no thinking stream today).
