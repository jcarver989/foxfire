# Foxfire

ACP clients for AI coding agents: a terminal UI, a native desktop app, an in-memory demo for browsers and iOS, and the shared client libraries behind them.

## Run

```sh
cargo run -p foxfire                              # defaults to the Aether ACP agent
cargo run -p foxfire -- --agent "my-agent"        # any stdio ACP agent
cargo run -p foxfire-desktop -- --agent "my-agent"
```

The terminal binary is `foxfire`; the desktop binary is `foxfire-desktop`. The default terminal agent command is `aether acp`, which requires Aether on `PATH`. Foxfire is an ACP client, not an agent server.

To measure desktop Cloud thread startup, run `RUST_LOG=warn,foxfire_startup=debug cargo run -p foxfire-desktop` and open a new thread. The logs split out launch, endpoint resolution, WebSocket upgrade, ACP initialization, session opening, and composer readiness. The endpoint timing includes any waits inside the configured command, such as a CLI readiness probe.

## Browser demo

Install the `wasm32-unknown-unknown` Rust target, `jq`, and `wasm-bindgen-cli` at the exact `wasm-bindgen` version in `Cargo.lock`. The syntax-highlighting dependencies also need a Clang that can target WebAssembly. If your default `clang` cannot, set `CC_wasm32_unknown_unknown` and `AR_wasm32_unknown_unknown` to the matching LLVM tools.

```sh
rustup target add wasm32-unknown-unknown
scripts/build-web
cargo install --locked miniserve  # once
miniserve --interfaces 127.0.0.1 --port 8000 --index index.html target/foxfire-web
```

Open <http://localhost:8000>, hover over **Demo project**, click **+**, and send a message. The browser uses the same Rouge sidebar and conversation components as desktop. Drag the sidebar divider to resize it, double-click the divider to reset its width, or click the sidebar icon to hide or reopen it. Send `/permission` to try the shared permission card: choose **Allow once** or **Reject**, or press **Escape** while the composer is focused to cancel the turn.

On a touch browser, open **Threads → Demo project → New thread**. Tap **Back** or swipe inward from the leading edge to return to Threads without closing the session or discarding a draft. To close it intentionally, use **End thread** in the conversation header and confirm. If you reload a conversation URL, return to Threads to start a fresh demo session.

Use the **Settings** gear in the sidebar or Threads header to choose **System**, **Light**, or **Dark** and set a theme for each appearance. These choices are saved in this browser profile for the site.

The replies are scripted, and threads live only in the page's memory: switching threads keeps their history, refreshing the page clears it. No agent, API server, account, or credentials are required. Add `?rouge_backend=gl` to test Rouge's WebGL2 renderer.

To supply a different backend, implement `foxfire_ui::backend::HostedBackend` and pass it to `foxfire_web::web_view(backend, platform, storage)`. The third argument is a Rouge `Storage` implementation for appearance preferences. Desktop supplies a session-opening function backed by its existing agent connector; its local and Cloud configuration are unchanged.

## iOS demo

On an Apple Silicon Mac with Xcode selected and a compatible iOS 17 or newer simulator runtime installed, run:

```sh
cargo rouge run ios
ROUGE_IOS_DEVICE="iPhone 17 Pro" cargo rouge run ios  # optionally choose by name or UDID
```

This builds Foxfire, opens Simulator, and installs and launches the demo. No Apple account or signing credentials are needed. Tap **New thread** under **Demo project** to start a conversation. Tap **Send** to submit; Return starts a new line. Use **Back** to return to Threads without ending the conversation. The demo uses the same scripted replies as the browser demo; it does not connect to a real agent.

This is an internal preview. Before distributing it, verify keyboard composition and Send visibility on a real iPhone and confirm that the Threads and conversation flows work with VoiceOver.

To build for a physical device instead, run `cargo rouge build ios`. The resulting `.rouge/out/ios/unsigned-app.zip` needs signing before it can be installed on a phone. Neither command publishes anything. For TestFlight, replace the placeholder App Store Connect ID in `.framer/src/main.rs` and follow [PUBLISHING.md](PUBLISHING.md).

## Android demo

Install Android Studio and create a virtual device in its Device Manager. Rouge finds the SDK in Android Studio's default location (`~/Android/Sdk`, or `~/Library/Android/sdk` on macOS); set `ANDROID_HOME` if yours is elsewhere. In Android Studio's SDK Manager, also install **Android SDK Command-line Tools** from the SDK Tools tab. Then install Gradle 9.9 or newer, which runs on JDK 17 through 27, and the rest of the toolchain:

```sh
sudo pacman -S gradle      # or brew install gradle
cargo rouge setup android  # SDK platform 36, build tools, NDK, Rust target and cargo-ndk
```

Then run:

```sh
cargo rouge run android
ROUGE_ANDROID_DEVICE=Medium_Phone_API_37.0 cargo rouge run android  # optionally choose an AVD or adb serial
```

This builds a debug APK for the device, boots the virtual device if none is running, and installs and launches the demo. A phone connected with USB debugging works too. No Google Play account or upload key is needed. The demo uses the same Threads and conversation screens and scripted replies as iOS.

This is also an internal preview. On a real device, verify keyboard composition, **Send** visibility, and the Threads and conversation flows with TalkBack. `cargo rouge build android` builds the release bundle for Google Play; see [PUBLISHING.md](PUBLISHING.md).

## Crates

- `foxfire-acp`: ACP transport, client utilities, conversation reduction, and optional Aether protocol extensions.
- `foxfire-core`: UI-independent session and runtime logic.
- `foxfire`: the terminal client binary (`cargo install foxfire`).
- `foxfire-tui`: embeddable terminal UI.
- `foxfire-desktop`: native desktop client.
- `foxfire-ui`: brand-neutral Rouge views and headless controllers for a host to build an agent app from, and the session backend contract.
- `foxfire-shell`: Foxfire's own shell, mark, words, and in-memory demo, shared by its apps.
- `foxfire-web`: browser host for the in-memory demo.
- `foxfire-mobile`: iOS and Android hosts for the in-memory demo.
- `foxfire-chat`: keeps the chat channels people share around an agent's sessions.
- `foxfire-relay`: serves a stdio ACP agent, and review of its workspace, to several Foxfire clients over one WebSocket each.
- `foxfire-docker`: runs desktop Cloud threads in isolated, warm-built containers on a local Docker engine. See [its setup guide](crates/foxfire-docker/README.md).

## Configuration

Set `FOXFIRE_HOME` to override the default `~/.foxfire` directory.

| Data | Default location |
| --- | --- |
| Terminal settings | `~/.foxfire/settings.json` |
| Terminal themes | `~/.foxfire/themes/` |
| Desktop state | `~/.foxfire/desktop/state.json` |
| Desktop agents | `~/.foxfire/desktop/acp.json` |
| Project agents | `<project>/.foxfire/acp.json` |
| Relay chat | `~/.foxfire/relay/chat/` |
| Terminal logs | `/tmp/foxfire-logs/foxfire.log.YYYY-MM-DD` |

This naming change does not automatically migrate configuration from the former clients. Copy existing settings, themes, desktop state, and agent catalogs to the locations above if you want to keep them. Aether's agent settings and sessions remain owned by Aether and should not be moved.

The `aether` modules, `_aether/*` methods, and `contextbridge/aether` metadata namespace describe Aether's ACP extensions, not Foxfire branding. They remain unchanged so Foxfire can interoperate with Aether; agents that implement only standard ACP are also supported.

See [terminal usage and embedding](crates/foxfire-tui/README.md) and [desktop activity motion](crates/foxfire-desktop/docs/activity-motion.md).
