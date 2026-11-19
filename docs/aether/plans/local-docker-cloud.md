# Local Docker Cloud projects for Foxfire and Rouge

> **Superseded in part.** The goals, lifecycle and security rules below still hold. The mechanism was simplified: Git objects move over `docker exec` with Git's `ext::` transport instead of bundles and a bare cache, Docker itself supervises the agent instead of an in-container `serve`, and each project is configured by its `remotes.json` entry alone. See [the crate's README](../../../crates/foxfire-docker/README.md) for how it works now.

## Overview

Use the existing desktop Cloud sidebar to launch independent ACP workspaces for Foxfire and Rouge on Docker Desktop. Each launch starts at the exact SHA just fetched from `origin/main`. A reusable per-project image supplies a warm Cargo build; the container's writable layer supplies isolation. Neither local checkout is mounted, and the only change made to one is adding refs under `refs/foxfire/`.

Instances use the lifecycle the remotes contract and sidebar already support. **Suspend** stops the container and **Resume** starts it again; any exited container, whether suspended deliberately, crashed, or stopped by a Docker Desktop restart, appears as Suspended. **Stop instance** is the only finish signal: it exports the work as Git refs and then removes the container. App exit, disconnect, and the end of an agent turn are not finish signals.

**Done when:** Cloud `+` works for both repos; an intentionally stale warm image still launches at newly fetched main; two threads share their warm image but not their writes; Desktop shows ACP and the in-container diff; Suspend and Resume keep the workspace and its build artifacts; Stop lands commits, working-tree changes and non-ignored new files as a ref in the host checkout, then reclaims the writable layer. A failed Stop leaves the container listed and the work intact, and retrying Stop finishes the job.

## Technical approach

Use one Rust CLI, `foxfire-docker`, built for the host and for Linux ARM64. Its host commands invoke Git and Docker using argument arrays rather than shell fragments. Inside the container, `serve` supervises the relay and agent and answers `ready` and `quiesce` on a Unix control socket; `snapshot` and `bundle` produce the export. Use one parameterized Dockerfile. Reuse `CommandRemotes`, `AgentConnector` and `foxfire-relay`; do not add a Docker backend to the desktop app.

The remote config stays in `~/.config/clanker/remotes.json` and points at the installed helper by absolute path. A separate helper config, `~/.foxfire/docker/projects.json`, defines `foxfire` (`~/code/foxfire`, `origin`, `main`, `foxfire-dev:warm`) and `rouge` (`~/code/rouge`, `origin`, `main`, `rouge-dev:warm`). The checkout path supplies the upstream URL and the host's Git credentials, and it is where exports land. A Docker env file named in the config carries the model credentials. It is never mounted into a container. Foxfire uses its normal pinned Rouge dependency.

Host Git state lives in a helper-owned bare cache per project:

| Ref | Lifetime |
| --- | --- |
| `refs/foxfire/fetch/<uuid>` | One `launch` or `warm`; deleted once the SHA is pinned. |
| `refs/foxfire/base/<id>` | From `launch` until the instance's export lands. Keeps the bundle prerequisite available even if `main` is force-pushed. |
| `refs/foxfire/exports/<id>` | The durable export. A no-change export points at the base. |

The checkout receives `refs/foxfire/<project>/<id>` from the cache. `<id>` in ref names is the 12-character container ID prefix.

### Command contract

| Command | Behavior |
| --- | --- |
| `launch <project>` | Fetch and pin main, resolve the warm image, create, stage and start a labeled container, and print `{"id":"<container-id>"}`. |
| `list <project>` | Filter by helper and project labels and print `{"instances":[{"id":"…","state":"…"}]}`. Map Docker `created`/`restarting` to `starting`, `running` to `running`, `exited` to `suspended`, and `removing` to `stopping`. Report an instance with a live stop marker as `stopping`, which the desktop hides. `dead` containers are reported as `ended` and surfaced by `gc`. |
| `endpoint <project> <id>` | Verify ownership and print `{"url":"ws://127.0.0.1:<port>","headers":{}}` from Docker's published port. |
| `suspend <project> <id>` / `resume <project> <id>` | Verify ownership, then `docker stop` / `docker start`; `resume` waits for `ready`. |
| `stop <project> <id>` | Export the work to the cache and the checkout, then `docker rm`. Safe to repeat after any partial failure (see step 4). |
| `logs <project> <id>` / `shell <project> <id>` | Verify ownership, then `docker logs` / `docker exec -it … bash` with inherited stdio. |
| `warm <project>` | Build or update the shared image from fetched main, tagging it `:warm` only on success. Never called by `launch`. |
| `gc <project>` | Remove managed images that no container uses and base pins with no container, and report owned containers it cannot account for. Never removes a container that lacks an export ref. |
| `serve`, `ready`, `quiesce`, `snapshot`, `bundle --base <sha>` | In-container only; all but `serve` run via `docker exec`. See steps 3 and 4. |

In `remotes.json`, each lifecycle entry is an array such as `["~/.cargo/bin/foxfire-docker", "stop", "rouge", "$ID"]`, with `"$ID"` as the final argument where an instance is needed. `acp.websocket` calls `endpoint` with `"$ID"`. No `acp.cwd` or `diff` entry is needed because the relay serves both. Only protocol output goes to stdout. Logs go to stderr, and the last stderr line must be a self-contained message, because `CommandRemotes` reports it as the error.

## Implementation steps

1. **Pin upstream source.** Read the `origin` URL from the host checkout. Fetch the explicit `refs/heads/main` into the bare cache under a unique `refs/foxfire/fetch/<uuid>`, with `--no-write-fetch-head`, and record the full SHA. Resolve the `:warm` tag to an immutable image ID to avoid a race with a concurrent `warm`, and read its warmed-SHA label. If the warmed SHA is an ancestor of the new SHA and is present in the cache, build the incremental bundle `<warmed>..<new>`. Otherwise, for example after a force-push or with a fresh cache, fall back to a full bundle of the new ref. Skip the bundle when the SHAs match. If fetching fails or no warm image is installed, fail with an actionable message; never silently launch from stale source.
2. **Build the shared image.** Use a common Linux ARM64 tooling stage with Git, Rust, a non-root workspace user, platform packages from both repos' Linux CI, pinned Linux Aether, and `foxfire-relay` and `foxfire-docker` compiled from a pinned Foxfire revision. Use only tracked inputs or an explicit bundle in the build context, never either checkout wholesale. Build the per-project layer at `/workspace` from the bundle, preserving `.git`, and run `cargo check --locked --workspace --all-targets --all-features` with `CARGO_TARGET_DIR=/opt/foxfire-target`. Keep the UID, paths and flags the same at build time and run time. The warmed artifacts and required Cargo package data must be in the image itself, not only in a BuildKit cache mount. Label the image with the project and the warmed SHA. Prewarming linked test binaries is opt-in, after measuring image size.
3. **Launch, connect, suspend and resume.**
   - Run `docker create` with managed, project and base-SHA labels, `FOXFIRE_BASE=<sha>`, bounded log rotation, `--init`, and a randomly allocated loopback port (`127.0.0.1::7777`). Do not use `--rm` or a restart policy.
   - Pin `refs/foxfire/base/<id>`, `docker cp` the bundle in if there is one, and start the container.
   - On first start, `serve` imports the bundle, checks out the task branch `foxfire/task` at `FOXFIRE_BASE`, and verifies `HEAD` before starting `foxfire-relay -- aether acp` in its own process group (the image sets `FOXFIRE_RELAY_LISTEN=0.0.0.0:7777` and `FOXFIRE_RELAY_CWD=/workspace`). It updates the warmed workspace in place rather than cloning over it; the image's workspace is detached at the warmed commit, so `foxfire/task` is the only branch. `launch` then runs `ready`, which waits for this and reports a failed preparation.
   - On later starts (Resume), the task branch already exists, so `serve` starts the relay straight away. The workspace, its commits and its build artifacts persist across Suspend and Resume. The conversation survives only as far as the agent supports loading a session.
   - On SIGTERM (Suspend), `serve` terminates the relay and agent process group and exits. It does no export work, so Docker's stop grace period is never a correctness concern.
   - If preparation fails or the relay exits, `serve` stays up with its control socket, so Stop can still export. `ready` reports the problem.
   - If launch fails before `ready` succeeds, remove the container and the base pin. Once the agent may have written work, never remove the container except through `stop`.
   - Mount nothing from the host: no checkout, home directory, SSH agent or Docker socket. Inject only the selected model credential at run time, and document how the desktop-launched helper obtains it. Credentials must never be in image layers or exports. The relay is unauthenticated, so never publish its port on a non-loopback interface.
4. **Export through the host on Stop.** `stop` runs these steps in order. Each one is idempotent, and the container is removed last:
   1. Write a stop marker containing the helper's PID, so `list` reports `stopping` while that process lives. A crashed `stop` therefore never hides the instance.
   2. If the container has exited, `docker start` it.
   3. `docker exec quiesce`. This asks `serve`, over its control socket, to terminate the relay and agent process group and returns once the group has exited. The container keeps running.
   4. `docker exec snapshot`. If the working tree is dirty, this commits it on top of `HEAD` using a temporary index. The result preserves file contents, additions and deletions, but not the staging split. Ignored files are excluded, and the external target directory is never considered. `snapshot` points `refs/foxfire/export` at the result and prints its SHA; an unchanged tree reuses the previous snapshot, and an unprepared workspace reports the base.
   5. If the cache already has that commit (for example, it equals the base), point `refs/foxfire/exports/<id>` at it. Otherwise, stream `docker exec foxfire-docker bundle --base <base>`, which runs `git bundle create - <base>..refs/foxfire/export` (without a TTY, so the stream is binary-clean), to a temporary file. Then fetch it into the cache as `refs/foxfire/exports/<id>`; the fetch checks prerequisites and connectivity.
   6. From the checkout, run `git fetch --no-write-fetch-head <cache> refs/foxfire/exports/<id>:refs/foxfire/<project>/<id>`. This adds a ref and objects, and leaves the working tree, index and branches untouched.
   7. Run `docker rm`, then delete the base pin and the stop marker.

   On a retry where the container still exists, `stop` redoes the export and force-updates the helper-owned refs. If the container is gone, `stop` succeeds only if the export ref exists; otherwise it reports an unknown instance. Any failure before step 7 leaves the container in place. On the next poll it is listed again as running or suspended, so the work stays visible and Stop can be retried.

   v1 exports `HEAD` plus the working tree. Other local branches the agent creates are not exported. Refresh images periodically with `warm`. Reclaim space with `gc`, not `docker system prune`. There is no time-to-live deletion in v1.
5. **Install and document both profiles.** Provide example `projects.json` and `remotes.json` files to merge into existing config, not overwrite it. Document:
   - running `warm` manually and using the project row's `+`;
   - Suspend and Resume;
   - inspecting results with `git log refs/foxfire/<project>/<id>` and bringing them in with `git cherry-pick` or `git switch -c … refs/foxfire/<project>/<id>`;
   - pruning old export refs.

   Keep native macOS/iOS validation on the host. Also document the last-resort path for a suspended container that will not start: `docker cp <id>:/workspace` to a scratch directory, excluding the separate target directory, and inspect it without running Git commands that honor its copied config or hooks. The desktop's Stop removes the live thread before calling the backend. Because a failed Stop leaves the instance listed, it reappears and can be retried, so no desktop change is required for v1.

## Testing plan

- **Host helper, without Docker:**
  - Use real temporary Git repos for the checkout, the upstream and the bare cache. Test concurrent pins, dirty-checkout exclusion, upstream advancing, fetch failure, and incremental versus full-fallback bundles, including a force-pushed main.
  - Run `snapshot` and the export pipeline against a fake container runtime that maps a container to a local directory and runs `exec` as a local process. Round-trip commits, a deletion, a binary change, a new file and an ignored file. Cover a no-change export.
  - Cover each partial-failure resume: export landed but `rm` failed; checkout fetch failed; container gone with an export ref; container gone without one.
  - Test the state mapping and stop-marker liveness in `list`, argument and label validation, and JSON outputs. Invalid IDs must not affect other projects.
- **Docker integration (opt-in):**
  - Use a small Git fixture and the existing fake ACP v2 agent where practical. Warm at A and advance upstream to B. Launch two instances, verify that both are pinned at B and the host checkouts are untouched, connect to both over loopback, and check that edits in one cannot appear in the other.
  - Suspend and resume an instance and confirm that its edits and build artifacts survive.
  - Stop a running instance while the agent process is active, and stop a suspended one. In both cases confirm the checkout ref and that the container is removed.
  - Kill `stop` midway and confirm the instance is listed again and a retry completes. Run a clean Stop and a second Stop of the same instance.
- **Docker Desktop smoke test:** Warm both real projects and open two Cloud threads from the same image. Verify Git diffs in the app, run the exact warmed check, and Suspend, Resume and Stop from the sidebar. Inspect Cargo reuse and `docker ps --size` before startup, after an edit and build, and after removal. Copy-up of modified files from the warm target directory into each container's writable layer is the main disk-use risk. Confirm that there are no per-thread volumes and no warm-image rebuild at launch.

## Files to modify/create

| Path | Change | Status |
| --- | --- | --- |
| `crates/foxfire-docker/src/` | Host CLI (`fleet`), Git and Docker wrappers, in-container commands (`container`), and the `FakeDocker` engine behind the `testing` feature | Add |
| `crates/foxfire-docker/tests/` | Host tests against `FakeDocker`, which runs the real supervisor; Docker integration tests are still to do | Add |
| `crates/foxfire-docker/Dockerfile` | Tooling, tools and workspace stages; embedded in the binary and passed to `docker build` on stdin | Add |
| `crates/foxfire-docker/config/projects.example.json` and `remotes.example.json` | Foxfire and Rouge profiles and desktop wiring, checked by tests | Add |
| `crates/foxfire-docker/README.md` and root `README.md` | Setup, refresh, lifecycle, export refs and last-resort recovery | Add / modify |
| `crates/foxfire-desktop/`, `crates/foxfire-relay/` | Reuse existing APIs; no changes required for v1 | Unchanged |

## Additional notes

Start with one end-to-end vertical slice: Foxfire warm image → Cloud thread → Suspend and Resume → Stop → ref in the checkout → container removed. Measure disk use from writable-layer copy-up during that slice, before adding Rouge as configuration. Compare warm-hit rates and disk use before adding background refresh, compile-cache services, or first-class desktop settings.
