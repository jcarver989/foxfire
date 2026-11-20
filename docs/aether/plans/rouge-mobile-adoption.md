# Proposal: Adopt Rouge’s mobile design system in Foxfire

## Overview

**Start with iOS and put the mobile layout in `foxfire-ui`.** The native iOS binary and browser demo already call the same `hosted::demo_view`. Today the touch layout turns the desktop sidebar into a full-width list and hides it when a thread opens. Replace that toggle with a phone-style **Threads → Conversation** flow. Use the same layout on touch web; keep the pointer browser and native desktop on their existing shells. Foxfire has no Android app today, and this proposal does not add one.

A successful first slice lets someone open the demo, start or reopen a thread, send and cancel a prompt, answer a permission request, and return to Threads without losing an unsent draft or interrupting an active turn. Settings remains one tap away and saved themes still apply. The keyboard must not cover Send, and the new navigation must not cover the transcript or place controls in the system safe area. This is an interface change, not a new backend: iOS still uses scripted replies in the demo.

## Technical approach

| Surface | Adopt from Rouge | Keep in Foxfire |
| --- | --- | --- |
| Threads | `nav_bar`, `list_group`, `list_row` with a leading status icon and chevron, and a visible touch-sized New Thread button per project | Project grouping, launch and refresh behavior, and thread states. Swipe can reveal Stop; the conversation header also offers it so it is not gesture-only. |
| Conversation | `nav_stack`, the `nav_bar` back button and edge-swipe, and touch-sized Send/Cancel controls using Rouge tokens | `ChatPane`, transcript, rich content, prompt queue, permission cards, live session, and Foxfire’s branding. Back leaves the screen; it does not stop the thread. |
| Settings and feedback | The existing adaptive `modal` (Rouge renders it as a sheet on compact touch), grouped appearance rows, and `alert` for ending a hosted thread | The current theme catalog, preview/apply/revert behavior, storage, and distinction between cancelling a turn and ending its thread. |

Choose the hosted shell by Rouge’s `Presentation::is_touch()`, not an OS check or a breakpoint alone. Use the same stack on compact and regular touch in v1; constrain the list width on tablets rather than switch navigation patterns on rotation. Borrow Rouge’s color roles, typography, control sizes and motion; keep Foxfire’s active theme and visual identity. No bottom tabs, FAB, or global dock in v1: this flow has one primary destination, and the composer already owns the bottom of the conversation.

## Implementation steps

1. **Keep the local dependency baseline.** Foxfire’s workspace already patches all nine Rouge crates to `../rouge/crates/*`; `.framer/Cargo.toml` also points to the sibling `rouge-framer`. `cargo metadata --locked --format-version 1 --filter-platform aarch64-apple-ios-sim` resolves the UI crates to `~/code/rouge` at `ca2c5aff`. No manifest or lockfile change is needed for local development. Keep the workspace patches rather than switching just one direct dependency to a path: `clankerdiff-rouge` must share the same Rouge types. The seven existing browser/phone and settings tests pass against this checkout.
2. **Move touch navigation into `hosted/mobile.rs`.** Build `HostedApp` once, above both the mobile stack and the pointer shell. The mobile stack registers `/` and `/thread/:id`; render from the Rouge route, rather than synchronizing it with the pointer shell’s `selected`/sidebar flags. Use URL-safe, reversible encoding for the opaque `ThreadId` (the workspace already uses `base64`). Reuse the same launch/refresh/stop operations and the thread-entry cache in `HostedApp`. Open a new thread only after launch returns its ID; show the normal Starting/Failed state in the conversation route. If an ID is malformed or unavailable, show a recoverable state with Back, not an empty canvas or a new session. When the current thread is stopped, return to Threads. Remove the full-width sidebar workaround once the touch stack replaces it.
3. **Make the conversation fit the phone.** Put the Rouge nav bar in flow above the pane; keep the pane’s measured dock/transcript geometry. Use Rouge’s 44-point touch target for Send, Cancel, and critical options where Foxfire’s explicit sizing still opts out. Leave desktop shortcuts unchanged; on touch use Return for a newline and the visible Send button to submit. Keep launch/stop/setup logic out of the view and keep the existing per-thread scope alive across navigation. Check focus on Back so the keyboard does not stay open over Threads.
4. **Adapt Settings without replacing it.** Keep `SettingsModel` and existing preference storage. Use grouped appearance rows on touch and verify the theme picker still scrolls in the compact sheet; dismissing the sheet or backing out of a preview must restore the saved theme. Confirm **End thread** via Rouge `alert`, separately from the non-destructive **Cancel response** composer action. Don’t add new preferences in this slice.
5. **Make the keyboard a device gate.** First run stock Rouge on an iPhone simulator and a real device, with the hardware keyboard disconnected, and verify typing, composition, and resizing. Rouge applies native safe-area padding, but the current iOS and Android `WinitPlatform::keyboard_inset()` implementations fall back to zero. If native resizing doesn’t keep the composer visible, fix iOS keyboard occlusion reporting in Rouge, not with a hardcoded Foxfire offset. Feed the measured overlap to the existing keyboard-inset signal, excluding space already handled by resize or safe-area padding; test that the composer and end of the transcript stay visible together. Defer `keyboard_bar` until the host can report it reliably.
6. **Separate the local spike from a releasable build.** Before merging, use a published Rouge version containing the mobile APIs (and a matching packager if published), or temporarily replace the local paths with Git dependencies pinned to the same Rouge commit across the workspace patch set and `.framer`. Regenerate and verify both lockfiles from a clean checkout without `../rouge`. Keep the existing `just ios-sim` and package workflow; don’t change bundle ID or signing. Document how to try the new Threads flow on iOS and touch web.

## Testing plan

- **Component integration:** Extend `foxfire-web/tests/demo.rs` using `component_test()` and `MemoryBackend`. Verify launch, thread switching, explicit Back and edge-swipe, draft retention, prompt queue and permission/cancel behavior while switching, confirmed vs dismissed End Thread, and malformed/unknown routes. Assert taps reach visible controls only. Cover a narrow phone and regular touch width; resize with a thread open and verify navigation and draft stay intact. Extend `tests/settings.rs` for touch sheet scrolling, dismissed preview, and persisted light/dark selection. Run `cargo test --locked -p foxfire-web --test demo --test settings` and `cargo test --locked -p foxfire-ui`; run the existing desktop chat/sidebar regressions unchanged.
- **Browser smoke:** Build with `scripts/build-web`; try a coarse-pointer browser and a pointer browser. Rouge maps navigation to URL hashes, so check in-app Back, browser Back/Forward, and refresh on a thread hash after the in-memory backend has reset. A stale hash must show the recovery state. If app Back and browser history conflict, resolve that in Rouge before shipping the touch web flow; don’t maintain a second history stack in Foxfire.
- **iOS smoke and acceptance:** On Apple Silicon with Xcode, use `just ios-sim` and `cargo rouge build ios`. Check portrait/landscape, phone cutouts and home indicator, compact height with the soft keyboard open, multiline typing/paste/composition, dismissal and background/foreground. Repeat keyboard and navigation checks on a real iPhone. This Linux checkout can verify Cargo resolution and shared tests, not Xcode packaging or device behavior.

## Files to modify/create

| Path | Change | Action |
| --- | --- | --- |
| `crates/foxfire-ui/src/hosted.rs`, `crates/foxfire-ui/src/ui/shell.rs` | Keep one hosted model and pointer shell; remove the fullscreen sidebar workaround | Modify |
| `crates/foxfire-ui/src/hosted/mobile.rs` | Compose Threads and Conversation using Rouge routes and widgets | Add |
| `crates/foxfire-ui/src/pane.rs`, `crates/foxfire-ui/src/chat/composer/mod.rs`, `crates/foxfire-ui/src/ui/settings.rs` | Fit conversation controls and theme settings to touch without duplicating models | Modify |
| `crates/foxfire-web/tests/demo.rs`, `crates/foxfire-web/tests/settings.rs` | Expand shared UI coverage with the memory backend | Modify |
| `Cargo.toml`, `Cargo.lock`, `.framer/Cargo.toml`, `.framer/Cargo.lock` | Switch to a reproducible release source once the spike is validated | Modify before merge; no local change |
| `README.md` | Update mobile demo instructions and dependency setup | Modify |
| `~/code/rouge/crates/rouge/src/platform/ios.rs` | Provide keyboard overlap to the existing native host mechanism if resizing alone is insufficient | Modify if needed, upstream |

## Additional notes

- Keep this an **internal demo** until Rouge exposes screen-reader semantics and VoiceOver works for the full navigation and chat flow. The current custom-rendered widgets do not provide that accessibility tree; text labels, touch sizing, and keyboard shortcuts alone do not replace it.
- Android is a follow-up, not a parallel UI implementation. Once the shared screen is validated, spike a thin `foxfire-android` host and require usable IME/composition and system Back before committing to an Android release. Connecting mobile to a real hosted backend is separate work.
