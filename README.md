# zolo-fake

[![CI](https://github.com/devzolo/zolo-fake/actions/workflows/ci.yml/badge.svg)](https://github.com/devzolo/zolo-fake/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/devzolo/zolo-fake)](https://github.com/devzolo/zolo-fake/releases)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

Deterministic, typed fake data for the Zolo programming language.
The default engine is written in Zolo and runs on VM, native, LLVM, and WASM.
An optional Rust plugin backed by [`fake-rs`](https://github.com/cksac/fake-rs)
adds a high-throughput batch path for large fixtures.

## Install from GitHub

```sh
zolo add fake --git https://github.com/devzolo/zolo-fake.git --rev v1.0.0
```

This writes the dependency to `zolo.toml`, installs it into Zolo's
content-addressed store, and records both the exact Git commit and the BLAKE3
content hash in `zolo.lock`.

Equivalent manifest entry:

```toml
[dependencies]
fake = { git = "https://github.com/devzolo/zolo-fake.git", rev = "v1.0.0" }
```

## Typed fixtures

```zolo
use fake::{Fake, Faker, Locale}

@derive(Fake)
struct User {
  @fake(provider: .UuidV4)
  id: str,
  @fake(provider: .FullName)
  name: str,
  @fake(provider: .SafeEmail, unique: true)
  email: str,
  @fake(provider: .Int, min: 18, max: 80)
  age: int,
  @fake(provider: .Chance, probability: 0.82)
  active: bool,
}

let faker = Faker.new(seed: 42, locale: Locale::PtBr)
let user = User::fake(faker)
let users = User::fake_many(faker, 10_000)
```

`@derive(Fake)` generates concrete constructors on your type. This keeps
completion, type checking, and diagnostics precise—there is no `Any`-based
generic factory.

Field inference recognizes common names such as `id`, `name`, `email`,
`city`, and `company`. Use `@fake(...)` when you want an explicit provider or
constraints. Supported options include `provider`, `min`, `max`, `min_len`,
`max_len`, `probability`, `optional`, `unique`, and `ignore`.

## Direct providers

```zolo
let faker = Faker.new(seed: 7, locale: Locale::PtBr)

print(faker.person().full_name())
print(faker.internet().safe_email())
print(faker.location().city())
print(faker.company().name())
print(faker.lorem().sentence(min_words: 8, max_words: 14))
print(faker.identifier().uuid_v4())
```

Locales in v1: English (`En`), Brazilian Portuguese (`PtBr`), and French
(`FrFr`). The provider catalog is documented in [`providers.toml`](providers.toml).

## Determinism contract

- No process-global random-number generator.
- Every value derives from `seed + row + field path + retry`.
- Adding or reordering another field does not shift an existing field's stream.
- Changing `chunk_size` does not change generated values.
- The portable engine is the cross-backend snapshot oracle.
- Fake data is for tests, demos, fixtures, and seeds—not secrets,
  cryptography, or anonymization of real data.

## Optional native engine

The portable package is the recommended default. For very large batches,
build the opt-in Rust plugin:

```sh
cargo build --release --manifest-path native/Cargo.toml
```

Point Zolo at the resulting dynamic library and import the separate native
surface:

```sh
ZOLO_PLUGIN_DIR=native/target/release zolo run examples/native.zolo
```

PowerShell:

```powershell
$env:ZOLO_PLUGIN_DIR = "native/target/release"
zolo run examples/native.zolo
```

```zolo
use fake::{Fake, FakeNative}
use fake::native::{NativeFaker}

@derive(Fake, FakeNative)
struct User { /* fields */ }

let faker = NativeFaker.new(seed: 42, locale: "pt-BR", chunk_size: 4096)
let users = User::fake_many_native(faker, 100_000)
```

The plugin uses `fake-rs` 5.1 and crosses the plugin ABI once per columnar
chunk, never once per field. The native engine is intentionally opt-in so a
WASM/browser build never tries to load a native library.

The native crate consumes the public
[`zolo-plugin-sdk`](https://github.com/devzolo/zolo-plugin-sdk) release tag.
`Cargo.lock` records the immutable commit behind that tag, so builds are
reproducible without cloning the private Zolo compiler repository.

## Development

```sh
zolo check src/lib.zolo
cd examples
zolo install
zolo run basic.zolo
```

```sh
cargo test --manifest-path native/Cargo.toml
```

Public CI validates the native Rust plugin on Linux and Windows. The portable
Zolo package and examples are validated with an installed Zolo toolchain before
each release; the compiler repository is not a dependency of this package.

See [CONTRIBUTING.md](CONTRIBUTING.md) for contribution guidelines and
[SECURITY.md](SECURITY.md) for reporting vulnerabilities.

## License

[MIT](LICENSE) © devzolo contributors.
