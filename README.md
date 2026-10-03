# tuclaw-desktop

A native macOS client for the tuclaw agent system, written in Rust on
[GPUI](https://github.com/zed-industries/zed) — Zed's GPU-accelerated UI framework. It is the first
step toward replacing Telegram as the interface to a set of AI agents.

The app runs on the daemon's `/api/v3` through `tuclaw_core::v3`: the surfaces (Telegram topics) in the sidebar, one conversation grouped by day, sending with an optimistic row, live runs over the event socket, and the agents. Until the daemon ships the API it runs on the built-in mock daemon; set `TUCLAW_DAEMON_URL` (`http://host:9090`) and `TUCLAW_CLIENT_TOKEN` to talk to the real one. Threads and direct messages are not in v3.0 and are hidden until they are.

The window draws its own chrome. The titlebar is transparent, the traffic lights are positioned
inside the app's own top bar, and the feed is a rounded card floating on a warm
background. Nothing on screen is a system control.

Enter sends; Shift+Enter breaks the line. Clicking the composer focuses it, and it grows with its
text — there is no selection, no mouse caret placement and no height cap. Several controls are drawn
and inert on purpose: the search field and its `⌘K` hint, the sidebar toggle, the back and forward
arrows, the feed header's two trailing chips, and the composer's icon row and `Talk` chip. Times are
shown in UTC, not in the local zone.

## Layout

| Path | What it is |
|---|---|
| `core/` | `tuclaw-core`: the domain types the views render, day grouping, and `v3`, the client of the daemon's `/api/v3`. No `gpui` dependency, so it is testable without a window. |
| `app/` | `tuclaw-desktop`: the binary — state, views, the text input element, the theme. |
| `docs/design/` | The designer's mockup and five screenshots of it. Look here before touching a view. |
| `docs/contracts/` | `v3-client-contract.md`, the wire contract with the daemon, a copy of tuclaw's. |
| `docs/plans/completed/` | The implementation plans this repository was built from, archived complete. |

## The daemon client

`tuclaw_core::v3` speaks the daemon's `/api/v3` as `docs/contracts/v3-client-contract.md` defines it: REST with a bearer token, the event socket with replay and snapshots, a reducer that folds a run's frames into what a UI renders, and `MockTransport`, an in-process daemon the app runs against until the daemon ships the API. The UI is not wired to it yet; that comes as separate tasks.

### Real data before the daemon serves v3

Until `/api/v3` is live, the mock can start from a snapshot of the prod database instead of its built-in world. `mise run snapshot-world` (`script/snapshot-world.py`) reads `~/Library/Application Support/tuclaw-desktop/snapshot.db` and writes `world.json` next to it, shaped exactly like the contract's REST bodies; when that file exists the app starts on it (the toolbar says `snapshot`), otherwise on the built-in world. `TUCLAW_MOCK_WORLD` points at another file. Both files hold real chats: they stay outside the repository, are never committed or turned into fixtures, and are deleted once the daemon serves v3. Posting still plays the mock's canned run.

## Building

Prerequisites: macOS, [mise](https://mise.jdx.dev) — the Rust pin is enforced through it, see
Toolchain traps. Run `mise install` in the repository root to fetch the
version named in `mise.toml`.

Everything runs through mise tasks, defined in `mise.toml`. Read the Toolchain traps below before
running anything else.

```
mise run build       # cargo build --workspace --all-targets
mise run dev         # cargo run -p tuclaw-desktop
mise run test        # cargo test --workspace
mise run lint        # cargo clippy --workspace --all-targets -- -D warnings
mise run fmt         # cargo fmt --all
mise run fmt-check   # cargo fmt --all -- --check
mise run bundle      # target/release/bundle/Tuclaw.app, ad-hoc signed
mise run preview     # the bundle, opened (quits a running Tuclaw.app first)
mise run install     # the same, copied to /Applications
```

The four gates that must be green before any change lands: `mise run fmt-check`, `mise run lint`,
`mise run test`, `mise run build`. A clean build takes about a minute; incremental builds are a few
seconds.

## The app bundle

`mise run bundle` (`script/bundle-mac.fish`) builds the release binary and assembles `target/release/bundle/Tuclaw.app` by hand, without `cargo-bundle`: `app/resources/Info.plist` with the version from `Cargo.toml`, the commit count as the build number and the short commit as `TuclawCommit`; the icon compiled by `xcrun actool` from the Icon Composer source `app/resources/AppIcon.icon` (taken from the tuclaw-app iOS project) into `AppIcon.icns` plus `Assets.car`; an ad-hoc `codesign`. The bundle identifier is `dev.pkarpovich.tuclaw` and the minimum macOS is 14.0. `mise run preview` opens the fresh bundle after quitting a running one; `mise run install` copies it to `/Applications`. The same commands are Zed tasks in `.zed/tasks.json` (`task: spawn`, prefix `tuclaw:`).

`app/build.rs` bakes the short commit into the binary as `TUCLAW_COMMIT` (overridable from the environment), and the app menu's About Tuclaw shows `version (commit)`; Cmd+Q quits. A bare `mise run dev` binary has the menu too, but no icon and the executable's name in the menu bar.

## Toolchain traps

Four things about this project's toolchain are non-obvious, and each one has cost a build.

**Rust 1.98.1, pinned in `mise.toml`.** The floor is 1.97: GPUI uses `std::hint::cold_path`, and anything earlier fails to compile `gpui` with `E0658`. 1.98.1 (zed's own `rust-toolchain.toml`) compiles the whole dependency tree clean.

**Never run a bare `cargo`.** The pin does not reach it. On the author's machine `which cargo`
resolves to an older install placed ahead of mise's shims by the global mise config, and a
non-interactive session runs no directory hook, so `mise.toml` alone changes nothing. A mise task
runs inside the environment `mise.toml` declares, which is what makes the pin effective. Use
`mise run`; if you must call cargo directly, spell it `mise exec -- cargo ...`. Confirm the pin with:

```
mise exec -- rustc --version
```

**The entry point lives in `gpui_platform`, not `gpui`.** `gpui::Application` takes a platform, so the app starts with `gpui_platform::application().run(|cx: &mut App| { ... })`, opens its window with `cx.open_window(options, |_, cx| cx.new(...))` and calls `cx.activate(true)`.

**No Metal toolchain is needed.** GPUI Kit turns on `runtime_shaders` in the platform crate, so the build stitches `shaders.metal` into the binary as text and Metal compiles it when the app starts, instead of running Xcode's `metal` tool in a build script.

## Dependencies

GPUI comes from crates.io as the weekly snapshots GPUI Kit publishes and builds on, renamed back to the crate names the code uses:

```toml
gpui = { package = "gpui-pre", version = "=0.3.7" }
gpui_platform = { package = "gpui-pre-platform", version = "=0.3.7", features = ["font-kit"] }
gpui-kit = { version = "=0.7.0" }
```

`gpui-pre` is zed's own GPUI, published every week (0.3.7 is from 2026-09-28) together with the matching [GPUI Kit](https://gpui-kit.com) (`gpui-kit`, `gpui-base`, `gpui-component`): Markdown, inputs, lists and other components the app would otherwise write by hand. Both are pinned exactly and move together; a GPUI Kit release is a real task, not a version bump, because 0.x releases break APIs. The app moved off a git pin of zed's `main` on 2026-10-04: two `gpui` crates cannot share an app, and GPUI Kit only builds on its own snapshot.

Without the `font-kit` feature text lays out but renders no glyphs. `gpui` appears again under `[dev-dependencies]` with `features = ["test-support"]`, which is what `#[gpui::test]` needs.

## Data

Nothing is stored locally. On start the app connects to the event socket, then fetches the surfaces, the agents and the selected surface's newest page; everything after that arrives on the socket. A dropped socket reconnects with backoff and replays from the last event it applied. If the daemon URL is set without a token, or is not `http://`, the window opens on a failure view naming the URL and the error.

## Tests

Two tiers, split by what they need to run.

`tuclaw-core` uses plain `#[test]` — no window, no GPU. It covers domain invariants and day grouping. The v3 client is tested against golden JSON in `core/testdata/v3/`, against
`FakeDaemon` for the HTTP transport, and end to end over the mock in `core/tests/v3_mock.rs`.

`tuclaw-desktop` uses `#[gpui::test]`, which needs `gpui` with `test-support`. Every state is built over the mock daemon in stepped mode, so frames are played from the test thread. It covers state
transitions through `AppState` methods (the fresh start, sending, live runs, reconnect and gap), the input element through `simulate_input` and
`simulate_keystrokes`, click paths through `debug_selector` + `debug_bounds` + `simulate_click`, pure
view-model functions, and one draw test per view.

Both tiers run under `mise run test`.
