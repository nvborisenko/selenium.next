# Selenium Next

Selenium Next is a WebDriver client with a shared Rust core and language-specific APIs.

- `core/`: Rust core, synchronous wrapper, and C ABI.
- `api/`: Rust, TypeScript, and .NET APIs, plus the shared Selenium IDL.
- `codegen/`: code-generation tooling.

## Build

From the repository root, build the Rust crates with:

```sh
cargo build --manifest-path core/selenium-core/Cargo.toml
cargo build --manifest-path core/selenium-sync/Cargo.toml
cargo build --manifest-path core/selenium-ffi/Cargo.toml
cargo build --manifest-path codegen/Cargo.toml
```
