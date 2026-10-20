# Endpoint BDD compiler parity fixtures

`expected/` records every stage of Smithy Java's `CompileBdd.compileBdd` from
`software.amazon.smithy:smithy-rules-engine:1.73.0`, the version smithy-rs pins in
`gradle/libs.versions.toml`. `tests/endpoint_bdd_java.rs` checks the fixtures against the
checked-in models and against this crate's codecs. A Rust compiler must reproduce each stage
exactly; normal Rust tests never run Java.

## Inputs

- `aws-NAME`: every `endpointRuleSet` in `aws/sdk/aws-models`, read from the model.
- `aws-s3-rewritten`: S3 after `S3TreeRewriter` (from `smithy-aws-endpoints`), which is what
  `CompileBddForAws` compiles for S3. The rewrite keeps version 1.0 while introducing negative
  `getAttr` indexes, which `RulesEngineVersionValidator` and this crate reject. The fixture
  records it as 1.1; the generator checks that this does not change the compiled BDD.
- `tests-NAME`: the rule sets shipped in `smithy-rules-engine-tests`, which use newer functions
  (`coalesce`, `ite`, `split`, and others).
- `random-NNN`: seeded rule sets from `Generate.java`'s `RandomRuleSet`. They exercise reused
  assignment names across siblings (SSA renaming), nesting, negation, and binding coalescing.

Java 1.73.0 reproduces every checked-in SDK `endpointBdd` byte for byte (S3 via the rewrite).
The tests fail if a model's rule set or BDD changes without regenerating the fixtures.

## Format

`manifest.json` lists the fixtures, the pinned version, and the random seeds Java fails on.
Each `NAME.json` has:

| Member          | Stage                                                                   |
| --------------- | ----------------------------------------------------------------------- |
| `ruleSet`       | The input, except for `aws-NAME` fixtures, whose input is the model.    |
| `ssa`           | `SsaTransform.transform`, run by `CfgBuilder`; omitted if unchanged.    |
| `cfgConditions` | `Cfg.getConditions()`: the CFG's interned conditions, in order.         |
| `unoptimized`   | `EndpointBddTrait.from(cfg)`.                                           |
| `sifted`        | `SiftingOptimization`.                                                  |
| `costOptimized` | `CostOptimization`.                                                     |
| `reversed`      | `NodeReversal`.                                                         |
| `compiled`      | `removeUnreferencedConditions()`: the complete `endpointBdd` trait.     |

Parameters, results, and version are identical in every BDD stage, so only `compiled` has them.
The other BDD stages record `order` (condition indexes into `cfgConditions`), `root`,
`nodeCount`, and `nodes`; the tests rebuild full traits from them.

## Determinism

The generator compiles every rule set twice and fails unless the stages are identical, and
checks that the staged run equals `CompileBdd.compileBdd`. Sifting evaluates positions in
parallel but breaks ties by size, then cost, then position, so thread scheduling does not
affect the result. No stage uses a time budget.

## Java failures and `java-fixes.patch`

Java's compiler fails on some valid rule sets (accepted by Java's own parser, type checker, and
`RuleEvaluator`). `manifest.json`'s `javaFailures` lists the random seeds and Java's errors.
`java-fixes.patch` is a minimal fix for each cause, applied to the pinned sources:

- `No field named X`: `CoalesceTransform` inlines a binding used by exactly one later
  condition, counting only condition references. Uses in the rule's result or nested rules
  no longer resolve. The fix counts those uses too.
- `Invalid shadowing of X`: `VariableConsolidationTransform` renames a binding to the name an
  identical expression was bound to elsewhere, even when that name is already assigned in
  scope. The fix keeps the input rule set when consolidation produces an invalid one.
- `No field named X` (again) and `Checking type OptionalType[...] that doesn't match cached
  type ...`: `EndpointBddTrait.removeUnreferencedConditions` removes conditions that unreachable
  results still need to type-check: an assignment they use, or an `isSet` that proves a
  reference set. The fix keeps those conditions.

Those seeds are recorded with pipeline `CompileBdd with java-fixes.patch`. The patched run fails
unless every other fixture is byte-identical to the unpatched run. Each random fixture is also
checked with Java's `RuleEvaluator`: the compiled BDD must select the same result as the rule set
for 300 seeded parameter values. Seeds where too few values reach a result are skipped.

## Regenerating

With JDK 17+ and the pinned jars, including the `smithy-rules-engine` sources jar (for example,
from the Gradle cache populated by building smithy-rs; set `SMITHY_JARS` to use another
directory laid out the same way):

```bash
rust-runtime/aws-smithy-lang/tests/fixtures/endpoint_bdd/generate.sh
```

It runs `Generate.java` unpatched, then again with `java-fixes.patch` compiled ahead of the jars.
It takes about three minutes, mostly compiling S3 several times.
