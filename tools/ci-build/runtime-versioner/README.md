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

1. Scans `sdk/**/Cargo.toml` in the old SDK, excluding `target/` directories
   and nested workspace roots that do not consume the root patch table. It checks
   normal, `dev`, `build`, and `target.<cfg>.*` dependency tables and honors
   `package = "..."` aliases.
   Requirements that already accept a patched version remain unchanged and make
   that patch part of the verified set. An incompatible requirement is rewritten
   only in `sdk/aws-config/Cargo.toml`, because transition mode explicitly
   supplies that crate. Generated SDK client manifests are deliberately not
   rewritten: their source may target an intentionally incompatible older `0.x`
   runtime API, so they retain their published requirements and Cargo may resolve
   both compatibility lines. Rewrites preserve `features`, `default-features`,
   `optional`, and `package`, and every rewrite is reported.
2. Routes that rewritten old SDK `aws-config` through `[patch.crates-io]`. The
   smithy-rs copy of `aws-config` is never used as a patch source, because it
   depends on generated SDK crates that do not exist in this workspace.
3. After `cargo update`, parses the resulting `Cargo.lock` and fails if an
   expected patch used by `aws-config` or accepted by an old SDK requirement is
   missing as an unsourced (path) package. It also fails if any registry-resolved
   `aws-config` copy remains, regardless of version. Other patches may
   legitimately go unused when they have no old SDK edge, are referenced only
   by optional dependencies, or are referenced only by incompatible published
   requirements that remain on the old line.

### What this does and does not prove

This mode exercises the updated runtime and `aws-config` dependency graph while
keeping the previous release's generated client source on the runtime
compatibility lines it was generated against.

It explicitly **waives** compatibility with:

- the already-published old `aws-config`, which pins the previous compatibility
  line of the patched runtime crates, and
- partial updates among runtime and configuration crates that must move together.

A passing run does not claim that every partial update is compatible. It claims
that the transitioned runtime and `aws-config` graph can coexist with the old
SDK clients and that those clients' tests still pass. The command prints this
waiver before and after it runs.

`check-semver-hazards` requires an explicit `true` or `false` authorization
argument. Pull-request CI queries the current PR labels and passes `true` only
when `breaking-change` is present; an unlabelled PR therefore runs the original
strict check. Main, scheduled, and manual CI have no PR label context and run
the accepted transition after it has passed the PR gate. In transition mode the
script prints the waiver before testing.
