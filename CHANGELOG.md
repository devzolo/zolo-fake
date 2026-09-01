# Changelog

All notable changes to this project are documented here. The project follows
[Semantic Versioning](https://semver.org/).

## [1.0.1] - 2026-09-01

### Fixed

- Consume the standalone `zolo-plugin-sdk` v0.1.1 release.
- Make Rust formatting reproducible outside the Zolo monorepo and update the
  public CI runtime.

## [1.0.0] - 2026-09-01

### Added

- Typed `@derive(Fake)` and `@fake(...)` field configuration.
- Deterministic portable engine for VM, native, LLVM, and WASM.
- Direct person, internet, location, company, lorem, and identifier providers.
- English, Brazilian Portuguese, and French locales.
- Optional columnar Rust engine backed by `fake-rs` 5.1.

[1.0.1]: https://github.com/devzolo/zolo-fake/compare/v1.0.0...v1.0.1
[1.0.0]: https://github.com/devzolo/zolo-fake/releases/tag/v1.0.0
