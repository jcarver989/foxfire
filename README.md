# Foxfire

ACP clients for AI coding agents: a terminal UI, a native desktop app, an in-memory browser demo, and the shared client libraries behind them.

## Run

```sh
cargo run -p foxfire                              # defaults to the Aether ACP agent
cargo run -p foxfire -- --agent "my-agent"        # any stdio ACP agent
cargo run -p foxfire-desktop -- --agent "my-agent"
```

The terminal binary is `foxfire`; the desktop binary is `foxfire-desktop`. The default terminal agent command is `aether acp`, which requires Aether on `PATH`. Foxfire is an ACP client, not an agent server.

## Browser demo

Install the `wasm32-unknown-unknown` Rust target, `jq`, and `wasm-bindgen-cli` at the exact `wasm-bindgen` version in `Cargo.lock`. The syntax-highlighting dependencies also need a Clang that can target WebAssembly. If your default `clang` cannot, set `CC_wasm32_unknown_unknown` and `AR_wasm32_unknown_unknown` to the matching LLVM tools.

```sh
rustup target add wasm32-unknown-unknown
scripts/build-web
cargo install --locked miniserve  # once
miniserve --interfaces 127.0.0.1 --port 8000 --index index.html target/foxfire-web
```

Open <http://localhost:8000>, hover over **Demo project**, click **+**, and send a message. The browser uses the same Rouge sidebar and conversation components as desktop. Drag the sidebar divider to resize it, double-click the divider to reset its width, or click the sidebar icon to hide or reopen it. Send `/permission` to try the shared permission card: choose **Allow once** or **Reject**, or press **Escape** while the composer is focused to cancel the turn.

The replies are scripted, and threads live only in the page's memory: switching threads keeps their history, refreshing the page clears it. No agent, API server, account, or credentials are required. Add `?rouge_backend=gl` to test Rouge's WebGL2 renderer.

To supply a different backend, implement `foxfire_ui::backend::HostedBackend` and pass it to `foxfire_web::web_view`. Desktop supplies a session-opening function backed by its existing agent connector; its local and Cloud configuration are unchanged.

## Crates

- `foxfire-acp`: ACP transport, client utilities, conversation reduction, and optional Aether protocol extensions.
- `foxfire-core`: UI-independent session and runtime logic.
- `foxfire`: the terminal client binary (`cargo install foxfire`).
- `foxfire-tui`: embeddable terminal UI.
- `foxfire-desktop`: native desktop client.
- `foxfire-ui`: shared Rouge shell, conversation views, and session backend contract.
- `foxfire-web`: browser shell and in-memory demo.
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
| Terminal logs | `/tmp/foxfire-logs/foxfire.log.YYYY-MM-DD` |

This naming change does not automatically migrate configuration from the former clients. Copy existing settings, themes, desktop state, and agent catalogs to the locations above if you want to keep them. Aether's agent settings and sessions remain owned by Aether and should not be moved.

The `aether` modules, `_aether/*` methods, and `contextbridge/aether` metadata namespace describe Aether's ACP extensions, not Foxfire branding. They remain unchanged so Foxfire can interoperate with Aether; agents that implement only standard ACP are also supported.

See [terminal usage and embedding](crates/foxfire-tui/README.md) and [desktop activity motion](crates/foxfire-desktop/docs/activity-motion.md).
