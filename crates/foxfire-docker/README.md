# foxfire-docker

Runs Foxfire Cloud threads in containers on your machine's Docker engine (Docker Desktop on macOS). Each thread gets its own container. It starts from a shared per-project image with a warm Cargo build, at the commit just fetched from upstream `main`. Your checkouts are never mounted. Stop exports the thread's work to a Git ref in your checkout and then removes the container.

## Setup

1. Install the helper:

   ```sh
   cargo install --locked --path crates/foxfire-docker
   ```

2. Put the agent's model credentials in a Docker env file, for example `~/.foxfire/agent.env` containing `ANTHROPIC_API_KEY=…`, and run `chmod 600` on it. Credentials are passed to each container when it is created. They never enter an image or an export.

3. Build each project's image from its checkout. The first build compiles Rust, Aether and the whole workspace, so it takes a while. Projects other than Foxfire name a Foxfire checkout to build `foxfire-relay` from:

   ```sh
   cd ~/code/foxfire && foxfire-docker warm foxfire
   cd ~/code/rouge && foxfire-docker warm rouge --foxfire ~/code/foxfire
   ```

4. Merge the entries from [`config/remotes.example.json`](config/remotes.example.json) into `~/.config/clanker/remotes.json`; don't overwrite the file. Each entry is the whole configuration for one project:
   - `name` is the project's name. It labels the project's containers and names its refs.
   - `dir` is the host checkout. It supplies the upstream URL and your Git credentials, and receives exports.
   - `PATH` matters, because apps launched from the Dock don't inherit your shell's `PATH`, and the helper needs `docker` and `git`.

The helper launches from `origin`'s `main` on the image `<project>-dev:warm`. Pass `--remote`, `--branch` or `--image` to change that.

## Use

- **Start:** click `+` on the project's row in the desktop Cloud sidebar. The new thread starts at upstream `main` as it is right now, even if the image was warmed earlier.
- **Suspend / Resume:** stop and start the container. Files, commits and build artifacts are kept. An exited container, whether from a crash or a Docker Desktop restart, shows as Suspended.
- **Stop instance:** this is the only way to finish a thread. Stop saves the agent's commits, plus one extra commit holding any uncommitted changes (ignored files are left out), as `refs/foxfire/<project>/<id>` in your checkout. Only then does it remove the container. If any step fails, the container is left in place with its agent stopped, so you can run Stop again.

Bring the work into your checkout:

```sh
git log refs/foxfire/rouge/0123456789ab
git switch -c my-branch refs/foxfire/rouge/0123456789ab   # or: git cherry-pick …
git update-ref -d refs/foxfire/rouge/0123456789ab         # prune once merged
```

The helper's commands also work from a terminal, in the project's checkout:

```sh
foxfire-docker <warm|launch|list> <project>
foxfire-docker <endpoint|stop> <project> <id>
```

Suspend, Resume, logs and a shell are plain `docker container stop`, `start`, `logs` and `exec`.

## Maintenance

- Run `warm` again whenever the image falls far behind upstream. Launches still start at fresh `main` without it, but they rebuild more.
- `warm` moves the image tag, so the previous image is left untagged. Remove the ones no container uses with `docker image prune --filter label=dev.foxfire.project`.
- Use `docker ps --size` to see each container's disk use.

## Recovery

If a container won't start, Stop can't export it. As a last resort, copy the workspace out and inspect it without running Git there, because its config and hooks came from the agent:

```sh
docker cp <id>:/workspace ./rescued-workspace
```

Then remove the container yourself with `docker rm <id>`.

## How it works

- **Git over `docker exec`:** the helper moves commits with Git's own `ext::` transport, which runs `git push` and `git fetch` against the container's workspace through `docker container exec`. Git works out what each side is missing, so nothing is mounted and nothing is copied by hand.
- **One ref per instance:** `launch` fetches upstream `main` into `refs/foxfire/<project>/<id>` in your checkout, starts the container and pushes that commit into it. It then checks it out on the branch `foxfire/task`. `stop` overwrites the same ref with the work.
- **Docker is the supervisor:** the container's entrypoint runs `foxfire-relay -- aether acp`, published on host loopback only. `stop` stops the container, which ends every process in it. It then restarts it with a marker file that tells the entrypoint to idle, so the workspace holds still while it is committed and fetched.

## Testing

The tests drive a real Docker engine with a small stand-in image, so they are opt-in:

```sh
cargo test -p foxfire-docker --features docker-tests
```

## Limitations

- Only the branch checked out at Stop is exported, plus the working tree. Other local branches the agent creates are not.
- A thread's chat channel survives Suspend and Resume, but Stop deletes it with the container.
- Resume restarts the agent. The conversation continues only if the agent can reload its session.
- A new thread's agent process starts a moment before its workspace moves to fresh `main`. Launch returns only after the move.
- Native macOS and iOS builds still need to run on the host.
