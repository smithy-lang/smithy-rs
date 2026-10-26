# Java compatibility fixtures

`valid/` and `invalid/` contain small Smithy JSON AST documents. `expected/` records how
`software.amazon.smithy:smithy-model:1.74.0` handles them:

- `expected/outcomes.json`: whether Java accepts each fixture, and the IDs of any ERROR or
  DANGER validation events.
- `expected/NAME.json`: each accepted fixture as serialized by Java's `ModelSerializer`.
- `expected/knowledge/NAME.json`: for each `knowledge/` fixture, the answers of Java's
  `TopDownIndex`, `OperationIndex`, and `BottomUpIndex`. Knowledge fixtures are assembled with
  Java validation disabled, because they exercise structural cases (such as lifecycle
  operations without identifier bindings) that Java's full validator rejects.

`tests/java_fixtures.rs` checks that this crate agrees with Java on every outcome, and that
Java's serialized output loads to a model equivalent to the original fixture. It also checks
that the `knowledge` indexes agree with Java's. Java's `BottomUpIndex` records one parent path
per shape even when several exist, so bottom-up answers are compared only where this crate
proves the path is unique; ambiguous paths are covered by the crate's own tests. Normal Rust
tests never run Java.

Valid fixtures use only prelude traits or traits they define, because Java rejects applied
traits without definitions. Invalid fixtures only cover rules that `StructuralV1` enforces.

## Regenerating

After adding or changing a fixture, regenerate `expected/` with JDK 17+ and the pinned
Smithy jars (for example, from the Gradle cache populated by building smithy-rs):

```bash
cd rust-runtime/aws-smithy-lang/tests/fixtures/java
G=~/.gradle/caches/modules-2/files-2.1/software.amazon.smithy
CP=$(ls $G/smithy-model/1.74.0/*/smithy-model-1.74.0.jar):$(ls $G/smithy-utils/1.74.0/*/smithy-utils-1.74.0.jar):$(ls $G/smithy-jmespath/1.74.0/*/smithy-jmespath-1.74.0.jar)
java -cp "$CP" Regenerate.java .
```
