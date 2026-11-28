# foxfire-relay

foxfire-relay runs an [ACP](https://agentclientprotocol.com/) agent over stdio and lets multiple Foxfire clients connect to it over WebSocket. All clients share one live session. The relay also answers Foxfire's diff and workspace status requests, so a client needs no other connection to the workspace.

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

## The live session

The first session a client opens becomes the relay's live session, and it stays live after every client leaves.

- A client that connects later is told of it and joins it with its history. So does a client that asks for a new session, which means `/clear` rejoins the live session rather than starting over.
- A client may resume a different session, which then becomes the live one, only while no other client is using the current one.
- When the relay starts, it adopts the workspace's most recently updated session as the live one and resumes it once a client joins.
- If the agent no longer knows the live session, the client that tried to join gets the agent's error, and the next one to open a session creates a new one.

## Chat channels

Each session has a chat channel, where the people using it can talk without prompting the agent. Clients reach it with the `_foxfire/channel_*` messages in `foxfire_acp::chat`. The relay answers these itself, and never forwards a `_foxfire/` method to the agent.

- A client says who is using it when it initializes. The relay takes its word for it, so anyone who can connect can use any name.
- A channel also records who sent each prompt, under the ID of the message it became in the agent's transcript, and which channel messages went with it.
- Channels are kept in `chat` in the data directory, one file to a session, and survive a restart of the relay. Keep that directory outside the workspace, so that chat is not committed with the agent's work.
- Chat stays out of the agent's context only by convention: an agent with a shell on the same machine can read the files.
- Relays can share a data directory, but only one at a time can have clients in a session's channel.
- If the directory cannot be used, the relay logs a warning and serves everything but chat.

## Artifacts

The relay gives the agent a `submit_artifact` tool, so that it can show people something it made inline in the conversation. It does this with `foxfire_artifacts::ArtifactHost`, which the desktop app also puts in front of an agent it runs itself. The SDK attaches the tool to each new or resumed session. It uses MCP over ACP natively, or authenticated loopback HTTP for agents that support MCP 2026-07-28 over HTTP.

- The agent submits the location of an HTML page or a PNG, JPEG, GIF or WebP image. The relay copies it, so a later edit does not change what was shown. Submitting the file again after it changed makes the next version of the same artifact, which takes the place of the one before in the conversation. An artifact belongs to the session whose tool submitted it, and each session counts its own versions.
- Clients are told of each version with `_foxfire/artifact` in `foxfire_acp::artifact`, in a slot that does not reveal the file's path, and fetch its content with `_foxfire/artifact_fetch` while it is the newest version. The relay forwards fetches to the artifact host rather than answering them, so its history holds only the announcements. There is no limit on how many a session submits.
- Clients that join later are shown what was submitted before. When everyone has left and the agent replays the session, the newest version of each artifact follows its replay, so they are shown at the end, not where they were made.
- Copies are kept in a temporary directory, which is removed when the relay stops, so they are gone after a restart.
- An agent advertising neither supported transport is not given the tool.

## Configuration

| Flag | Environment | Default |
| --- | --- | --- |
| `--listen` | `FOXFIRE_RELAY_LISTEN` | `127.0.0.1:7777` |
| `--cwd` | `FOXFIRE_RELAY_CWD` | The current directory |
| `--data-dir` | `FOXFIRE_RELAY_DATA` | `relay` in `$FOXFIRE_HOME`, or in `~/.foxfire` |

The relay logs to stderr at `info` level. Set `RUST_LOG` to change the level.

## License

MIT
