# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.4](https://github.com/jcarver989/foxfire/compare/foxfire-acp-v0.1.3...foxfire-acp-v0.1.4) - 2026-11-28

### Added

- *(acp)* announce artifacts and let clients fetch their content
- *(acp)* expose agent-reported tool kinds
- *(acp)* connect browser clients over WebSocket
- *(acp)* identify what people quote around a session

### Other

- *(acp)* let fake agents call the MCP tools a session was given
- *(acp)* describe a WebSocket with one Endpoint type
- *(acp)* serve a chat channel from the fake agent

## [0.1.3](https://github.com/jcarver989/foxfire/compare/foxfire-acp-v0.1.2...foxfire-acp-v0.1.3) - 2026-11-24

### Added

- *(acp)* add the chat extension

### Other

- *(acp)* record every extension message the fake agent receives

## [0.1.2](https://github.com/jcarver989/foxfire/compare/foxfire-acp-v0.1.1...foxfire-acp-v0.1.2) - 2026-11-24

### Fixed

- *(acp)* preserve session cost across partial usage updates

### Other

- *(acp)* let the fake agent hold session/close
- *(core)* separate the session model from native I/O

## [0.1.1](https://github.com/jcarver989/foxfire/compare/foxfire-acp-v0.1.0...foxfire-acp-v0.1.1) - 2026-11-19

### Fixed

- *(core)* dismiss questions the agent withdraws

### Other

- *(testing)* share the ACP test runtime
