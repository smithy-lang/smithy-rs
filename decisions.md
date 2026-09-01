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
