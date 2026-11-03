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

## IDL fixtures

`idl/GROUP/` contains Smithy IDL fixtures: either one `.smithy` file, or a directory of
`.smithy` and `.json` files that are assembled together (to exercise cross-file and mixed
IDL/JSON resolution). They include the examples from the
[Smithy 2.0 IDL specification](https://smithy.io/2.0/spec/idl.html), completed with any
shapes they reference, plus focused edge cases. `expected/idl/outcomes.json` records Java's
ERROR and DANGER event IDs separately and the location and message of its first ERROR event;
`expected/idl/NAME.json` is each accepted fixture as serialized by Java.

`tests/java_idl_fixtures.rs` checks each group:

- `valid/`: Java and Rust both accept, the models are equivalent, and the model survives a
  write and read through the JSON AST.
- `danger/`: as `valid/`, but Java reports only DANGER `SyntacticShapeIdTarget` events, which
  Rust reports as warnings (`Severity` has no DANGER level).
- `invalid/`: both reject, with corresponding diagnostic codes. Syntax errors are reported at
  the same line and column as Java's, except for the cases listed in `COLUMN_DIFFERENCES`, where
  Rust points at the offending character instead of the start of the enclosing token.
- `unsupported/`: Java accepts but this crate deliberately rejects: `apply`, mixins, Smithy 1.0,
  and leading zeroes in numbers (which the IDL grammar forbids but Java's tokenizer accepts).

Other known differences:

- An empty IDL enum is a syntax error here, because the grammar requires a member; Java
  reports it while building the shape (`invalid/syntax_empty_enum`).
- When `$version` is omitted, Java assumes IDL 1.0 and rejects 2.0-only syntax such as
  `= value` defaults. This crate assumes 2.0, as the specification recommends.
  `valid/focused_no_version` therefore uses only syntax valid in both versions.
- Both treat `///` as a documentation comment only at the start of its line, and join every
  such line since the previous token. A `///` after other tokens on its line is an ordinary
  comment here; Java attaches it to a following member, but not to a following shape. The
  grammar does not decide this case (`LineComment` excludes a third `/`). Covered by lexer
  unit tests.
- Java warns about documentation comments that attach to nothing; this crate ignores them.
  Before a closing `}`, both reject them in enums and accept them elsewhere.
- Documentation text from files with lone CR line endings uses `\n` between lines here;
  Java keeps the CRs.
- `-0` loads as the float `0.0`, matching how serde_json reads it in JSON AST input; Java
  reads it as the integer `0`. Covered by `tests/idl.rs`.

## Regenerating

After adding or changing a fixture, regenerate `expected/` with JDK 17+ and the pinned
Smithy jars (for example, from the Gradle cache populated by building smithy-rs):

```bash
cd rust-runtime/aws-smithy-lang/tests/fixtures/java
G=~/.gradle/caches/modules-2/files-2.1/software.amazon.smithy
CP=$(ls $G/smithy-model/1.74.0/*/smithy-model-1.74.0.jar):$(ls $G/smithy-utils/1.74.0/*/smithy-utils-1.74.0.jar):$(ls $G/smithy-jmespath/1.74.0/*/smithy-jmespath-1.74.0.jar)
java -cp "$CP" Regenerate.java .
```
