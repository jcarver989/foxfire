# Plan: LLMonade client apps on Foxfire

Status: draft for review. Nothing in this plan has been built.

## Summary

LLMonade gets desktop, web, iOS, and Android apps from one Rust codebase. They sign in with Clerk,
talk to the LLMonade API, and open a WebSocket to the agent in a Lambda MicroVM.

Foxfire already has most of the parts: an ACP client, a session state machine, and Rouge views for
transcripts, composing, approvals, diff review, and multiplayer chat. This plan separates those
parts from Foxfire's own look, publishes them as `foxfire-ui`, and builds LLMonade's apps on them.

The model is assistant-ui: a runtime, an adapter for your backend, and composable parts. The app
owns its shell and its brand.

## Decisions

1. **A session has its own ID.** Today a session has exactly one MicroVM. Later it will outlive one,
   so clients never see a MicroVM ID.
2. **Sessions are shared.** Every member of an organization can list, join, prompt, rename, suspend,
   and stop its sessions. There are no per-user permissions.
3. **Foxfire crates ship on crates.io.**
4. **Branding is very separate.** LLMonade does not use Foxfire's shell, mark, copy, or themes.
5. **The apps only talk to the LLMonade API** and to the agent connection the API hands them. How the
   API stores or derives anything is outside this plan.

## Goals

1. One LLMonade client codebase for desktop, web, iOS, and Android.
2. LLMonade writes no ACP, socket, or session state code.
3. LLMonade can look nothing like Foxfire without forking it.
4. Foxfire's own apps keep working and use the same parts.

## Non-goals

- API internals: persistence, caching, and how session metadata is kept.
- Transcripts of sessions whose MicroVM has ended.
- Private sessions, roles, or per-user permissions.
- Local agents in the LLMonade app. Foxfire desktop keeps that.
- React or other non-Rust consumers. Rouge renders to a canvas.
- Push notifications.

## Where things stand

| Piece | State |
| --- | --- |
| `HostedBackend` | Covers projects, list, launch, stop, and open (`foxfire-ui/src/backend.rs`). Lacks titles, creators, expiry, suspended and ended states, and typed errors. |
| `Thread`, `SessionCore` | Portable. `foxfire-ui` type-checks for `wasm32-unknown-unknown`. |
| Session driver | Desktop only (`foxfire-desktop/src/threads/driver.rs`). |
| `connection`, `runtime` | Compiled out on wasm. `connect_acp_client` uses `tokio::spawn`; `SessionRuntime` uses `JoinSet` and `spawn_blocking`. |
| Native transport | `foxfire-core` plus `foxfire-acp/websocket` type-checks for Android. iOS is unchecked: it needs a Mac. Neither has been run. |
| Files and artifact review | Desktop only, but their dependencies are already in `foxfire-ui`. |
| `hosted_view` | Inventory logic, navigation, and Foxfire branding in one closed view. |
| LLMonade Rust client | Inside `crates/cli`: device-flow login, refresh, organizations, MicroVMs. |
| LLMonade API | `OrganizationService` and `MicrovmService` over Connect. No CORS: a preflight returns 404. |
| Agent image | Pins `foxfire-relay` 0.1.0 on `main`, with an uncommitted bump to 0.1.1. Chat channels first ship in 0.1.2. |

## Architecture

```
                   LLMonade                                      Foxfire
 ┌──────────────────────────────────────────┐   ┌──────────────────────────────────┐
 │ llmonade-app                             │   │ foxfire-ui (published)           │
 │   shell, theme, mark, copy, sign-in  ────┼──>│   thread list, Thread, driver    │
 │   SessionBackend: HostedBackend          │   │   transcript, composer, approvals│
 │        │                                 │   │   files, review, people          │
 │        v                                 │   ├──────────────────────────────────┤
 │ llmonade-client                          │   │ foxfire-core, foxfire-acp        │
 │   OAuth, Connect RPC, credentials        │   │   runtime, ACP, WebSockets       │
 └────────┬─────────────────────────────────┘   └───────────────┬──────────────────┘
          │ HTTPS                                               │ WSS
          v                                                     v
   api.llmonade.ai ── url, headers, subprotocols ──>  MicroVM: foxfire-relay ─> aether
```

| Crate | Repo | Contents | Published |
| --- | --- | --- | --- |
| `foxfire-acp` | foxfire | ACP client, native and browser WebSockets | yes |
| `foxfire-core` | foxfire | Session core and portable runtime | yes |
| `foxfire-ui` | foxfire | Brand-neutral parts and headless controllers | yes (new) |
| `foxfire-shell` | foxfire | Foxfire's hosted shell, sidebar, settings, mark, kindling, demo | no (new) |
| `llmonade-client` | llmonade | API client, OAuth, credential storage | no (new) |
| `llmonade-app` | llmonade | The app and its platform entry points | no (new) |

`foxfire-desktop`, `foxfire-web`, and `foxfire-mobile` move to `foxfire-shell`. `llmonade-cli` moves
to `llmonade-client`.

## API contract

The apps need the following. `MicrovmService` stays as it is for the CLI.

```proto
service SessionService {
  rpc ListSessions(ListSessionsRequest) returns (ListSessionsResponse);
  rpc GetSession(GetSessionRequest) returns (GetSessionResponse);
  rpc CreateSession(CreateSessionRequest) returns (CreateSessionResponse);
  rpc UpdateSession(UpdateSessionRequest) returns (UpdateSessionResponse);
  rpc ConnectSession(ConnectSessionRequest) returns (ConnectSessionResponse);
  rpc SuspendSession(SuspendSessionRequest) returns (SuspendSessionResponse);
  rpc StopSession(StopSessionRequest) returns (StopSessionResponse);
}

service UserService {
  rpc GetCurrentUser(GetCurrentUserRequest) returns (GetCurrentUserResponse);
}

message Session {
  string id = 1;
  string title = 2;
  string image_id = 3;
  User created_by = 4;
  google.protobuf.Timestamp create_time = 5;
  google.protobuf.Timestamp expires_at = 6;
  SessionState state = 7;
  string failure_message = 8;
}

enum SessionState {
  SESSION_STATE_UNSPECIFIED = 0;
  SESSION_STATE_STARTING = 1;
  SESSION_STATE_READY = 2;
  SESSION_STATE_SUSPENDED = 3;
  SESSION_STATE_ENDED = 4;
  SESSION_STATE_FAILED = 5;
}

message CreateSessionRequest {
  string image_id = 1;
  string client_token = 2;
}

message ConnectSessionResponse {
  oneof result {
    SessionStarting starting = 1;
    SessionConnection connection = 2;
  }
}

message SessionConnection {
  string url = 1;
  map<string, string> headers = 2;
  repeated string subprotocols = 3;
  google.protobuf.Timestamp expires_at = 4;
}
```

`User` is `{ id, name }`. Images keep coming from `ListMicrovmImages`, which is the app's project
list.

Behavior the apps rely on:

- **IDs are opaque.** Session IDs appear in URLs and deep links and never change.
- **`CreateSession` is idempotent** for a `client_token`, so a retry starts nothing new.
- **`ConnectSession` resumes** a suspended session. It answers `starting` until the session can be
  connected to, so no call outlives the API Lambda's 30-second timeout. The client polls.
- **Connections are short-lived.** Clients call `ConnectSession` for every attempt and never store
  the result.
- **`headers` serve native clients and `subprotocols` serve browsers.** Both carry the same AWS
  credential. Keeping this in the API keeps AWS out of the apps.
- **Errors are distinct.** `UNAUTHENTICATED` signs the user out, `NOT_FOUND` removes the session, and
  `FAILED_PRECONDITION` means it ended.
- **Titles** come from the first prompt. The client sends it, and the API keeps the first one unless
  a user renames the session.
- **CORS** allows the app's origins to send `authorization`, `content-type`, `x-llmonade-org`, and
  the Connect protocol headers.

Clients need no ACP session ID. The relay advertises its live session, and
`Connection::open_remote` joins it or starts the first one.

## Foxfire work

### F1. Session driver in `foxfire-ui`

Move the generic half of `foxfire-desktop/src/threads/driver.rs` into `foxfire-ui`. It takes a
future that opens a session and returns a `SessionLink`. The desktop's launch event and review
override become inputs. Native tasks run on Tokio and browser tasks run on Rouge's executor.

Add one endpoint type to `foxfire-acp::websocket` and remove the desktop's own:

```rust
pub struct Endpoint {
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub subprotocols: Vec<String>,
}
```

Make `SessionEvent::ConnectFailed` carry a typed reason: retryable, unauthenticated, ended, or other.
A browser reports every failed handshake the same way, so the driver asks for a new endpoint on each
attempt and lets that answer decide.

Done when desktop uses the shared driver and its connector tests pass.

### F2. Headless thread list

Extract the inventory from `HostedApp`: refresh with epochs, polling while starting, launch, stop,
select, errors, and keeping panes alive. It exposes signals and actions and draws nothing.

Extend `HostedBackend` rather than adding a second trait:

- `HostedThread` gains `title`, `created_by`, `created_at`, and `expires_at`.
- `HostedState` gains `Suspended` and `Ended`.
- `BackendError` gains `Unauthenticated`.
- The trait gains `rename` and `suspend`.

Extend `MemoryBackend` to cover these. It is also LLMonade's UI test fake.

Done when the Foxfire demo runs on the list and tests cover every state without rendering.

### F3. Split the shell from the parts

Move `hosted.rs`, `hosted/`, `ui/shell.rs`, `ui/settings.rs`, `ui/kindling.rs`, `ui/foxfire.rs`, and
the demo to `foxfire-shell`. In what remains:

- **Activity mark:** nine call sites outside `ui/` draw Foxfire's mark. They draw one the host
  provides instead. `FoxfireState` becomes a neutral activity state.
- **Copy:** text such as "What do you want to build?" and "The agent could not start" comes from a
  struct the host provides.
- **Layout:** `ChatPane` hands out its parts and their wiring. The dock arrangement and the
  fresh-thread view move to `foxfire-shell`.
- **Themes:** saved preferences reject any theme that isn't built in. Accept a host-supplied set.
- **Storage keys and client name** come from the host.

`_foxfire/` methods and `foxfire://chat/messages` stay. They are wire protocol names.

Done when a test host with its own mark, copy, and theme renders no Foxfire text or mark.

### F4. Files, artifact review, and side room

Move `Files` and `ArtifactReview` into `foxfire-ui`. `Files` only needs `Thread`, and
`ArtifactReview` only needs `ChatPlatform::open`. Share the side-room panel sizing that
`hosted.rs` and `thread_view.rs` both copy today.

Diff review uses `ReviewSource::Agent`, which runs over the ACP socket. No second connection is
needed on any platform.

Done when the demo shells and desktop use the same three views.

### F5. Reconnect

Reconnecting is mostly behind buttons today. Retry with backoff after a drop, and immediately when
`window_focused()` becomes true. Stop on unauthenticated or ended. Keep the transcript on screen and
keep unsent text.

Rouge reports focus on the web and Android. Confirm it does on iOS.

Done when dropping a fake agent's socket and restoring it recovers without a click.

### F6. Portable runtime and browser WebSocket

Needed for web only.

1. Move `connection` and `runtime` out of `native`. Drive the ACP connection from the caller instead
   of `tokio::spawn`. Replace `JoinSet` with the `FuturesUnordered` already used for requests.
2. Keep Git, processes, local review, and file reading native, behind the host.
3. Add the browser socket to `foxfire-acp::websocket` under the same `connect`. ACP futures must be
   `Send` on every target and browser sockets are not, so a local task pumps the socket over
   channels. `clankerdiff-client` is the precedent: `gloo-net`, `MaybeSend`, and a platform `spawn`.
4. Pings are not available in browsers. The relay already pings every 20 seconds.

Done when `foxfire-web` holds a conversation with a local `foxfire-relay`, and wasm tests pass
against `fake_agent_server`, which already checks a negotiated subprotocol.

### F7. Publish

Publish `foxfire-acp` and `foxfire-core`, then `foxfire-ui` with a versioned workspace dependency and
a release-plz entry. The name `foxfire-ui` appeared free on crates.io on 2026-11-24.

Its API exposes Rouge types, so a breaking Rouge release is a breaking `foxfire-ui` release. Consumers
must match its Rouge version and `agent-client-protocol =5.0.0`.

## Rouge work

### R1. Themeable shape and type

`UI_FONT`, `CODE_FONT`, `Radius`, and `Material` are constants or fixed recipes. Colors are already
replaceable. Make fonts, radii, and materials part of an installed style with today's values as the
default. Keep the 4px spacing grid fixed.

Roughly 74 `Radius` and 45 `Material` uses in Rouge read these. Without this change LLMonade would
have its own colors and layout but Foxfire's fonts, corners, and glass.

### R2. Smaller web builds

The 2026-11-20 release build is 92 MB, or 13.3 MB gzipped. 72 MB of it is static data. `rouge-syntax`
depends on 62 grammars unconditionally, which is the likely cause but is not yet measured.

Feature-gate grammars, run `wasm-opt`, and serve Brotli. Set a size budget after measuring.

## LLMonade work

### L1. `llmonade-client`

Extract the generated code, OAuth, credentials, and `LlmonadeClient` from `crates/cli`. The CLI keeps
the daemon, harness, OTLP, and tray code. Existing CLI tests guard the move.

Make two things pluggable:

- **HTTP.** Native uses the current transport. Web implements `ClientTransport` on fetch. Its future
  must be `Send`, so a local task does the fetch and answers over a channel.
- **Credential storage.** An async trait with keychain, browser session, and in-memory versions.

Replace `SystemTime` and Tokio `sleep`, which do not work in browsers. `connectrpc` without its HTTP
client, `buffa`, and `oauth2` all type-check for wasm.

### L2. API

Add the contract above, `GetCurrentUser`, and CORS. Set an idle policy on launched MicroVMs: they have
none, so a forgotten one can run for the full four hours.

Build on the IAM changes in #42, #43, and #44.

### L3. Agent image

Move `foxfire-relay` to 0.1.2 or later for chat channels.

### L4. `llmonade-app`

One package: a library with the app, a binary for desktop and iOS, and a `cdylib` for web and Android.

```rust
pub fn app(config: AppConfig, credentials: Rc<dyn CredentialStore>) -> impl View {
    let account = Account::new(config, credentials);
    signed_in(account, move |session| {
        let threads = ThreadList::new(Rc::new(SessionBackend::new(session)));
        llmonade_shell(session, threads)
    })
}
```

| Screen | Desktop and wide web | Touch |
| --- | --- | --- |
| Sign in | Button, then the code to confirm | Same, in the system browser |
| Organization | Switcher in the sidebar | Switcher in the list header |
| Sessions | Sidebar grouped by image | List, then push |
| Conversation | Transcript, composer, approvals | Same |
| Files and review | Tabs | Tabs |
| People | Side panel | Sheet |
| Settings | Modal | Screen |

- Rows show title, creator, state, time left, and who is present in sessions that are open.
- **Stopping always confirms** and says that others may be using the session. Foxfire's sidebar stops
  without asking.
- Suspended sessions resume when opened. Ended sessions say that their transcript is unavailable.
- Switching organization closes every connection first.
- The participant in chat is the current user, not a typed display name.

### L5. Packaging and hosting

- **Desktop, iOS, and Android:** `RougePublish`, as Foxfire uses.
- **Web:** Rouge's web target publishes to GitHub Pages. Build with it, but host on LLMonade's AWS
  account through CDK. Set a content security policy that allows the API, Clerk, and MicroVM
  endpoints.

## Authentication

| Platform | Flow | Storage |
| --- | --- | --- |
| Desktop | Device flow (exists) | OS keychain (exists) |
| iOS, Android | Device flow in the system browser | Keychain, Keystore |
| Web | Authorization code with PKCE | Session storage, no refresh token |

- Device flow needs no deep links, which Rouge does not have.
- `keyring` 4.2's default `Entry` API does not support iOS or Android. The stores exist as
  `apple-native-keyring-store` (`protected`) and `android-native-keyring-store`, used through
  `keyring-core`.
- Clerk documents refresh tokens as never expiring. Web does not request `offline_access`, so none is
  ever in the browser.
- The PKCE callback arrives in the query string and Rouge routes in the hash, so they do not collide.
  Remove the code from the URL once it is used.
- Clerk's device, token, and revoke endpoints accepted a preflight from `http://localhost:3000`.
  Redirect URI rules are unverified.
- The CLI uses a Clerk development instance. Move to production before any app is distributed.

## Security

1. **Joining is full access.** The agent runs with `always-allow` and holds repository credentials.
   Sharing is intended, so membership and invite screens must say so.
2. **Chat identity is self-asserted.** Any member can post as another. Before customers use it, the
   API signs a short-lived participant token, `ConnectSession` returns it, and the relay verifies it.
   This needs a verifier option in `foxfire-relay`.
3. **Connection credentials** are never logged or stored. They travel in a WebSocket header, so
   check that access logs do not record them.

## Testing

- **Foxfire:** integration tests in `tests/`, extending `FakeAgent`, `MemoryBackend`, and the desktop
  connector fakes. Wasm tests run in a browser against `fake_agent_server`.
- **`llmonade-client`:** the CLI's `ClientFixtureBuilder` moves with the code. Both transports run the
  same tests.
- **API:** `FakeMicrovmClient` and `FakeClerkApiClient`, plus CDK tests for CORS and routes.
- **`llmonade-app`:** Rouge's `TestUi` over `MemoryBackend` for every session state, and one test
  against a real local relay.
- No timeouts in tests. Wait on conditions.

## Order

Work in the same phase can run in parallel.

| Phase | Work | Size | Needs | Outcome |
| --- | --- | --- | --- | --- |
| 0 | Land the Foxfire side-room changes and LLMonade #42–#44 | – | – | Stable base |
| 0 | Browser check, below | S | – | Web risk settled |
| 1 | L1 client | M | – | CLI unchanged on a shared client |
| 1 | L2 API | M | #42–#44 | Contract deployed |
| 1 | F1 driver, F2 thread list | M | phase 0 | LLMonade needs no ACP code |
| 1 | R1 shape and type | M | – | Full restyling possible |
| 2 | F3 split, F4 views, F5 reconnect | L | F1, F2 | Brand-neutral parts |
| 2 | F7 publish | S | F3 | `foxfire-ui` on crates.io |
| 3 | L3 image, L4 desktop | L | L1, L2, F7, R1 | Desktop app on live sessions |
| 4 | Mobile: secure storage, touch shell, packaging | M | phase 3 | TestFlight and Play internal |
| 5 | F6 runtime, R2 size, web auth, hosting | L | phase 3 | Web app |
| 6 | Verified participants | M | phase 3 | Ready for customers |

F6 does not depend on phases 2 to 4 and can start after F1 if web matters sooner than mobile.

L1 can start now. It touches neither Foxfire's uncommitted work nor the open LLMonade PRs.

**Browser check.** Open a socket from a web page to a live MicroVM. AWS
[documents](https://docs.aws.amazon.com/lambda/latest/dg/microvms-launching.md) passing the token and
port as subprotocols and says it removes them before forwarding. It does not say whether the handshake
response selects `lambda-microvms`. Browsers fail a handshake that selects none of the protocols they
offered, and the relay selects none today. If AWS does not answer for us, the relay must.

## Risks

| Risk | Response |
| --- | --- |
| Web download too large | R2. Ship desktop and mobile first. |
| Handshake needs a subprotocol from the relay | Phase 0 check |
| `foxfire-ui` breaks with each Rouge release | Upgrade Rouge and Foxfire together |
| Brand split is deeper than found | F3's test host shows what is left |
| iOS build or focus problems | Build on a Mac at the start of phase 4 |
| Phone drops sockets in the background | F5, and `ConnectSession` on every attempt |
| A session ends while people are using it | Show time left and warn before `expires_at` |

## Open questions

Each has a default, so none blocks phase 1.

1. Should the CLI create sessions, so that `llmonade vm launch` appears in the app? Default: not yet.
   The app lists sessions only.
2. How long may an idle session run before it is suspended? Default: 30 minutes.
3. Who can distribute the mobile apps, and under which bundle ID? Needed by phase 4.
4. Which domain serves the web app? Default: `app.llmonade.ai`. Needed for CORS and PKCE.
