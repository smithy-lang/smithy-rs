# Java compatibility fixtures

`valid/` and `invalid/` contain small Smithy JSON AST documents. `expected/` records how
`software.amazon.smithy:smithy-model:1.74.1` handles them:

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
`expected/idl/NAME.json` is each accepted fixture as serialized by Java. For accepted fixtures
with shapes that use mixins, `expected/idl/NAME.mixins.json` records how Java resolved them:
each such shape's mixins, its effective and own (introduced) traits, and its members in order
with their targets, mixin members, and effective and own traits. The serialized model cannot
show this, because it keeps mixins in their declared form, which both implementations resolve
when loading it.

`tests/java_idl_fixtures.rs` checks each group:

- `valid/`: Java and Rust both accept, the models are equivalent, mixins resolve to the same
  members and traits, and the model survives a write and read through the JSON AST.
- `danger/`: as `valid/`, but Java reports only DANGER `SyntacticShapeIdTarget` events, which
  Rust reports as warnings (`Severity` has no DANGER level).
- `invalid/`: both reject, with corresponding diagnostic codes. Syntax errors are reported at
  the same line and column as Java's, except for the cases listed in `COLUMN_DIFFERENCES`, where
  Rust points at the offending character instead of the start of the enclosing token.
- `unsupported/`: the reference implementation accepts but this crate deliberately rejects:
  mixins of shape types other than structure, union, enum, and intEnum, a shape that is its own
  mixin, Smithy 1.0, leading zeroes in numbers (which the IDL grammar forbids but the
  reference tokenizer accepts), unknown properties in a JSON AST `apply` entry (the
  reference implementation warns; this crate rejects unknown properties everywhere), and a
  line break or comment between `apply` and its target (the grammar requires spaces,
  `%s"apply" SP ShapeId`; the reference implementation accepts any whitespace).

Other known differences:

- An empty IDL enum is a syntax error here, because the grammar requires a member; Java
  reports it while building the shape (`invalid/syntax_empty_enum`,
  `invalid/validators_mixins_empty_enum_mixin`). Like Java, an enum with mixins may declare no
  members and inherit them all (`valid/validators_mixins_non_empty_enum`), although the grammar
  does not allow it.
- When a member is inherited from more than one mixin, Java counts the traits of its second and
  later mixin members as the member's own (introduced) traits, and so writes them in an `apply`
  entry. Here a member's own traits are only those applied to it. The effective traits are the
  same, so the models are equivalent; `mixins_resolve_like_reference_impl` compares own traits
  only for members with one mixin.
- Java lists the other shapes of a mixin cycle in the order of a hash map; this crate lists them
  in the order the shapes were first seen.
- Java drops a shape that is its own mixin, and the shapes that use it, without a diagnostic;
  this crate reports it like any other mixin cycle (`unsupported/focused_mixins_self_reference`).
- When `$version` is omitted, Java assumes IDL 1.0 and rejects 2.0-only syntax such as
  `= value` defaults. This crate assumes 2.0, as the specification recommends.
  `valid/focused_no_version` therefore uses only syntax valid in both versions.
- Both treat `///` as a documentation comment only at the start of its line, and join every
  such line since the previous token. A `///` after other tokens on its line is an ordinary
  comment here; Java attaches it to a following member, but not to a following shape. The
  grammar does not decide this case (`LineComment` excludes a third `/`). Covered by lexer
  unit tests.
- The reference implementation warns about documentation comments that attach to nothing
  (including inside an `apply` block); this crate ignores them. Before a closing `}`, both
  reject them in enums and accept them elsewhere.
- Documentation text from files with lone CR line endings uses `\n` between lines here;
  Java keeps the CRs.
- `-0` loads as the float `0.0`, matching how serde_json reads it in JSON AST input; Java
  reads it as the integer `0`. Covered by `tests/idl.rs`.

## Assembly fixtures

`assembly/CASE/` contains a `case.json` with ordered assembler steps, plus the files they use.
They cover inputs added in code, which file fixtures cannot express:

- `{"addShapes": FILE}` loads `FILE` and adds each of its shapes as if built in code
  (`addShape` / `add_shape`). With `"disablePrelude": true`, the file is loaded without the
  prelude so that it can define shapes in the prelude namespace.
- `{"addModel": FILE}` loads `FILE` and adds the resulting model (`addModel` / `add_model`).
- `{"addFile": FILE}` adds a `.json` or `.smithy` file (`addImport` / `add_json_file`,
  `add_idl_file`).
- `{"putMetadata": KEY, "value": VALUE}` adds metadata in code.
- `{"addTrait": SHAPE_ID, "trait": TRAIT_ID, "value": VALUE}` applies a trait in code.

`case.json` may also list `locations`: shapes, members, or traits (`SHAPE_ID@TRAIT_ID`) whose
source file `expected/assembly/outcomes.json` records, which shows which of several definitions
the reference implementation keeps. The outcomes also record whether each case is accepted and
the IDs of ERROR and DANGER events; `expected/assembly/CASE.json` is each accepted case as
serialized by the reference implementation, including shapes the case adds to the prelude
namespace but not the prelude's own shapes.

`case.json` may also have `"serializeWithPrelude": {"from": FILE, "to": OUTPUT}`: regenerating
writes `OUTPUT` as `FILE` serialized by the reference implementation with its prelude shapes
(as the smithy-build `model` plugin writes it with `includePreludeShapes`), for the steps to
load. `OUTPUT` is committed with the case, and only regenerating keeps it in sync with `FILE`:
regenerate after changing `FILE` or moving the pinned Smithy version.

`tests/java_assembly_fixtures.rs` replays the same steps and checks that accepted cases produce
an equivalent model (including the order of merged arrays and metadata) with the same source
files, and that rejected cases report corresponding diagnostic codes.

## Pagination fixtures

`paginated/` contains `.smithy` or `.json` models for `knowledge::PaginatedIndex`. Unlike
`knowledge/` fixtures, they are assembled with full validation, including Java's
`PaginatedTraitValidator`, and regenerating fails if Java reports an ERROR or DANGER event:
the index is meant to agree with Java only on valid models. `expected/paginated/NAME.json`
records Java's `PaginatedIndex` answer for every service and every operation of the model,
`null` where Java has no pagination information. Otherwise it records the effective trait,
the input and output structures, and every member of each resolved path.

`tests/java_fixtures.rs` checks that every Rust query succeeds and gives the same answer.
`paginated/paginated.smithy` is also the valid model of the crate's pagination unit tests.

For invalid models, this crate intentionally differs from Java's index. The unit tests cover
these cases:

- An `inputToken` or `outputToken` that is missing or does not resolve is an error here; Java
  returns no pagination information.
- A `pageSize` or `items` value that does not resolve is an error here; Java omits it.
- An output path with an empty member name is an error here. Java splits paths with
  `Pattern.split`, which drops trailing empty names, so `token.` resolves to `token`.

## Waiter fixtures

`waiters/GROUP/` contains `.smithy` models with `smithy.waiters#waitable` traits. Java
assembles each with full validation and the trait definitions and validators of
`software.amazon.smithy:smithy-waiters:1.74.1`; Rust assembles each with
`waiters::validator()` and no trait definitions.
`expected/waiters/GROUP/NAME.json` records whether Java accepts the fixture (no ERROR event),
its ERROR, DANGER, and WARNING events, and each operation's waiters as Java's typed
`WaitableTrait` decodes them, with defaults applied.

`tests/java_fixtures.rs` maps each Java event to a severity (DANGER is a warning) and a
diagnostic code, and compares the set of shapes, severities, and codes with Rust's
diagnostics. The number and wording of diagnostics, and parser offsets, are not compared. Where
both implementations decode a trait, the waiters must be equal; Rust may fail to decode a value
Java decodes only if Java also rejects the model.

- `valid/`: Java accepts, and so does Rust, with warnings on the same shapes.
- `invalid/`: both reject, with errors on the same shapes. Each operation has one problem.
  Java stops before validation when a trait value cannot be decoded, and stops before the
  waiter validators when a trait value or target is invalid, so each fixture only has
  problems that Java finds in the same phase.
- `differences/`: this crate deliberately disagrees. `WAITER_DIFFERENCES` in the test records
  Rust's diagnostics for each, and the test fails if Java starts to agree:
  - `same_operation_names`: two waiter names of one operation that differ only by case are an
    error here. Java only compares names across operations; the specification forbids both.
  - `unknown_message_type`: a `message` whose type static analysis cannot know is accepted
    here. Java requires its sample evaluation to produce a string or an array of strings.
  - `projected_message`: a projected `message`, such as `items[].reason`, is accepted here.
    Java checks the element type by appending `[0]`, which indexes each projected value
    instead, so it rejects the message.
  - `argument_types`: a function argument of the wrong type whose sample in Java might have
    another type is a warning here; Java reports an error when its sample has the wrong type.
    This crate reports an error only where Java's sample is certain to have the wrong type
    (see `waiters::validator`).
  - `function_results`: Java's linter gets the result types of `ends_with`, `max_by`,
    `sort_by`, and `merge` wrong, and reports DANGER events for valid expressions.
  - `expression_limits`: expressions longer than 2048 bytes or nested more than 32 levels
    deep are rejected here without parsing, so that model content cannot exhaust the stack.

## Regenerating

After adding or changing a fixture, regenerate `expected/` with JDK 17+ and the Smithy jars
of the version smithy-rs pins in `gradle/libs.versions.toml` (for example, from the Gradle
cache populated by building smithy-rs). `Regenerate.java`'s `VERSION` must match. It also writes
the prelude this crate embeds, `src/prelude/prelude-VERSION.json` (see `src/prelude/README.md`):

```bash
cd rust-runtime/aws-smithy-lang/tests/fixtures/java
V=1.74.1
G=~/.gradle/caches/modules-2/files-2.1/software.amazon.smithy
CP=$(for jar in smithy-model smithy-utils smithy-jmespath smithy-model-jmespath smithy-waiters; do
    ls $G/$jar/$V/*/$jar-$V.jar
done | paste -sd: -)
java -cp "$CP" Regenerate.java .
```
