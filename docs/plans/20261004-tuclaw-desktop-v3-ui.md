# tuclaw desktop: the app on the v3 client

## Overview

The app drops its local SQLite store and runs on `tuclaw_core::v3`: the in-process `MockTransport` by default, the real daemon when `TUCLAW_DAEMON_URL` and `TUCLAW_CLIENT_TOKEN` are set. Decided by Pavel on 2026-10-04: replace the local mode entirely (no switch), and hide what v3.0 cannot feed (threads, the Reply pill, the Direct segment) until step D brings the API.

Four steps, one commit each, gates green after each:

1. **Read**: sidebar from `GET /surfaces`, feed from `GET /surfaces/{id}/messages`, agents from `GET /agents`; the store, schema, fixtures and paths modules and `rusqlite` leave; threads and Direct leave the UI.
2. **Live**: one event socket per app, opened before the REST fetch (the contract's fresh start), focus on the selected surface, a `Run` per live run rendered in the feed (text steps, tool and task steps, the streaming segment), the answer replacing the run when its `message.created` arrives, reconnect with `Backoff` and `since`, refetch on `gap`.
3. **Send**: the composer posts with a `ClientMessageId`, an optimistic row reconciled by `client_message_id`, a failed post hands the text back to the composer; a Stop button on a live run calls interrupt.
4. **Agents and status**: the Agents view and the status bar from live agent state, the connection status in the toolbar (mock, connecting, live, reconnecting), the source picked from the environment.

## Design

- `model.rs` stays the views' vocabulary (`Channel`, `Agent`, `Message`, `Span`); `app/src/link.rs` maps v3 DTOs onto it. `Author` gains `System` for notices. Message text is one `Span::Text` (Markdown rendering is a later task).
- `AppState` owns the `Client`, the selected surface's messages, the live runs (`BTreeMap<RunId, Run>`), the connection status and the queued placeholders; every mutation stays a method ending in `cx.notify()`.
- The link loop is a foreground task on the `AppState` entity (`cx.spawn`), REST futures are awaited in it, reconnect delays use `cx.background_executor().timer(..)`.
- Tests build `AppState` over `MockTransport` with `Pace::Stepped`, drive frames with `step()`/`play_all()` and `run_until_parked()`; never `Realtime` or `HttpTransport` under `#[gpui::test]`.

## Progress

- [x] Step 1: read - with the link task, the socket, the run state and sending already in it (the commit ordering moved: threads and Direct left in the same commit, since a separate removal would have rewritten the same tests twice; the toolbar shows the link status already)
- [x] Step 2: live - `live.rs` draws a run card per live or queued run of the selected surface after its messages: author, state chip, thoughts, tool and task and status steps, the streaming segment with a cursor; the list follows the tail (`FollowMode::Tail`) and remeasures the run rows while they grow
- [ ] Step 3: send
- [ ] Step 4: agents and status
