# Changelog

All notable changes to this project are documented here. The project follows
[Semantic Versioning](https://semver.org/).

## [Unreleased]

### Changed

- **Breaking.** The single `@fake(...)` field attribute is replaced by one
  decorator for each thing it said: `@provider(.X)`, `@range(min, max)`,
  `@chance(p)`, `@present(p)`, `@length(min, max)`, `@unique`, `@ignore`
  and `@infer(false)`. They come with `Fake` and `FakeNative`; nothing
  else is imported.
- **Breaking.** `@derive(Fake)` is written `deriving Fake` in the type's
  header, and the type option `infer` is given to the request:
  `deriving Fake(infer: false)`.
- `probability` meant the chance of `true` on a `bool` and the chance of a
  value on an optional field. They are now `@chance` and `@present`.
- A wrong argument, `min` above `max`, a probability outside `[0, 1]`,
  `@present` on a field that is not optional and `@length` on a field
  that is not a list are compile errors at the line that wrote them,
  instead of a panic from inside the generator.
- One of the decorators on a type that derives neither `Fake` nor
  `FakeNative` is a compile error with the request to add, instead of
  being ignored.
- Generated values are unchanged: the same seed gives the same rows.

Requires a Zolo compiler with declared decorators (`decorator`, `derive`
blocks and `deriving` in the header).

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
