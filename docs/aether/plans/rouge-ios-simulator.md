# Proposal: `just ios-sim` for Rouge apps

## Overview

Generate `just ios-sim` for any application that enables `.ios(...)` in `RougePublish`. From the app repo, that one command should build the configured binary for the iOS simulator, open Simulator, and install and relaunch the app. It should work on repeated runs, require no Apple credentials, and leave `cargo rouge build ios` and TestFlight publishing unchanged.

```sh
just ios-sim                                      # pick a compatible simulator
ROUGE_IOS_DEVICE="iPhone 17 Pro" just ios-sim       # pick by name or UDID
```

## Technical Approach

Make the Just recipe a thin wrapper around a new local-only `cargo rouge run ios` command. The plugin already resolves the Cargo package and binary, app identifier, icon, minimum OS, device families, and orientations. Reuse those settings so app repos don't need their own shell script or `Info.plist`. Don't extract a device archive to make a simulator bundle: compile directly for `aarch64-apple-ios-sim` in debug mode, with the `iphonesimulator` SDK, and package into `.rouge/out/ios-sim/`. Use local ad-hoc signing (`codesign --sign -`), not the distribution signer, provisioning profiles, or entitlements. Scope the first pass to Apple Silicon, matching Rouge's existing gallery simulator recipe.

## Implementation Steps

1. **Generate the shortcut.** In `RougePublish::synthesize`, use `framer::just::{Justfile, Recipe}` to add a separate `rouge/ios-sim` block when iOS is enabled. Preserve the app's other recipes and settings. The generated recipe is:

   ```just
   # Build and launch the app in the iOS Simulator
   ios-sim:
       cargo rouge run ios
   ```

2. **Route the command.** Add `Action::Run` and pass the optional `ROUGE_IOS_DEVICE` selector into the packager (expose `--device` as the direct CLI equivalent). Reject `run` for non-iOS targets with a clear error. Keep the simulator path independent of `ROUGE_CHANNEL`, build numbers, and publishing secrets; neither add a workflow job nor alter the existing `build ios` path.
3. **Build and bundle.** Reuse the locked Cargo build helper, adding a debug variant without changing release defaults. Use Xcode's simulator SDK and the configured deployment target. Share iOS plist and asset-catalog creation between simulator and device builds, parameterized by SDK, so the simulator gets `iPhoneSimulator`/`iphonesimulator` metadata and the same icon and app identity. Ad-hoc sign the `.app` and place it in `.rouge/out/ios-sim/`. Keep regular device artifacts untouched.
4. **Select and launch.** Read `xcrun simctl list devices available --json` and filter to compatible iOS runtimes and configured device families. Resolve an explicit name or UDID first; if a name matches multiple compatible devices, tell the user to pass a UDID. With no selector, prefer an already-booted compatible device, otherwise take an available compatible device from the newest runtime (prefer iPhone when both families are supported; use a stable tie-break). If none are available, explain how to install a runtime or choose a compatible device. Use `simctl bootstatus <UDID> -b` to boot and wait, `open -a Simulator` to show the UI, then `simctl install` and `simctl launch --terminate-running-process` using the configured bundle ID. Return the prompt after launch rather than attaching to the app's console.
5. **Try it downstream.** Regenerate Foxfire with its existing `just framer` recipe, confirm that `just --list` exposes `ios-sim`, and replace the manual simulator instructions with `just ios-sim`. No handwritten Foxfire recipe is needed.

## Testing Plan

- Extend Framer synthesis tests: `ios-sim` appears only when iOS is configured, coexists with existing Just recipes, disappears when iOS is removed, and does not appear in the publishing workflow. Check regenerated files with `--check`.
- Extend public packager integration tests using its existing command doubles: assert the simulator triple, debug build, simulator plist metadata, expected boot/install/relaunch sequence and app ID, and that device artifacts and publishing credentials are untouched. Cover an explicit UDID, ambiguous names, a booted compatible device, no compatible runtime, and an unsupported `run` target.
- On macOS, run `just ios-sim` from Foxfire with a shut-down simulator and again after editing the app. Confirm both launches work and the second uses the updated binary. Separately verify that `cargo rouge build ios` still produces `.rouge/out/ios/unsigned-app.zip`.

## Files to Modify/Create

| Path (relative to `~/code/rouge` unless noted) | Change | Status |
| --- | --- | --- |
| `crates/rouge-framer/src/lib.rs` | Generate the Just recipe and accept the device selector. | Modify |
| `crates/rouge-framer/src/packager.rs`, `src/platform.rs` (in the same crate) | Route the local run action and share the Cargo build machinery. | Modify |
| `crates/rouge-framer/src/platform/ios/mod.rs` | Reuse bundle generation for both SDKs. | Modify |
| `crates/rouge-framer/src/platform/ios/simulator.rs` | Select, boot, install, and launch the simulator app. | Add |
| `crates/rouge-framer/tests/{publish_test,packager_test}.rs` | Cover generation and the simulator launch path. | Modify |
| `crates/rouge-framer/README.md`, `crates/rouge-framer/src/platform/ios/guide.md` | Document prerequisites, commands, and the device override. | Modify |
| `~/code/foxfire/justfile`, `.framer/manifest.json`, `PUBLISHING.md`, `README.md` | Regenerate the plugin-owned files; document `just ios-sim`. | Modify |

## Additional Notes

The existing `just ios run` in Rouge is for gallery examples. Leave it alone: `just ios-sim` is for apps configured through the plugin. Xcode and an installed iOS simulator runtime are prerequisites; physical-device signing and TestFlight setup are not.
