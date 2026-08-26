# tuclaw-client

## What it is

A native macOS SwiftUI client, first version, rebuilding an approved UI mockup as a real app: a
three-pane window (sidebar | message feed | contextual right panel) over Core Data, seeded from
fixtures, with **no networking at all**. It is step 1 of an incremental path whose end state is
replacing a Telegram-based interface to a set of AI agents. One user, one machine, no server, no
distribution yet — ad-hoc signing only.

Two modules. `TuclawCore` is a host-free SwiftPM package under Swift 6 complete concurrency checking,
depending only on Foundation, Observation and CoreData, with **no `import SwiftUI` anywhere** — it
holds the domain, the Core Data stack, the feed source and the observable window state, and its tests
run under `swift test` with no app host and no simulator. The `Tuclaw` app target is the side-effect
adapter: SwiftUI views and nothing else. A third target, `TuclawUITests`, drives the built app through
XCUIAutomation.

## What a real failure looks like here

The app shows the wrong thing, or loses what it stored, and no gate notices:

- a first launch on a clean store that renders an empty sidebar permanently, because seeding and the
  initial load raced and the store cached the empty result
- migration flags configured after `loadPersistentStores` rather than before, so every later launch
  silently runs default migration behaviour and the next schema change wipes the store — that
  guarantee is the entire reason Core Data is in v1 rather than deferred
- Core Data tests that pass because they quietly share one store or reload the model per test, so
  isolation is fictional and the suite goes flaky later under parallelism
- an `actor` where the plan calls for `@MainActor`, touching `viewContext` off the main thread: it
  compiles, passes every grep, keeps `swift test` green, and fails in the app under load

## Blast radius

One developer, one machine, recoverable by deleting the store and relaunching. No server, no other
users, nothing irreversible. The app is not distributed, so a bad state is a local annoyance rather
than a shipped defect.

## Reporting bar

Severity follows user-visible consequence: critical for data loss or a broken primary path, major for
wrong results or a broken secondary path, minor otherwise. Documentation inaccuracies are never
critical or major.

macOS 26 is the only platform and the only deployment target, so back-deployment concerns,
`#available` gating and non-Apple portability are not findings on their own.

## Deliberate conventions, not defects

- Comments and docs are kept short on purpose. Only non-obvious constraints, rejected alternatives, or
  why the obvious implementation fails. Narrating code or restating a fact owned elsewhere is a defect
  in the other direction.
- Test comments are rare and one line. No arrange/act/assert labels, no restating an assertion.
- Unit and integration tests use Swift Testing; UI automation stays on XCTest because
  `XCUIApplication` exists only there. Two frameworks in one repository is intentional.
- `TuclawCore` never imports SwiftUI. This is load-bearing, not stylistic — it is what keeps the tests
  host-free.
- `@unchecked Sendable` and `nonisolated(unsafe)` are forbidden outright, and the per-task gate greps
  for both.
- Everything the app target consumes from the package is `public`; Swift's internal default would let
  `@testable import` hide the problem until the app fails to build.
- SwiftLint runs strict with zero findings required: 200-column lines, 1000-line files, 800-line
  types, raised to 2000 for tests. Disabled and tuned rules are deliberate and carry their reason.
- The message model is a plain `text: String` with no enum wrapper. Custom message types are a stated
  non-goal for v1 and arrive later as a lightweight migration.
- Fixture data and the seeder are intentionally throwaway; they are deleted when a network
  `FeedSource` arrives.

## What the project keeps in sync

The accessibility identifier contract in the plan is a two-sided contract: a view sets an identifier,
a UI test queries the same literal string. A change that renames one side without the other is a real
finding, and so is a new interactive element that no test reaches.

`docs/design/screenshots/` is the visual source of truth for anything the UI renders — a view whose
layout contradicts them, or a plan task that describes the UI without pointing at them, is a finding.
