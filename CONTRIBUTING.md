# Contributing

Thanks for helping improve `zolo-fake`.

## Setup

Use a current Zolo toolchain and make its `zolo` executable available on
`PATH`.

```sh
zolo check src/lib.zolo
cd examples
zolo install
zolo run basic.zolo
```

For the optional Rust engine:

```sh
cargo fmt --manifest-path native/Cargo.toml -- --check
cargo test --manifest-path native/Cargo.toml
```

## Pull requests

- Keep deterministic output stable unless the change is explicitly breaking.
- Add or update an executable example for public API changes.
- Keep the portable and native plan contracts aligned.
- Update `providers.toml` and `CHANGELOG.md` when the provider catalog changes.
- Never use generated values for security-sensitive examples.

By contributing, you agree that your contribution is licensed under the MIT
license used by this repository.
