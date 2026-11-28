# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.4](https://github.com/jcarver989/foxfire/compare/foxfire-core-v0.1.3...foxfire-core-v0.1.4) - 2026-11-28

### Added

- *(core)* load artifact content when a renderer asks for it
- *(acp)* announce artifacts and let clients fetch their content
- *(core)* run ACP sessions on native and browser hosts
- *(core)* take part in a session's chat channel

### Fixed

- *(chat)* keep background posts unread and preserve disconnected drafts

### Other

- *(acp)* let fake agents call the MCP tools a session was given
- *(core)* share LiveSession with other crates' tests

## [0.1.3](https://github.com/jcarver989/foxfire/compare/foxfire-core-v0.1.2...foxfire-core-v0.1.3) - 2026-11-24

### Added

- *(acp)* add the chat extension

## [0.1.2](https://github.com/jcarver989/foxfire/compare/foxfire-core-v0.1.1...foxfire-core-v0.1.2) - 2026-11-24

### Added

- *(core)* format reported session costs for display

### Fixed

- *(core)* keep the replay when a new session rejoins the current one

### Other

- *(core)* separate the session model from native I/O

## [0.1.1](https://github.com/jcarver989/foxfire/compare/foxfire-core-v0.1.0...foxfire-core-v0.1.1) - 2026-11-19

### Added

- *(relay)* serve workspace status and review over ACP

### Fixed

- *(core)* dismiss questions the agent withdraws

### Other

- *(release)* publish binaries to GitHub Releases with dist
- *(testing)* share the ACP test runtime
- Merge pull request #1 from jcarver989/chore/prepare-crates-io-release
