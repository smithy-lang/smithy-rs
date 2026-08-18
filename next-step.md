# Next step: server-only multi-protocol codegen without `ProtocolFunctions` changes

## Objective and non-negotiable constraints

Multi-protocol generation is a **server-only** feature. The final implementation should:

1. Make no client or client-test changes.
2. Avoid changing `codegen-core/.../ProtocolFunctions.kt`; the client side is unlikely to accept changes to this pervasive shared utility.
3. Minimize all other `codegen-core` changes. Prefer no core changes if a reliable server-only design exists.
4. Preserve single-protocol generated output and behavior.
5. Preserve existing Pokémon example operations and tests; multi-protocol support must not delete established event-streaming examples.
6. Generate protocol-specific serializers/deserializers without one protocol silently reusing another protocol's implementation.
7. Do not commit until the design is settled and the complete diff has been reviewed.

## Workspace and current state

Use this worktree:

```text
/local/home/fahadzub/multi-protocol-clean
```

Do not review or modify `~/multi-protocol` or `mproto-new` for this effort.

Current branch and base:

```text
branch: fahdazub/mproto-clean
base/HEAD: 3a96c7a96 (latest main when the worktree was created)
```

Current net diff:

```text
37 files changed, 2863 insertions, 408 deletions
```

No commit has been created. The previous review artifacts at `~/mproto-pr-review.diff` and `~/mproto-pr-files.txt` are stale and must not be used until the design is finalized.

The current working implementation compiles and its focused module test passes, but it is **architecturally rejected** because it changes `ProtocolFunctions.kt`.

Current validation that passed before this handoff:

```text
:codegen-core:compileKotlin
:codegen-server:compileKotlin
:codegen-server:compileTestKotlin
ProtocolSpecificModuleTest
ServerProtocolOrderTest
```

The current implementation also has a generic event-stream module scope in core. That design is still under review; do not assume it is acceptable merely because it is generic.

### Index/status caveat

Earlier, `git add -N` was used so new files would appear in `git diff`. As a result, these removed experimental files may appear as ` D` in `git status` even though they are not in `HEAD`:

```text
codegen-core/common-test-models/pokemon-cbor.smithy
codegen-core/common-test-models/pokemon-multi-protocol.smithy
```

Do not treat those as real mainline deletions. Clean up the intent-to-add index entries later with a non-destructive index reset after the design is settled.

## Why collisions exist

On main, `ProtocolFunctions` puts serializer/deserializer inline dependencies in one fixed module:

```kotlin
val serDeModule = RustModule.pubCrate("protocol_serde")
```

Function names are derived from Smithy shapes, not protocols. For the same shape, REST JSON and RPC v2 CBOR can both create a function such as:

```text
crate::protocol_serde::shape_validation_exception::ser_validation_exception_error
```

The bodies are protocol-specific even though the path is identical.

`InlineDependency.key()` is:

```kotlin
"${module.fullyQualifiedPath()}::$name"
```

`RustCrate.injectInlineDependencies()` deduplicates with `distinctBy { it.key() }`. If two protocols generate the same `(module, name)`, the later renderer is discarded. This can compile while using the wrong protocol implementation, so compilation alone is insufficient validation.

Event-stream marshallers/unmarshallers have the same class of problem because they traditionally use the fixed `event_stream_serde` module and shape-derived names.

## Facts established from source inspection

Relevant mainline implementation points:

- `RuntimeType.forInlineFun(name, module, renderer)` creates an `InlineDependency` immediately.
- The `InlineDependency` stores its destination `module` and lazy `renderer`.
- A `RustWriter` does not execute the inline renderer when the `RuntimeType` is formatted. It records the dependency.
- `RustCrate.finalize()` later calls `injectInlineDependencies()` and executes each dependency's renderer in `dep.module`.
- Surrounding code with `rustCrate.withModule(...)` does **not** redirect an already-created inline dependency.
- Inline renderers can create more inline dependencies when they execute, so dependency graphs may be discovered recursively during finalization.
- By the time finalization has deduplicated identical keys, a discarded renderer cannot be recovered by post-processing generated files.

Important source locations:

```text
codegen-core/src/main/kotlin/software/amazon/smithy/rust/codegen/core/smithy/RuntimeType.kt
codegen-core/src/main/kotlin/software/amazon/smithy/rust/codegen/core/rustlang/CargoDependency.kt
codegen-core/src/main/kotlin/software/amazon/smithy/rust/codegen/core/smithy/CodegenDelegator.kt
codegen-core/src/main/kotlin/software/amazon/smithy/rust/codegen/core/rustlang/RustWriter.kt
```

## Experiments already performed in untouched main

Temporary tests were added to `~/smithy-latest`, run, and then removed. `~/smithy-latest` was verified clean afterward.

The experiments established:

1. **Ambient writer modules do not redirect dependencies.**
   A `RuntimeType` created for `fixed_destination` still generated there even when referenced from a writer opened with `withModule(ambient_destination)`.

2. **Identical dependency keys discard the later renderer.**
   Two inline functions with the same module and name but bodies returning `1` and `2` generated only the first body.

3. **A simple inline dependency can be cloned into another module.**
   Constructing a new `RuntimeType.forInlineFun` with the original dependency's renderer and a new module redirected a simple dependency successfully.

4. **Simple cloning does not recursively redirect nested dependencies.**
   When the cloned outer renderer referenced another inline dependency, that nested dependency remained in its original module.

5. **A scoped core override works technically.**
   A ThreadLocal module override, with lazy renderers re-establishing the scope, correctly redirected nested protocol dependencies and restored state after exceptions. This is the current approach for `ProtocolFunctions`, but it is rejected because it changes that shared client/core utility.

These experiments mean that a server-only capture/remap design must handle both emitted Rust paths and recursively discovered inline dependencies.

## Current rejected implementation

The current worktree changes `ProtocolFunctions.kt` to add a generic scoped module override. `ServerCodegenVisitor` enters the scope before rendering each protocol. It works technically and focused tests pass, but it must be removed if a server-only alternative succeeds.

Do **not** immediately restore `ProtocolFunctions.kt` before building an alternative prototype; the current implementation is useful as a known-working behavioral reference. First prove the replacement, then restore the core file and compare generated output.

## Alternatives to investigate

### Option A: server-side temporary `RustWriter` capture and recursive dependency remapping

This is the first option to prototype because it could avoid all `ProtocolFunctions` changes while keeping one generated crate.

High-level approach:

1. For each selected server protocol, render its operation code into a temporary in-memory `RustWriter` rather than directly into the crate's writer.
2. Capture:
   - the emitted Rust text,
   - imports,
   - all recorded `SymbolDependencyContainer`s,
   - inline dependencies and their lazy renderers.
3. Rewrite legacy paths such as:

   ```text
   crate::protocol_serde::...
   crate::event_stream_serde::...
   ```

   to protocol-specific server modules.
4. Clone each `InlineDependency` with a protocol-specific destination module.
5. Wrap cloned renderers so that when they execute they render into another temporary writer, recursively remap newly discovered nested inline dependencies, rewrite paths, and then append the rewritten result to the real writer.
6. Add non-inline/Cargo dependencies unchanged.
7. Write the captured operation text to the real server operation writer.

Questions to answer with a focused prototype:

- Can `RustWriter.factory(debugMode)` create a temporary writer with the correct filename and namespace when `RustWriter`'s primary constructor and filename are private?
- Are `writer.dependencies` and `writer.addDependency(...)` sufficient to transfer all dependencies?
- Can `InlineDependency.name`, `module`, `renderer`, and `dependencies()` recreate a semantically equivalent dependency? Note that constructor extra dependencies are private, but `dependencies()` is public.
- Can imports be preserved by using `toString()`, or will dumping formatted text into another writer duplicate generated headers/import sections?
- Will textual path rewriting accidentally modify string literals, comments, or unrelated identifiers?
- How are inline child modules and `mod` declarations handled?
- Are safe-name counters and debug codegen comments stable after capture?
- Can the remapper detect and reject two same-key dependencies with unequal rendered bodies rather than silently choosing one?

Prototype success criteria:

- Implement entirely under `codegen-server/src/test` first.
- Start with two synthetic inline dependencies having the same legacy key and different bodies.
- Include a nested dependency created only when the outer renderer executes.
- Remap both protocol graphs to separate modules.
- Generate and compile a Rust crate.
- Then apply the prototype to a two-protocol server integration test and compare generated modules against the current known-working implementation.

Likely risk: high complexity and reliance on text rewriting. Do not productionize without proving nested dependencies, imports, and module declarations.

### Option B: per-protocol temporary `RustCrate` / `FileManifest`, then merge

Instead of capturing one writer, generate each protocol into an isolated temporary crate/file manifest so normal finalization executes all inline dependencies without cross-protocol deduplication.

Possible flow:

1. Generate protocol-independent models/builders/service scaffolding in the final crate.
2. For each protocol, create a temporary `FileManifest` and `RustCrate`.
3. Render only that protocol's operation ser/de into the temporary crate.
4. Finalize it so all lazy/nested inline dependencies are materialized.
5. Read generated Rust files from the temporary directory.
6. Rewrite module roots into protocol-specific modules and merge into the final crate.
7. Merge Cargo dependencies/features from all temporary crates.

Advantages:

- Uses existing recursive inline dependency finalization.
- No need to clone unevaluated nested dependency graphs manually.
- Protocol collisions cannot occur inside each one-protocol temporary crate.

Challenges:

- Finalization also writes `lib.rs`, `Cargo.toml`, module declarations, and possibly unrelated generated artifacts.
- Need a precise allowlist of protocol-owned files.
- Operation code references model/error/service types in the final crate.
- Paths still use `crate::protocol_serde`, so files and references require rewriting.
- Cargo dependency and feature merging must remain deterministic.
- Temporary directories increase build time and complicate debugging.
- Must ensure source comments and debug mode remain useful.

A narrower variation is to use a temporary `WriterDelegator<RustWriter>` rather than a complete crate, then invoke or reproduce only inline dependency injection. Investigate whether existing APIs expose enough functionality without changing core.

### Option C: generate protocol modules as internal sub-crates

Generate one internal Rust crate per protocol and make the public server crate compose them.

Potential shape:

```text
server crate
  ├── protocol-rest-json crate
  ├── protocol-rpc-v2-cbor crate
  └── shared model/runtime crate
```

Advantages:

- Natural namespace and dependency isolation.
- Same function names are harmless across crates.
- Each protocol can use existing single-protocol codegen unchanged.

Major problems:

- Generated model/input/output/error types currently live in the server crate; serializers in child crates need those types.
- Moving shared types to another crate would be a large public API and architecture change.
- Rust orphan rules and privacy boundaries may block trait implementations across crates.
- Cargo workspace/package generation becomes substantially more complex.
- Runtime plugin/service types and generated operation traits cross crate boundaries.
- Publishing/versioning internal generated crates must be considered.

This is likely too large for the current feature, but it should be evaluated if in-memory merging proves fundamentally unsafe.

### Option D: server-owned copy/fork of protocol function generation

Create server-specific equivalents of `ProtocolFunctions` and/or protocol generators that take explicit destination modules.

Problems:

- Core protocol implementations instantiate/use `ProtocolFunctions` internally, including companion/static `crossOperationFn` calls.
- `ProtocolFunctions` is not designed for substitution at all use sites.
- Forking shared JSON/XML/CBOR protocol generators would duplicate large amounts of logic and likely drift.

This is unlikely to be acceptable unless only a very small, well-defined set of calls needs replacement.

### Option E: generate one protocol at a time and snapshot only dependency objects

Render protocol A, snapshot newly added writer dependencies, remove or isolate them, then render protocol B.

Questions:

- Is there a supported way to remove dependencies from `RustWriter`/`WriterDelegator`? None has been identified yet.
- Can each protocol use a fresh writer while sharing the final file manifest?
- Can dependencies be intercepted before `RustCrate.injectInlineDependencies()` deduplicates them?

This may be a simpler form of Option A if dependency sets can be isolated without text capture.

### Option F: post-process generated files after normal finalization

This is not viable by itself. Finalization has already deduplicated same-key inline dependencies, so later protocol renderers have been discarded. File rewriting cannot recover missing bodies.

It could only work if generation/finalization happens separately per protocol first, as in Option B.

### Option G: pre-register aliases or re-export modules

Creating aliases such as:

```rust
mod protocol_serde_rest_json1 {
    pub use crate::protocol_serde::*;
}
```

is insufficient because different protocols need different function bodies. Aliasing one shared implementation reproduces the correctness bug.

### Option H: a low-level core namespace/remapping facility, not a `ProtocolFunctions` change

If all server-only approaches fail, consider a narrowly generic low-level facility on `InlineDependency`/`RustCrate` that can remap dependency modules during collection.

Potential API idea:

```kotlin
rustCrate.withInlineDependencyNamespace(remapper) {
    serverProtocolGenerator.renderOperation(...)
}
```

The low-level collector would alter both the `RuntimeType` path and dependency module before formatting/deduplication and propagate through nested renderers.

This is still a core change and must be treated as a last resort. It may be more generally defensible than changing `ProtocolFunctions`, but it still affects shared client infrastructure and requires client regression/codegen-diff proof.

## Recommended investigation order

1. **Option A prototype:** temporary server-side writer plus recursive inline-dependency remapping.
2. If writer capture loses imports/module semantics, prototype the narrower **temporary WriterDelegator** variant.
3. **Option B prototype:** per-protocol temporary crate/file manifest and controlled merge.
4. Assess whether shared model types make **Option C sub-crates** impractical; document concrete compiler failures rather than dismissing it abstractly.
5. Only if all server-only designs fail, propose **Option H**, with explicit evidence and a client codegen no-diff test.

Do not continue the large service/runtime review until this architectural question is settled. Otherwise, more work may be built on an unacceptable codegen foundation.

## Focused files for the next investigation

Read these first:

```text
codegen-core/src/main/kotlin/software/amazon/smithy/rust/codegen/core/rustlang/CargoDependency.kt
codegen-core/src/main/kotlin/software/amazon/smithy/rust/codegen/core/rustlang/RustWriter.kt
codegen-core/src/main/kotlin/software/amazon/smithy/rust/codegen/core/smithy/RuntimeType.kt
codegen-core/src/main/kotlin/software/amazon/smithy/rust/codegen/core/smithy/CodegenDelegator.kt
codegen-server/src/main/kotlin/software/amazon/smithy/rust/codegen/server/smithy/ServerCodegenVisitor.kt
codegen-server/src/main/kotlin/software/amazon/smithy/rust/codegen/server/smithy/protocols/ServerHttpBoundProtocolGenerator.kt
```

Existing tests to use as patterns:

```text
codegen-core/src/test/kotlin/software/amazon/smithy/rust/codegen/core/rustlang/InlineDependencyTest.kt
codegen-server/src/test/kotlin/software/amazon/smithy/rust/codegen/server/smithy/ProtocolSpecificModuleTest.kt
```

## Required validation for any replacement

At minimum:

```bash
./gradlew --quiet :codegen-core:compileKotlin
./gradlew --quiet :codegen-server:compileKotlin
./gradlew --quiet :codegen-server:compileTestKotlin
./gradlew --quiet :codegen-server:test --tests '*ProtocolSpecificModuleTest'
./gradlew --quiet :codegen-server:test --tests '*ServerProtocolOrderTest'
```

Then generate a real multi-protocol server and compile it:

```bash
./gradlew codegen-server-test:assemble --quiet
```

Inspect generated modules under:

```text
codegen-server-test/build/smithyprojections/codegen-server-test/
```

Also prove:

- A single-protocol server still uses legacy module names and generated structure.
- Client codegen has no source changes and no generated-code diff caused by the feature.
- Two protocols generate distinct bodies for at least one same-named shape serializer.
- Both implementations survive finalization.
- Event-stream marshallers, error marshallers, and unmarshallers are isolated.
- The established Pokémon examples and event-stream tests remain present.

## Other unfinished review work

After the module-isolation architecture is settled, continue reviewing these areas in small batches:

1. Protocol discovery and ordering:
   - `ServerCodegenVisitor.kt`
   - `ServerProtocolLoader.kt`
   - `ServerProtocolOrder.kt`
   - `ServerCodegenDecorator.kt`
   - `ServerProtocolBasedTransformationFactory.kt`
2. Large generated service changes:
   - `ServerServiceGenerator.kt` (currently the largest codegen diff)
   - builder/root/protocol generator changes
3. Runtime dispatch:
   - `aws-smithy-http-server/src/routing/multi_protocol.rs`
   - protocol detector APIs
   - upgrade/plugin wiring
4. Tests and compile-fail coverage.
5. Regenerate a fresh mainline-based PR diff only after all cleanup.

## Safety and workflow notes

- Do not push; this restricted agent cannot push externally.
- Do not create a commit until explicitly requested.
- Use `~/smithy-latest` or temporary files for isolated experiments, and restore/remove experiment sources afterward.
- Keep each investigation batch small and report the result before proceeding.
