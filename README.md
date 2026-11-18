# Foxfire

ACP clients for AI coding agents: a terminal UI, a native desktop app, and the shared client libraries behind them.

## Run

```sh
cargo run -p foxfire                              # defaults to the Aether ACP agent
cargo run -p foxfire -- --agent "my-agent"        # any stdio ACP agent
cargo run -p foxfire-desktop -- --agent "my-agent"
```

The terminal binary is `foxfire`; the desktop binary is `foxfire-desktop`. The default terminal agent command is `aether acp`, which requires Aether on `PATH`. Foxfire is an ACP client, not an agent server.

## Crates

- `foxfire-acp`: ACP transport, client utilities, conversation reduction, and optional Aether protocol extensions.
- `foxfire-core`: UI-independent session and runtime logic.
- `foxfire`: the terminal client binary (`cargo install foxfire`).
- `foxfire-tui`: embeddable terminal UI.
- `foxfire-desktop`: native desktop client.
- `foxfire-relay`: serves a stdio ACP agent, and review of its workspace, to several Foxfire clients over one WebSocket each.

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
