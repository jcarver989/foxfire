# foxfire-relay

Serves one stdio [Agent Client Protocol](https://agentclientprotocol.com/) (ACP) agent to any number of Foxfire clients over WebSocket. Every client connected to it shares the agent's sessions. The relay also reviews the agent's workspace itself, so Foxfire's diff and file browsing, `@` file completion, LSP lookups and branch display all work over the same socket.

```sh
foxfire-relay --listen 0.0.0.0:7777 --cwd /workspace -- my-agent acp
```

The agent must speak ACP v2, as Foxfire does. The relay refuses to start an agent that picks an older version.

## Behaviour

- **Shared sessions.** Whatever the agent says about a session reaches every client watching it, including prompts other clients send, since v2 agents echo each prompt as a user message. A prompt sent mid-turn goes to the agent, which decides whether to steer the running turn with it or queue it.
- **Joining late.** The relay keeps each watched session's updates in memory. Every resume reaches the agent, but once a client already watches the session, the relay strips `replayFrom` and replays its own log instead, so the clients already watching never see the history twice.
- **Questions.** The agent's permission requests and elicitations go to every client watching the session, or, if nobody is, to the next client that joins it. A request about no session goes to every connected client. The first answer wins, and the relay withdraws the other clients' copies with `$/cancel_request`.
- **Closing.** Closing a session detaches the client. The relay closes the session at the agent only once no other client watches it. A client disconnecting never closes a session.
- **Workspace.** The relay answers `_aether/git_diff*`, `_aether/workspace_status` and `_aether/session_preview` itself, and advertises them in the `initialize` response along with its workspace, as `RemoteServerInfo`.
- **Exit.** When the agent exits, the relay closes every client's socket and exits with an error, leaving the restart to whatever supervises it.

The relay does not authenticate clients. Run it where the network already does, for example inside a sandbox reached only through an authenticated proxy.

## Connecting Foxfire

Point a remote's `acp.websocket` command at the relay. The command prints the endpoint as JSON:

```json
{ "url": "wss://sandbox.example.com/", "headers": {} }
```

The remote needs no `diff` channel or `acp.cwd`, since the relay reports both over ACP. Foxfire retries refused connections while a dynamic remote starts. The relay accepts connections from the moment it binds, but only answers them once the agent has initialized.

## Configuration

| Flag | Environment | Default |
| --- | --- | --- |
| `--listen` | `FOXFIRE_RELAY_LISTEN` | `127.0.0.1:7777` |
| `--cwd` | `FOXFIRE_RELAY_CWD` | The current directory |

Logs go to stderr. Set `RUST_LOG` to change their level, which defaults to `info`.

## License

MIT
