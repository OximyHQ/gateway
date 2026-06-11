# Changelog

All notable changes to Oximy Gateway are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/), and the project adheres to
[Semantic Versioning](https://semver.org/) from 1.0 onward.

## [Unreleased]

### Added
- Initial repository scaffold: Apache-2.0 license, 11-crate Cargo workspace
  (`gateway-spine`, `gateway-llm`, `gateway-mcp`, `gateway-route`, `gateway-cache`,
  `gateway-telemetry`, `gateway-guard`, `gateway-config`, `gateway-control`,
  `gateway-dash`, and the `oximy-gateway` binary).
- Design doc and 65-agent competitive research under `docs/`.
- CI (build/test/clippy/fmt) and nightly workflow skeletons.
- `oximy-gateway` CLI skeleton (`up`, `version`, `help`).
