# foxfire-core

The UI-agnostic core of the Foxfire [Agent Client Protocol](https://agentclientprotocol.com/) (ACP) clients. [`foxfire-tui`](https://crates.io/crates/foxfire-tui) and the Foxfire desktop app both build on it, so they share one implementation of session behaviour and only render it differently.

## Key Types

- **`connection::Connection`** / **`connection::Session`**: connect to an ACP agent and open a session
- **`session_core::SessionCore`**: a session's state (conversation, queued prompts, elicitations, git review). It reduces agent events and user commands into `Effect`s for the host to act on
- **`runtime::SessionRuntime`**: runs the commands a `SessionCore` dispatches against the agent and returns their results as `Input`
- **`composer`**: attachments, `@` file completion, prompt history, and prompt search
- **`elicitation`**: permission requests and form elicitations
- **`aether`**: Aether's ACP extensions, the only place clients read them from

## Feature Flags

| Feature | Description | Default |
|---------|-------------|---------|
| `testing` | In-memory fakes and builders (`TestWorkspace`, `SessionCoreConfigBuilder`) for host integration tests | No |

## License

MIT
