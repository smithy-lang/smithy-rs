# Implementation Plan - Put `http` 0.2.x behind a feature flag in `aws-smithy-runtime-api`

Fixes https://github.com/smithy-lang/smithy-rs/issues/4805

## Problem Statement
`http` 0.2.x has two unpatched vulnerabilities (AIKIDO-2026-10922, AIKIDO-2025-10839) with no 0.2.x fix available. `aws-smithy-runtime-api` pulls `http` 0.2.x in as a mandatory dependency because its internal data representations (`Headers`, `Uri`, `HttpError`, `EndpointPrefix`, `Extensions`) are built on `http_02x` types. The `http-02x` feature there is currently an empty marker that only gates conversion impls, so every default SDK build compiles the vulnerable crate. We will migrate the internal representations to `http` 1.x and make `http` 0.2.x a truly optional, opt-in dependency (default off), without breaking the default public API.

## Requirements
- Scope limited to `aws-smithy-runtime-api`.
- Migrate to http 1.x: `Headers` internal storage (`http_02x::HeaderMap`), `set_endpoint`, `TryFrom<String> for Uri`, `TryFrom<&str> for Uri`, the default `ParsedUri::H0` construction path, and `HttpError` source types.
- Keep all existing http 0.2.x conversions/API, but only compile them under an opt-in `http-02x` feature (default off).
- Also update the test modules that use `http_02x` (port to http_1x where not validating 0.2.x interop).
- Strictly non-breaking under current semver; ship as a minor version bump.

## Background / Key facts from code investigation
- `HeaderValue` (newtype in `src/http/headers.rs`) already stores an internal `Inner::{H0,H1}` enum with working `into_http1x()`/`from_http1x()` and a version-agnostic `AsRef<str>`, so `Headers` can store `http_1x::HeaderMap` transparently without changing the public `Headers` API.
- `ParsedUri` already has an `H1(http_1x::Uri)` variant and `from_http1x_uri`; only the default-construction sites point at `H0`.
- Conversions to/from `http_02x` are already gated by `#[cfg(feature = "http-02x")]`; the gap is the non-gated construction/storage code.
- `HttpError` (`src/http/error.rs`) imports `http_02x::header::{InvalidHeaderName, InvalidHeaderValue}`, `http_02x::uri::InvalidUri`, and uses `http_02x::Error` in `invalid_uri_parts`. `invalid_method` already uses `http_1x::method::InvalidMethod`.
- `EndpointPrefix::new` (`src/client/endpoint.rs`) uses `http_02x::uri::Authority`.
- `Extensions` (`src/http/extensions.rs`) stores both `extensions_02x` and `extensions_1x`.
- `merge_paths` in `request.rs` is typed on `http_02x::uri::PathAndQuery`.
- Changelog convention: add a Markdown-with-YAML-front-matter file under `.changelog/` per `.changelog/.example` (fields: `applies_to`, `authors`, `references`, `breaking`, `new_feature`, `bug_fix`).
- Test modules using http_02x: `src/http/request.rs` and `src/http/response.rs` (gated `#[cfg(all(test, feature = "http-02x", feature = "http-1x"))]`); `src/client/interceptors/context.rs` and `src/client/runtime_plugin.rs` (gated `#[cfg(all(test, feature = "test-util", feature = "http-02x"))]`).

## Task Breakdown

### Task 1: Establish the feature flag and dependency wiring (fail-first baseline).
In `rust-runtime/aws-smithy-runtime-api/Cargo.toml`, mark `http-02x` optional (`http-02x = { package = "http", version = "0.2.12", optional = true }`) and change the `http-02x` feature from an empty marker to `http-02x = ["dep:http-02x"]`. Leave source unchanged for now. This is the "make it fail" step to enumerate exactly which sites break without the feature.
- Test: Run `cargo build -p aws-smithy-runtime-api --no-default-features` and `cargo build -p aws-smithy-runtime-api --features http-02x` to enumerate breaking sites. Record the compiler error list as the migration checklist.
- Demo: Compiler error output listing every unguarded `http_02x` reference — the concrete work list feeding Tasks 2-6b.

### Task 2: Migrate `HttpError` source types to http 1.x.
In `src/http/error.rs`, replace `http_02x::header::{InvalidHeaderName, InvalidHeaderValue}`, `http_02x::uri::InvalidUri`, and `http_02x::Error` (used in `invalid_uri_parts`) with http_1x equivalents (`http_1x::header::InvalidHeaderName`, `http_1x::header::InvalidHeaderValue`, `http_1x::uri::InvalidUri`, `http_1x::Error`). These are boxed into `source`, so no public signature changes.
- Test: `cargo test -p aws-smithy-runtime-api --no-default-features` compiles `error.rs`; keep existing error `Display` tests green.
- Demo: `HttpError` compiles and functions with `http-02x` off; error messages unchanged.

### Task 3: Migrate `Headers` internal storage to `http_1x::HeaderMap`.
In `src/http/headers.rs`: change `Headers.headers` to `http_1x::HeaderMap<HeaderValue>`, `HeadersIter.inner` to `http_1x::header::Iter`, and the `header_name`/`header_value` helpers plus `HeaderValue: TryFrom<String>` to build via `http_1x::HeaderName`/`http_1x::HeaderValue`. Add a `repr_as_http1x_header_name` fast path to the sealed `AsHeaderComponent` trait for http_1x header-name inputs, and gate the existing `repr_as_http02x_header_name` + `impl AsHeaderComponent for http_02x::HeaderName/HeaderValue` behind `#[cfg(feature = "http-02x")]`. Gate `http0_headermap`, `from_http02x`, `into_http02x`, and `TryFrom<http_02x::HeaderMap> for Headers` behind `http-02x`. `HeaderValue::Inner` keeps both variants; the default construction path now produces `Inner::H1`. Keep the `HeaderValue` public API identical.
- Test: Run existing `headers.rs` unit tests (insert/append/redaction/proptest) under `--no-default-features`; add a test asserting a default-constructed `HeaderValue`/`Headers` round-trips through `try_into_http1x`. Under `--features http-02x`, gated `http_02x` header tests still pass.
- Demo: `Headers` stores/reads/iterates header data with `http-02x` off; header behavior (including Debug redaction) unchanged.

### Task 4: Migrate `Uri`/`Request` default paths to http 1.x.
In `src/http/request.rs`: change `Request::new` to construct `Uri::from_http1x_uri(http_1x::Uri::from_static("/"))`, `TryFrom<String>`/`TryFrom<&str> for Uri` to parse into `ParsedUri::H1`, `set_endpoint` to use `http_1x::Uri`/`http_1x::uri` builder, and `merge_paths` typed on `http_1x::uri::PathAndQuery`. Gate `from_http0x_uri`, `into_h0`, `impl From<http_02x::Uri> for Uri`, `TryInto<http_02x::Request>`, `try_into_http02x`, and `TryFrom<http_02x::Request>` behind `#[cfg(feature = "http-02x")]` (most already are). `ParsedUri::H1` and `from_http1x_uri` already exist.
- Test: Add a default-feature test exercising `Request::empty()`, `set_uri`, `set_endpoint`, and `try_into_http1x` with `http-02x` off. Under `--features http-02x`, existing round-trip tests pass (these move to Task 6b).
- Demo: A `Request` can be created, have its URI/endpoint set, and convert to `http_1x::Request` with `http-02x` off.

### Task 5: Migrate `EndpointPrefix` and gate `Extensions` 0.2.x half.
In `src/client/endpoint.rs`, change `use http_02x::uri::Authority` and `EndpointPrefix::new` validation to `http_1x::uri::Authority`. In `src/http/extensions.rs`, gate the `extensions_02x` field, its population in `insert`, the `From<http_02x::Extensions>` impl, and `TryFrom<Extensions> for http_02x::Extensions` behind `#[cfg(feature = "http-02x")]`; adjust the http-1x `TryFrom` length check so it is correct when the 0.2.x half is absent. The cross-version "cant copy extension" guard only applies when both are present.
- Test: `EndpointPrefix::new` validation tests pass under `--no-default-features`. Extensions insert/convert tests pass with feature off (1x only) and on (both, existing cross-convert guard tests).
- Demo: `EndpointPrefix` validates authorities and `Request`/`Response` extensions work with `http-02x` off.

### Task 6: Sweep remaining non-test `http_02x` references and green the default library build.
Resolve every remaining non-test compiler error from Task 1's checklist so the default `cargo build -p aws-smithy-runtime-api` (lib) compiles with zero `http_02x`. Keep `NonUtf8Header::new`'s existing `#[cfg(any(feature = "http-1x", feature = "http-02x"))]` gate valid.
- Test: `cargo build -p aws-smithy-runtime-api` (default, lib only) succeeds; `cargo tree -p aws-smithy-runtime-api -i http:0.2.12` on the default build reports the crate is absent.
- Demo: Default library build has no `http` 0.2.x compiled.

### Task 6b: Migrate/port the test modules that use `http_02x`.
Update the four test modules so their coverage runs under the default feature set:
- `src/http/request.rs` and `src/http/response.rs` test mods (currently `#[cfg(all(test, feature = "http-02x", feature = "http-1x"))]`): split into (a) a default-runnable test mod that builds requests/responses via `http_1x` and exercises `try_into_http1x`, header insert/append, URI mutations, `set_endpoint`, `try_clone`, and status handling; and (b) a mod that stays gated on both features containing only the genuine http 0.2 <-> 1.x cross-conversion round-trips (`check_roundtrip`, the `cant_cross_convert_with_extensions` cases).
- `src/client/interceptors/context.rs` test mod (currently gated `test-util` + `http-02x`): re-gate to `#[cfg(all(test, feature = "test-util", feature = "http-1x"))]` and port `http_02x::{Request,Response,Uri,HeaderValue}` setup and the `AUTHORIZATION`/`CONTENT_LENGTH`/`from_static` imports to `http_1x`, so `test_success_transitions`, `test_rewind_for_retry`, and `try_clone_clones_all_data` run by default.
- `src/client/runtime_plugin.rs` test mod: re-gate to `http-1x` and port `http_02x::HeaderValue` and the `http_02x::Response::builder()` connector stub to `http_1x`.
Prefer porting to `http_1x` (default-on) over leaving gated on `http-02x`. Preserve the intent of each test; only cross-version round-trip assertions require both features. Confirm `HttpRequest`/`HttpResponse` aliases and `TryFrom`/`try_into` resolve with http_1x builders.
- Test: `cargo test -p aws-smithy-runtime-api` (default) compiles and runs the ported tests; `cargo test -p aws-smithy-runtime-api --features http-02x` still runs retained cross-version round-trip tests; `--no-default-features` builds cleanly; `--all-features` passes.
- Demo: rewind/retry, context-transition, runtime-plugin, and header/URI/status tests all run and pass under default features (no `http-02x`), while http 0.2<->1.x round-trip tests still pass when `http-02x` is enabled.

### Task 7: Verify downstream consumers and finalize (version bump + changelog).
Confirm crates that request `aws-smithy-runtime-api/http-02x` (e.g. `aws-smithy-http-client`'s `hyper-014`/`legacy-test-util`, `aws-smithy-legacy-http`, `aws-smithy-legacy-http-server`, `aws-types`, `aws-inlineable`) still build, since they now correctly pull the feature that enables the optional dep. Bump `aws-smithy-runtime-api` minor version in `Cargo.toml`. Add a `.changelog/` entry (YAML front matter: `applies_to: ["client","aws-sdk-rust"]`, `new_feature: true`, `breaking: false`, `references: ["smithy-rs#4805"]`, `authors`) describing that `http` 0.2.x is now behind the opt-in `http-02x` feature and off by default. Build before committing; do not push.
- Test: Run `cargo test -p aws-smithy-runtime-api` under default, `--features http-02x`, `--no-default-features`, and `--all-features` as the final gate. Build direct runtime dependents. Changelog file validates against the `.example` format.
- Demo: Full runtime workspace builds with `http-02x` opt-in; default SDK dependency tree is free of `http` 0.2.x; changelog and version bump in place.

## Notes for execution
- This is a Brazil/cargo Rust workspace at /Volumes/workplace/smithy-rs. Build/test per-crate with cargo as shown.
- Do not push. Only commit when the user explicitly asks.
- Follow inclusive-language and conventional-commit conventions if committing.
