# framer:begin rust-monorepo
set shell := ["bash", "-euo", "pipefail", "-c"]

# List available recipes
default:
    @just --list

# Type-check every crate and target
check:
    cargo check --workspace --all-targets --all-features

# Run tests with nextest, then doctests, which nextest doesn't run
test profile="default":
    cargo nextest run --profile {{profile}} --workspace --all-features --no-tests pass
    cargo test --doc --workspace --all-features

# Run tests with nextest's CI profile, which writes JUnit results
test-ci: (test "ci")

# Run Clippy, failing on any rustc, Clippy or Cargo warning
lint:
    CARGO_BUILD_WARNINGS=deny cargo clippy --workspace --all-targets --all-features

# Format all code
fmt:
    cargo fmt --all

# Check formatting
fmt-check:
    cargo fmt --all -- --check

# Build docs, failing on any warning
doc-check:
    RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps --document-private-items

# Regenerate files from .framer/
framer:
    cargo run --quiet --manifest-path .framer/Cargo.toml

# Check generated files are up to date
framer-check:
    cargo run --quiet --locked --manifest-path .framer/Cargo.toml -- --check
# framer:end rust-monorepo

# Run the terminal UI against an ACP agent
tui agent="aether acp":
    cargo run -p foxfire -- -a '{{agent}}'

# Run the desktop app against an ACP agent
desktop agent="aether acp":
    cargo run -p foxfire-desktop -- --agent '{{agent}}'

# Run the desktop app in release mode against an ACP agent
desktop-release agent="aether acp":
    cargo run --release -p foxfire-desktop -- --agent '{{agent}}'
