# Design reference

The visual source of truth for this app. Committed so the repository is self-contained — building a
view does not require fetching anything external.

`mockup.html` is the designer's original, renderable locally (`open docs/design/mockup.html`). The
screenshots are renders of it, and are what you should look at first when building any view.

## Screenshots

| File | Shows |
|---|---|
| `screenshots/01-full-mockup.png` | everything at once — window chrome, all panels side by side |
| `screenshots/02-sidebar.png` | the sidebar: sections, channel rows, direct messages, the pinned "You" row and gear |
| `screenshots/03-feed-and-thread.png` | the message feed and the thread panel |
| `screenshots/04-direct-message.png` | a direct-message conversation |
| `screenshots/05-agents-and-settings.png` | the agent list and the per-agent settings panel |

The original window is 1440×920 with `overflow: hidden`, so the panels to the right of the feed are
clipped in the app frame. The renders were taken with that clipping removed, which is why they show
more than the window does at once.

## What to ignore in these images

The mockup is richer than v1. Everything below is visible in the screenshots and is **deliberately
out of scope** — do not build it, and do not treat its absence as a defect:

- the **Inbox** row and the **Agent crews** section in the sidebar
- every **structured message card**: the film picker with Pick buttons, the run log with its progress
  stages, the decision card with Do it / Not now, the task header with the Stop button, the archive
  entry table. v1 messages are plain text only
- the composer's **behaviour beyond typed text**: the Talk button, the Listening state with its
  waveform, the transcript block, and the `@` / paperclip / smiley / `Aa` icon row are all drawn as
  the mockup shows them and do nothing. v1 sends typed text only
- the **agent settings panel** (Role / Permissions / How it replies / Voice replies / Push
  notifications / Disconnect) — v1 shows agent cards, not an editor
- the **channel chips on agent cards** (`#downloads`, `#movie-night`, `night-shift`, `all channels`) — the
  domain has no agent-to-channel membership in v1, and some of those values are not channels at all
- the search field's behaviour — it is rendered as a static affordance, it searches nothing
- the reaction row

- the **"2 running" pill** on a channel row, the **plain activity dot** on `smart-home` and
  `downloads`, and the header's **"N tasks running"** — the domain has no per-channel agent or task
  data. The header shows a derived `N agents` count instead
- the **third status-dot colour**: agents are idle (green) or busy (amber), nothing else

The **Channel / Direct / Agents** switcher, the two-colour **status dots**, the **unread badge** and
the **bottom status bar** ARE in scope for this version: they are part of the window chrome the app
draws itself, which is the whole reason this version exists.

## What to take from them

Layout, proportion, spacing, type hierarchy, the chip-and-name row shape, the AGENT badge, the day
separator, the "N replies" affordance, the sidebar grouping and the pinned footer. Where the plan's
prose and a screenshot disagree about a detail the plan does not pin, follow the screenshot.
