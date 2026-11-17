# foxfire-acp

Utilities for the [Agent Client Protocol](https://agentclientprotocol.com/) (ACP): Aether's protocol extensions and the client that UIs use to talk to agents. The crate depends on no Aether agent crates, so ACP clients can use it on their own; with `default-features = false` it is only the serde wire types, so Aether's agent crates depend on it without pulling in ACP.

## Table of Contents

<!-- START doctoc generated TOC please keep comment here to allow auto update -->
<!-- DON'T EDIT THIS SECTION, INSTEAD RE-RUN doctoc TO UPDATE -->

- [Key Types](#key-types)
- [Feature Flags](#feature-flags)
- [WebSocket transport](#websocket-transport)
- [WebAssembly](#webassembly)
- [License](#license)

<!-- END doctoc generated TOC please keep comment here to allow auto update -->

## Key Types

- **`aether`** -- The `_aether/*` requests and notifications, `AetherCapabilities`, and the `_meta` Aether reads and writes
- **`display_meta`**, **`mcp_status`**, **`artifact_review`**, **`ReasoningEffort`** -- Wire types carried in tool, MCP status, elicitation, and config option metadata
- **`McpNotification` / `McpRequest`** -- MCP message tunneling over ACP
- Agent subprocesses -- use upstream `agent_client_protocol::{AcpAgent, AcpAgentConfig, Stdio}`
- **`AcpClientHandle`** -- Initialized, cloneable client for typed ACP v2 requests
- **`AcpEvent`** -- Ordered notifications, including replay updates sent before the resume response; `AcpEvent::session_id` tells hosts which session an event concerns
- **`conversation::Conversation`** -- Host-independent reduction of one session's events (`apply_event`) and prompt lifecycle into ordered items (messages, thoughts, tool calls, notices) and turn state; shared by `wisp` and the browser client

## Feature Flags

| Feature | Description | Default |
|---------|-------------|---------|
| `protocol` | The `aether` extension messages and `content` helpers, built on `agent-client-protocol` | via `client` |
| `client` | ACP client and conversation model (for UIs connecting to agents); enables `protocol` | yes |
| `websocket` | Message-oriented WebSocket transport; enables `client` | no |
| `testing` | Public in-memory ACP test peers and transport helpers (enables `client`) | no |

## WebSocket transport

Enable the `websocket` feature to use `websocket::WebSocketTransport<S>` with an already-established WebSocket connection.

`examples/fake_agent_server.rs` serves the `testing` crate's fake agent over this transport, choosing a scenario by request path. Run it on `aether server`'s port with `cargo run -p foxfire-acp --features testing,websocket --example fake_agent_server`.


## License

MIT
