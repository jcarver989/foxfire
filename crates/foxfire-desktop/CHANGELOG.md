# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.3](https://github.com/jcarver989/foxfire/compare/foxfire-desktop-v0.1.2...foxfire-desktop-v0.1.3) - 2026-11-28

### Added

- *(desktop)* give agents run on this machine the artifact tool
- *(ui)* fold transcript tool activity by turn
- *(ui)* manage hosted inventory independently of open workspaces
- *(ui)* reconnect dropped sessions without losing their transcript
- *(core)* run ACP sessions on native and browser hosts
- *(ui)* say why a session could not be opened
- *(desktop)* open the side room beside a thread
- *(ui)* add a side room for the people in a thread
- *(desktop)* introduce the user to the agents they connect to

### Other

- *(desktop)* lay out at the same interface size on every platform
- *(desktop)* start the footer reflow test from a one-line footer
- *(acp)* let fake agents call the MCP tools a session was given
- *(ui)* separate transcript fold rules from rendering
- *(shell)* share Foxfire’s workspace across desktop and hosted clients
- *(ui)* share the session driver
- *(desktop)* hold the paused clock while a connection is accepted
- *(acp)* describe a WebSocket with one Endpoint type
- *(ui)* size a side panel with PanelWidth

## [0.1.2](https://github.com/jcarver989/foxfire/compare/foxfire-desktop-v0.1.1...foxfire-desktop-v0.1.2) - 2026-11-24

### Other

- updated the following local packages: foxfire-acp, foxfire-core

## [0.1.1](https://github.com/jcarver989/foxfire/compare/foxfire-desktop-v0.1.0...foxfire-desktop-v0.1.1) - 2026-11-24

### Added

- *(ui)* shorten the cell meter to five cells
- *(ui)* show context usage and session cost in the composer
- *(branding)* add the jade desktop icon and browser favicon
- *(mobile)* share the hosted demo across web, iOS, and Android

### Fixed

- *(ui)* wrap the composer footer instead of squeezing its controls
- *(desktop)* bound remote WebSocket startup and trace connection stages

### Other

- *(ui)* extract shared Rouge shell and conversation views
- *(core)* separate the session model from native I/O

## [0.1.0](https://github.com/jcarver989/foxfire/releases/tag/foxfire-desktop-v0.1.0) - 2026-11-19

### Added

- *(desktop)* lay the app out on the Rouge 0.2 design system
- *(desktop)* offer past sessions from a new thread's composer

### Other

- *(desktop)* make the layout tests hold at Linux's interface size
- *(release)* publish binaries to GitHub Releases with dist
- Merge pull request #1 from jcarver989/chore/prepare-crates-io-release
- *(desktop)* [**breaking**] adopt Foxfire branding and configuration
- point the crates at the foxfire repository
- rename desktop to foxfire-desktop
- import desktop from aether with its history
