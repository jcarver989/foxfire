# foxfire-relay

foxfire-relay runs an [ACP](https://agentclientprotocol.com/) agent over stdio and lets multiple Foxfire clients connect to it over WebSocket. All clients share the agent's sessions. The relay also answers Foxfire's diff and workspace status requests, so a client needs no other connection to the workspace.

```sh
foxfire-relay --listen 0.0.0.0:7777 --cwd /workspace -- my-agent acp
```

The agent must support ACP v2. The relay exits if the agent negotiates a different version.

The relay does not authenticate clients. Run it only on a network that restricts access, such as a sandbox behind an authenticated proxy.

## Connecting Foxfire

Set a remote's `acp.websocket` command to one that prints the relay's endpoint as JSON:

```json
{ "url": "wss://sandbox.example.com/", "headers": {} }
```

The remote does not need a `diff` channel or `acp.cwd`, because the relay provides both. While a dynamic remote starts, Foxfire retries refused connections.

## Configuration

| Flag | Environment | Default |
| --- | --- | --- |
| `--listen` | `FOXFIRE_RELAY_LISTEN` | `127.0.0.1:7777` |
| `--cwd` | `FOXFIRE_RELAY_CWD` | The current directory |

The relay logs to stderr at `info` level. Set `RUST_LOG` to change the level.

## License

MIT
