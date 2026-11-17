# Foxfire

A terminal client for AI coding agents that speak the [Agent Client Protocol](https://agentclientprotocol.com/) (ACP) v2.

```bash
cargo install foxfire
foxfire                      # launches the default agent ("aether acp")
foxfire --agent "my-agent"   # launches any ACP agent that speaks stdio
```

This crate is the `foxfire` binary. The terminal UI lives in [`foxfire-tui`](https://crates.io/crates/foxfire-tui), where you'll find keybindings, settings, and the API for embedding it. See the [repository](https://github.com/jcarver989/foxfire) for the other Foxfire crates.

## License

MIT
