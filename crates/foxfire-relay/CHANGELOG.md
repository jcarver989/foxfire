# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.2](https://github.com/jcarver989/foxfire/compare/foxfire-relay-v0.1.1...foxfire-relay-v0.1.2) - 2026-11-24

### Added

- *(relay)* serve chat channels

## [0.1.1](https://github.com/jcarver989/foxfire/compare/foxfire-relay-v0.1.0...foxfire-relay-v0.1.1) - 2026-11-24

### Fixed

- *(relay)* share one live session between every client

### Other

- *(relay)* explain the live session
- *(core)* separate the session model from native I/O

## [0.1.0](https://github.com/jcarver989/foxfire/releases/tag/foxfire-relay-v0.1.0) - 2026-11-19

### Added

- *(relay)* add the command-line entry point and usage docs
- *(relay)* serve workspace status and review over ACP
- *(relay)* fan out agent requests and keep the first answer
- *(relay)* share ACP sessions over WebSockets

### Other

- *(relay)* satisfy rustfmt and Clippy
- *(release)* publish binaries to GitHub Releases with dist
- *(relay)* shorten the README
