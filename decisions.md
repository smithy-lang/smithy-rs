# Decisions

## Prefix Routing Trait Shape

### Background

Phase 1b needs Smithy operation traits for route prefix admission. The plan names `OnlyIfPrefix("/v1")` and `AlsoWithPrefix("/v1")`, but this repo does not already define these traits and the references do not provide an upstream trait namespace.

### Alternatives

- Define one list-valued trait with a mode enum. This is compact but does not match the plan examples.
- Define two string-valued traits with the exact example names.
- Delay trait parsing and expose only runtime prefix APIs.

### Choice

Support two string-valued operation traits by shape name: `OnlyIfPrefix` and `AlsoWithPrefix`. Codegen recognizes these names regardless of namespace so internal model files can choose their namespace without requiring a shared prelude package in this branch. Multiple instances normalize into one runtime prefix policy.

### Risks/Tests

Namespace-insensitive matching could accept unrelated traits with the same name. The risk is limited to operation-level routing metadata and can be tightened once the final trait package is agreed. Tests should cover canonical-only default, only-prefix disabling canonical routes, also-prefix preserving canonical routes, and application across REST, RPC v2 CBOR, and AWS JSON routing.

## Schema Serde Protocol Dispatch

### Background

Phase 2 must make schema-enabled servers deserialize requests and serialize responses through schema-driven protocol logic while preserving the existing Tower route stack. Routing now inserts `SelectedProtocolContext`, but the operation upgrade layer is still statically typed as `Upgrade<Ser::Protocol, ...>`.

### Alternatives

- Add a dynamic upgrade layer that reads `SelectedProtocolContext` and dispatches to an object-safe protocol instance.
- Keep `Upgrade<P, ...>` static and generate protocol-generic `FromRequest<P, B>` / `IntoResponse<P>` impls that call a static `ServerProtocol` trait.
- Keep legacy generated protocol serde and only use selected context for tracing.

### Choice

Use the static `ServerProtocol` bridge first. It is conservative for this branch because it keeps Tower routing and operation services intact, preserves per-protocol runtime error/rejection types, and avoids introducing an object-safe protocol registry before the generated schema serde path has parity. `SelectedProtocolContext` remains available for future REST matched-route reuse and dynamic/private protocol work.

### Risks/Tests

The static bridge means multi-protocol services still instantiate operation services for the selected protocol marker in generated code; private protocols may need a registration layer later. Focused tests should prove schema-enabled generated crates compile and that AWS JSON, RPC v2 CBOR, and REST requests still serialize/deserialize correctly enough to preserve protocol test behavior.
