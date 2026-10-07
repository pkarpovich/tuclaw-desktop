# tuclaw for iPhone

Telegram is being retired as tuclaw's interface. Away from home, Pavel needs a phone app that does what the Mac app does. Build it tonight in this repository on GPUI, as a new iOS crate next to `core` and `app`. Reuse `tuclaw-core` (the v3 client and reducer) and the app's views where they fit a phone. GPUI Kit's mobile hosting landed in GPUI Kit 0.6.2, with `gpui-mobile` (Metal through wgpu); check its current state before you design around it. Write it the way you would want to maintain it for years. Do not break the Mac app or its four gates (`fmt-check`, `lint`, `test`, `build`).

## Rules

- **Backend**: the live daemon on bravo, `http://192.168.199.72:9090/api/v3`, no token, exactly what the Mac app uses. Every message you post wakes a real agent in Pavel's real topics. Post only in the `#phone-qa` topic. Keep posts short and few (a handful per feature you verify), never loop posting.
- **Scope = feature parity with the current Mac app** (`~/Projects/tuclaw-desktop`, `app/src/`, read-only for you). Whatever the desktop does, the phone does. Derive the list from the desktop code and the contract (`~/Projects/tuclaw/docs/plans/20261003-v3-client-contract.md`, v3.0 to v3.13); the known set is below. If something does not fit a phone, say how you adapted it rather than dropping it silently.
- **Design = the designer's iPhone mockup.** `Tuclaw iOS.dc.html` in the Claude Design project ba8496af-fa82-4ec1-83b0-d1cb492a9b3b (read it with the claude_design MCP: `read_file`, plus its `ios-frame.jsx` and `support.js`) is the visual spec: layout, navigation, palette, typography, components. Build what it shows. It was drawn in August, before the v3 API, so it has direct messages and channels that do not exist. Map them onto what does exist (surfaces and their groups) and keep the look. Features come from the Mac app, the look comes from the mockup. Where the mockup has no screen for a desktop feature, design it in the same language.
- **Branch**: work on the branch `iphone-app` in this repository. No git worktrees. Commit as you go, push the branch, and do not merge and do not touch `main`. Pavel decides in the morning.
- **Pairing.** You write all the code yourself. Your pair is the session mimi-transient-leaf, which knows the daemon, the v3 API, the contract, prod and the Mac app inside out. Whenever you are unsure about the API, a behaviour, the backend's data or a design call, ask it (SendMessage) instead of guessing, and keep working on something else while you wait. It may also send you review notes and screenshots requests during the night; treat them as part of the work. Code reviews come from mimi-fable; send it your diffs at each milestone.
- **Server changes**: none. If the phone needs something the v3 API lacks, write it down for the morning; do not change tuclaw.
- Work autonomously through the night. Pavel is asleep; a question that needs him goes into the report, not into a stop.

## Feature parity checklist (the Mac app today)

- Sidebar equivalent: surfaces with groups, order, archived hidden, unread and unread-replies badges, mark unread, live-run indicator.
- Conversation:
  - messages grouped by day, paging back through history (`before`);
  - Markdown rendering (rich text, code, links);
  - avatars of agents and the user.
- Live runs over the event stream:
  - the agent's text streaming in;
  - the steps (tools, status) in a collapsible run card;
  - the run summary on finished answers;
  - Stop (interrupt).

  The Mac app uses the WebSocket `/api/v3/events`; the per-surface SSE stream `/api/v3/surfaces/{id}/events` also exists. Pick one and say why.
- Composer:
  - text send with an optimistic row and idempotent `client_message_id`;
  - voice recording sent through `POST /surfaces/{id}/voice`, with the transcript shown;
  - @ mention picker sending `addressed_agent_id` (typing `@` lists the surface's agents, arrows plus Tab/Enter pick one);
  - Reply with a quote bar sending `reply_to_message_id`.
- Voice messages: play the original (`GET /api/v3/attachments/{id}`, ranges) and show the transcript.
- Suggested replies (v3.12): buttons under an answer, the tap endpoint, closed or chosen state.
- Read state: mark read when a thread is shown.
- Agents: list, profile, settings (model, description, wiring on a surface).
- User profile (name, description, avatar).
- Automations: the task list, a task's runs, pause, resume, cancel, and the `task.fired` marks in the conversation.
- Notifications: a local notification for a new agent message while the app is in the background, as far as the platform allows without APNs. APNs for iPhone is out of scope tonight: the daemon pushes to registered devices, but the iPhone bundle id and its profile are not set up.

## Morning deliverable

Put these under `docs/iphone/` on the branch:

1. `REPORT.md`, with these sections:
   - what works, checked against the parity list (done / partial / missing);
   - what you had to build yourself or bridge to platform APIs, and how much code that took;
   - the rough edges a user would hit;
   - an honest assessment of this codebase for an iPhone app: what makes it good or painful to keep developing here;
   - build and run instructions for the simulator.
2. Screenshots from the iPhone 17 Pro simulator (iOS 26):
   - the surface list;
   - a conversation with a streaming answer;
   - a run card expanded;
   - the composer with the @ picker open;
   - a voice message;
   - agent settings;
   - automations.
3. One screen recording of the acceptance scenario:
   1. open the app, pick `#phone-qa`;
   2. send a text and watch the answer stream;
   3. stop a run;
   4. send a voice message;
   5. reply to a message;
   6. tap a suggested reply if one appears.
