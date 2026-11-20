# canary

The post-publish build canary. Every check in this repository builds the workspace *before* publish,
against path dependencies. This one runs *after* a release and builds what a consumer would actually
resolve from crates.io.

## Why a second check

`runtime-versioner audit` is the pre-publish guard for this failure class, and its README describes
the shape: `aws-smithy-json` moved from `0.63` to `0.64` while the already-published
`aws-config 1.12.0` still required `aws-smithy-json ^0.63.0`, so a consumer resolved two
incompatible copies.

The audit catches that from inside the repository, before publishing. It cannot catch everything,
because the thing that breaks is a graph that does not exist until the crates are published and a
consumer's own requirements are mixed in. A real red leg here failed with:

```
error[E0308]: mismatched types
    --> aws-config-1.10.1/src/lib.rs:1040:34
     |
     |             builder.set_protocol(self.protocol);
     |                     ------------ ^^^^^^^^^^^^^ expected `SharedClientProtocol`,
     |                                                found a different `SharedClientProtocol`
     |
note: there are multiple different versions of crate `aws_smithy_schema` in the dependency graph
```

Nothing in this repository resolves that graph, so nothing in this repository compiles it. A
consumer writing `aws-config = "~1.10"` does.

## What it checks

Forty legs per release:

- **Six dependency scenarios × five compiler versions.** The first two are manifests we write; the
  other four take their requirements on us from a third party's real `Cargo.toml`, which is the
  point of those rows — patching their requirements would destroy the evidence. The compiler
  versions are the current stable and the four behind it, clamped at the `rust-version` the crates
  declare.
- **Ten build-axis legs**, which vary target or profile rather than dependencies, and so are not
  multiplied by the compiler axis. Four reuse the `new-project` manifest (`musl`, `aarch64`,
  `windows`, `release-profile`); the other six need manifests of their own — `wasm-native` and
  `wasm-wasip2` use `wasm` to reach `aws-smithy-wasm`; `i686`, `powerpc` and `powerpc64` use the
  narrow `serialization` manifest, because CI excludes `aws-smithy-http-client` on those targets and
  a consumer manifest reaches it transitively; and `large-generated-clients` resolves `aws-sdk-ec2`.

Nine manifests back the forty: one per dependency scenario, plus those three. A seventh scenario
resolves three crates declared in `src/test_registry.rs` and assembled on disk as a cargo
`local-registry`, instead of crates.io. Its five legs are excluded from `all` and dispatched by
name, so the matrix can express 45 legs and a release runs 40.

### Four scenarios are switched off, which is 20 legs

`ssstar` and three `prior-aws-config-*` rows are commented out in the table. All four are red
against the crates published today: the three `aws-config` rows fail `E0308` inside `aws-config`
itself, on two majors of `aws-smithy-schema` in one graph, and `ssstar` fails on defects in its own
source. Any hard verdict writes `Red {Tag}`, the series the release alarm watches, so these 20 legs
would cut a ticket on **every release, forever**, about a break no release caused and whoever
receives it cannot act on. An oncall who learns the canary's ticket is always the same ticket stops
reading it, which costs more than 20 legs of coverage.

They are commented rather than deleted because the break is real and each row is still the right
row to be checking — withdrawing it stops the canary repeating itself, and fixes nothing. Each
block carries its reason and its restore condition, and a test fails if an id is switched back on
while it is still known-red.

Worth separating from the stop rule below: the floor tier compares against the previous release, so
a standing red leg does not cost the window its remaining hours. Paging is the reason these four are
off. The cost is that `new-project` is now the only dependency scenario carrying the app body, so
nine legs of the forty can reach a run-phase verdict.

## How a leg runs

Each leg renders a crate, resolves it against crates.io and builds it; the legs that can execute
also run its assertions. The
rendered manifests pin `edition = "2021"`, which pins the dependency resolver to v2. Resolver v3 is
MSRV-aware and would quietly resolve *older* crates on an old toolchain, turning a real break into a
green leg.

## Verdicts

A leg reports one of five. The distinction that matters is the last one: everything else blames the
published crates, and `INFRA_FAIL` blames the runner.

| Verdict | Means |
|---|---|
| `RESOLVE_FAIL` | cargo refused to resolve the graph at all |
| `COMPILE_FAIL` | `cargo build` failed with a real compiler diagnostic — what a consumer compiles |
| `HARNESS_FAIL` | only the test binary failed to compile, so no consumer-facing code is implicated |
| `RUNTIME_FAIL` | it built, ran, and an assertion failed |
| `INFRA_FAIL` | not attributable to the release: fetch, DNS, toolchain install, the runner |

A hard verdict requires positive evidence. The three verdict-owning commands run `--offline` after
`cargo fetch --locked` has populated the cache, so they structurally cannot reach the network —
which is what makes "no compiler diagnostic" mean "the runner broke" rather than "the registry was
slow". Anything without that evidence is `INFRA_FAIL`, because a misattributed page sends the
responder to the wrong place.

## Commands

```sh
# The leg matrix, and what the MSRV clamp dropped from the toolchain span
cargo run -- gen-matrix --stable 1.99.1 --msrv 1.94.1 --pretty
cargo run -- toolchains --stable 1.99.1 --msrv 1.94.1

# Render one leg's crate, then run it: resolve, build, test, with retries and a deadline
cargo run -- gen-leg  --leg new-project--stable --stable 1.99.1 --msrv 1.94.1 --out /tmp/leg
cargo run -- run-leg  --leg new-project--stable --stable 1.99.1 --msrv 1.94.1 --dir /tmp/leg

# Which runtime crates this checkout declares that crates.io does not have yet.
# The path is the repository root, which is three levels up from this crate.
cargo run -- publish-status --repo ../../..

# Has a manifest's resolved graph moved since the last time we looked
cargo run -- baseline --manifest new-project --lockfile /tmp/leg/Cargo.lock \
    --store-dir /tmp/baselines --update

# Fold a run's leg reports into one record, for a release that broke
cargo run -- summarize --release release-2026-11-09 --run-id 123 --run-url ... \
    --tier dispatch --stable 1.99.1 --msrv 1.94.1 --reports ./artifacts --out ./record
```

`run-leg --reuse-crate` runs whatever is already in `--dir` instead of rendering over it. It exists
so a leg can be perturbed by hand — a broken assertion, an unsatisfiable requirement — and the real
pipeline run over it, which is how the verdicts that no scenario produces on purpose get induced. It
must never be used in a release run: a leg's guarantee is that it resolves the published graph
afresh.

## When it runs

Two tiers, because the two questions have different shapes.

The **dispatch tier** covers the publish window, when crates are arriving one at a time and the
graph is changing under consumers. It ticks every 15 minutes, resolves every manifest, and
dispatches legs only for the manifests whose graph actually moved — a tick with no publish in it
resolves exactly what the previous tick did, so running the whole matrix to re-prove it buys
nothing. It ends when `publish-status` reports that every declared runtime crate has arrived, or at
a three-hour ceiling with crates still outstanding, which is itself a finding.

The **floor tier** then runs hourly for 24 hours with no gate, because a graph can break without
anything in this repository moving: a third-party crate publishes, and a consumer's resolution
changes.

A hard verdict on a leg that was not already red before this release stops the remaining hours. The
team has already been paged, and later hours would dedupe into the same ticket and page nobody. A
leg red for a reason nobody has fixed is not news and does not stop it, and nor does `INFRA_FAIL`: a
registry hiccup is expected to clear.

## Where the evidence goes

Three places, with different lifetimes.

- **The job log** carries the full diagnosis and is the fastest way in while the run exists.
- **An artifact per leg**, kept 14 days. Every leg that ran uploads its `report.json`; a leg that reached a
  verdict also uploads the rendered crate and its `Cargo.lock`, which is the reproduction recipe.
  A green leg has nothing to reproduce, and a superseded leg uploads nothing because it never built.
- **One record per release**, folded from every leg's report and kept 14 days as an artifact.

A permanent archive of those records is deliberately deferred. It would answer "how often does this
happen, and to what" -- artifacts, logs and the runs themselves are kept 90 days, so three months
on there is nothing left to count -- but that is a historical question, not the one this canary
exists for, and it cost a durable bucket and a third IAM role to answer. The hooks for it are
gone rather than dormant, so adding it later is a deliberate change rather than flipping a switch.

The record never infers success from silence. A leg that died without reporting is a defect in the
canary, so legs land in four buckets — red, green, superseded, unreported — and a leg reaches
`green` only by having submitted a report that says so.

## Testing

```sh
cargo test                                  # no test touches the network
cargo clippy --all-features
```

Every verdict has been induced through the real runner rather than only asserted in a test, which is
how most of the defects in this crate were found — a hang that reported nothing, a `RESOLVE_FAIL`
summary that showed cargo's suggestion instead of its error, a green leg that had run zero
assertions, and a report that could be written but not read back.
