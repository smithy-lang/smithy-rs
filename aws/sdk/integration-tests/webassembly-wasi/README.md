## webassembly-wasi test
This crate tests the `wasm32-wasi` target with the SDK (configured by `.cargo/config.toml`).

## Running the tests
1. Ensure prerequesites are installed:
   ```bash
   rustup target add wasm32-wasi
   cargo install cargo-wasi
   ```

2. If wasmtime has not been installed, `cargo wasi` will prompt you to install it.
3. `cargo wasi test`

## Testing this crate on wasm32-unknown-unknown
This crate can also be compiled on wasm32-unknown-unknown, however Rust >1.71 must be used:
```
cargo +stable check --target wasm32-unknown-unknown
```
