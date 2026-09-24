# runtime-versioner

Runtime versioner serves two purposes:

1. `audit` the runtime crates to ensure that (a) if their contents have changed,
   the version number in the crate has been updated, and (b) the dependency
   requirements already published for their current versions still accept the
   current versions of the runtime crates they depend on. This is run as part of
   pre-commit.
2. `patch-runtime`: Used by `check-semver-hazards` (and manually) to test a
   specific set of runtime crates against the generated AWS SDK. This works by
   utilizing
   [Cargo's source patching](https://doc.rust-lang.org/cargo/reference/overriding-dependencies.html).

## The dependency requirement check

At release time, `publisher fix-manifests` replaces each local path dependency's
version with the dependency's complete version, for example
`version = "0.63.0"`. Cargo reads that as a caret requirement, so it accepts
`0.63.x` but not `0.64.0`.

If a runtime crate later moves to an incompatible version line, every
already-published dependent keeps requiring the old line. Consumers then resolve
two incompatible copies of the dependency. This is what happened when
`aws-smithy-json` moved from `0.63` to `0.64` while the published
`aws-config 1.12.0` still required `aws-smithy-json ^0.63.0`: because no file
under `aws-config/` changed, the content check did not ask for a version bump,
and `aws-config` was not released.

The audit therefore also checks, for every publishable runtime crate whose
current version exists in crates.io, that each normal and build dependency
requirement published for that version accepts the dependency's current version
in this repo. It uses the requirement recorded in the crates.io index, so it
doesn't depend on which release tag the audit was given and stays correct across
the decoupled smithy-rs and SDK release trains. Crates whose current version
isn't published yet need nothing further, because `fix-manifests` stamps the
dependency's current version before publishing them.

To fix a finding, give the dependent crate a new, unpublished version. The tool
does not choose the bump size; semver correctness is still a human decision.

What it does and doesn't cover:

- Dev dependencies are ignored: a dependency changing version lines doesn't
  require republishing a crate just to update its tests and examples.
- A dependency entry is only checked if it has a `path` key. A version-only
  entry is an intentional pin to a published line. To keep depending on an older
  line deliberately, declare it as a separate renamed, version-only entry
  alongside the current path dependency, as `aws-smithy-http-server-metrics`
  does with `aws-smithy-http-server-065`.
- Paths are never resolved. `aws-config` points at the generated SDK tree, which
  is gitignored and normally absent when the audit runs, so the `path` key is
  treated purely as a marker and the dependency's version comes from the runtime
  crate of that name.
- Dependencies on third-party crates and on generated SDK clients (such as
  `aws-config -> aws-sdk-sts`) are out of scope; they aren't runtime crates.
- No Cargo resolution, feature resolution, MSRV, or API compatibility analysis
  is performed.

### Known `fix-manifests` limitations

Neither of these affects any current runtime crate, and the audit models
dependency entries correctly regardless, but `publisher fix-manifests` would
need work before a runtime crate could use either form:

1. `update_dep` looks up the version to stamp by the manifest key rather than
   honoring `package = `, so a renamed path dependency would fail to resolve.
2. `fix_dep_sets` only rewrites the top-level `dependencies`,
   `dev-dependencies`, and `build-dependencies` tables, so a path dependency
   under `[target.<cfg>.*]` would not get a version stamped into it.

## Compatibility transition mode

`patch-runtime` and `patch-runtime-with` accept an opt-in
`--allow-compatibility-transition` flag. It is **off by default**, and the
default behavior of both subcommands is unchanged.

Cargo ignores a `[patch.crates-io]` entry whose version does not satisfy the
requirement it is meant to replace. So when a runtime crate moves to a new Cargo
compatibility line (for example `aws-smithy-json` `0.63.x` -> `0.64.x`), the old
SDK release cannot be patched at all: every old requirement still names the
previous line, and the patch silently goes unused.

With `--allow-compatibility-transition`, after `sdk-versioner` converts the
checked-out old SDK to version-only dependencies, `patch-runtime`:

1. Walks `sdk/**/Cargo.toml` in the old SDK, excluding nested workspace roots
   (such as fuzz crates) that do not consume the root patch table, and inspects
   the normal, `dev`, `build`, and `target.<cfg>.*` dependency tables, honoring
   `package = "..."` aliases. For every dependency that targets a crate being
   patched in, a version requirement that does **not** accept the patched
   version is rewritten to that version. Requirements that already accept it are
   left alone, and `features`, `default-features`, `optional`, and `package` are
   preserved. Every rewrite is reported.
2. Routes `aws-config` through the rewritten old SDK crate (`sdk/aws-config`) by
   adding it to `[patch.crates-io]`. The smithy-rs copy of `aws-config` is never
   used as a patch source, because it depends on generated SDK crates that do
   not exist in this workspace.
3. After `cargo update`, parses the resulting `Cargo.lock` and fails if any
   expected patched crate is missing as an unsourced (path) package. For
   `aws-config` it also fails if any registry-resolved copy remains, regardless
   of version. Patches with no old SDK dependency edge (for example the DNS and
   OpenTelemetry integrations, which nothing in the SDK depends on) are
   legitimately unused and are not required.

### What this does and does not prove

This mode exercises the **coordinated post-release dependency graph**: the graph
that exists only once every affected crate in the release has been published
together.

It explicitly **waives** compatibility with:

- the already-published old `aws-config`, which pins the previous compatibility
  line of the patched runtime crates, and
- any partial update, where a consumer upgrades only some of these crates.

A passing run does not claim that compatibility with those consumers has been
restored. It only claims that the coordinated graph resolves, builds, and passes
the old SDK's tests. The command prints this waiver before and after it runs.

`check-semver-hazards` requires an explicit `true` or `false` authorization
argument. Pull-request CI queries the current PR labels and passes `true` only
when `breaking-change` is present; an unlabelled PR therefore runs the original
strict check. Main, scheduled, and manual CI have no PR label context and run
the accepted transition after it has passed the PR gate. In transition mode the
script prints the waiver before testing.
