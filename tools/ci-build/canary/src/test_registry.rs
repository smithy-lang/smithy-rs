/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! A registry the canary can break, without publishing anything to crates.io.
//!
//! The canary's own correctness is hard to test for one reason: the thing it detects is a break in
//! *published* crates, and we cannot publish a broken crate to prove it works. crates.io has no
//! delete — only yank — so a deliberately broken version would exist under our name forever, and
//! the crate name itself could never be withdrawn.
//!
//! So the break goes in a registry we assemble on disk instead. Cargo's `local-registry` source
//! replacement makes this a directory rather than a service: a few `.crate` files beside an index,
//! pointed at from a `.cargo/config.toml` that only the test legs get. No server, no listening
//! port, no network, and nothing published anywhere.
//!
//! # What this reproduces, and what it cannot
//!
//! It reproduces the *shape* that caused the outage this canary exists for: two majors of one crate
//! in a single graph, with a type crossing between them. Measured, offline, from the table below:
//!
//! ```text
//! Locking 4 packages
//!   ct-types 0.1.0
//!   ct-types 0.2.0                   <- two majors in one graph
//! error[E0308]: mismatched types: expected `ct_schema::Shared`, found `ct_types::Shared`
//! ```
//!
//! It cannot reproduce the real publish path, or the timing of the real index propagating. It is
//! better than a real release at everything else: any intermediate state of a release can be
//! constructed here deliberately, including the ones that exist for ninety seconds and are
//! therefore almost impossible to catch by waiting.
//!
//! # The committed state is green, deliberately
//!
//! The table below resolves to a single major of everything, so the test scenario passes on every
//! release and pages nobody. Breaking it is an edit — add a version, move a requirement — and
//! withdrawing it is reverting that edit. A permanently broken test scenario would cut a ticket on
//! every release about something nobody can act on, which is the same mistake as leaving a
//! known-red row switched on.
//!
//! [`tests::the_committed_registry_resolves_to_one_major`] is what keeps that true.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::Context;

/// Crates in the family are prefixed so that nobody can mistake one for something shipped, and so
/// that a stray `.crate` file found on a runner is self-explanatory.
pub const PREFIX: &str = "ct-";

/// One version of one crate in the test registry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TestCrate {
    pub name: &'static str,
    pub version: &'static str,
    /// `(crate, requirement)` on other members of this family. Never on anything real: the family
    /// is self-contained so that the registry needs no proxy and the legs need no network.
    pub deps: &'static [(&'static str, &'static str)],
    /// The whole of `src/lib.rs`.
    pub lib: &'static str,
}

/// The family, in dependency order.
///
/// Three crates, mirroring the roles in the outage rather than its size. A real release publishes
/// twenty-nine crates across twelve batches; what mattered was that a leaf landed in the first
/// batch and a mid-tier crate in the eighth, leaving a window where a consumer could resolve the new
/// leaf and an old mid-tier that still required the previous major.
///
/// * `ct-types` is the leaf holding a type that crosses a crate boundary. That crossing is the
///   mechanism: two majors of a crate in one graph is legal and harmless until a type from it
///   passes between them.
/// * `ct-schema` is the crate that pins a requirement and falls behind — the `aws-smithy-schema`
///   role.
/// * `ct-client` is what a test scenario depends on — the `aws-config` role.
pub const TEST_REGISTRY: &[TestCrate] = &[
    TestCrate {
        name: "ct-types",
        version: "0.1.0",
        deps: &[],
        lib: "pub struct Shared(pub u8);",
    },
    TestCrate {
        name: "ct-schema",
        version: "0.1.0",
        deps: &[("ct-types", "0.1")],
        lib: "pub use ct_types::Shared;\npub fn take(_: Shared) {}",
    },
    TestCrate {
        name: "ct-client",
        version: "0.1.0",
        deps: &[("ct-types", "0.1"), ("ct-schema", "0.1")],
        lib: "pub fn go() { ct_schema::take(ct_types::Shared(1)); }",
    },
];

/// Where an index entry for a crate lives, by cargo's convention.
///
/// One, two and three-character names are special-cased; everything longer is nested under its
/// first two and next two characters. Wrong paths are not an error, they are a crate cargo cannot
/// see, so this is worth a test of its own.
pub fn index_path(name: &str) -> PathBuf {
    let p = match name.len() {
        0 => unreachable!("a crate with no name cannot be declared"),
        1 => format!("1/{name}"),
        2 => format!("2/{name}"),
        3 => format!("3/{}/{name}", &name[0..1]),
        _ => format!("{}/{}/{name}", &name[0..2], &name[2..4]),
    };
    PathBuf::from(p)
}

/// One line of the index, as cargo's registry format wants it.
fn index_line(c: &TestCrate, cksum: &str) -> anyhow::Result<String> {
    let deps: Vec<serde_json::Value> = c
        .deps
        .iter()
        .map(|(name, req)| {
            serde_json::json!({
                "name": name,
                "req": req,
                "features": [],
                "optional": false,
                "default_features": true,
                "target": null,
                "kind": "normal",
            })
        })
        .collect();
    Ok(serde_json::to_string(&serde_json::json!({
        "name": c.name,
        "vers": c.version,
        "deps": deps,
        "cksum": cksum,
        "features": {},
        "yanked": false,
    }))?)
}

/// The `.cargo/config.toml` that points a leg at the registry instead of at crates.io.
///
/// Source replacement rather than an alternative registry, because the leg's manifest should read
/// exactly as a consumer's would — a plain version requirement, no `registry = "..."` key. The
/// swap is invisible to the manifest, which is what keeps the test scenario shaped like the real
/// ones.
pub fn cargo_config(registry: &Path) -> String {
    format!(
        "# Written by the canary. Points this leg at a registry assembled on disk, so a break can\n\
         # be induced without publishing anything to crates.io.\n\
         [source.crates-io]\n\
         replace-with = \"canary-test-registry\"\n\
         \n\
         [source.canary-test-registry]\n\
         local-registry = {:?}\n",
        registry.display().to_string()
    )
}

/// Assemble the registry at `out`, returning the path to hand to `local-registry`.
///
/// Built **in dependency order**, with each crate packaged against the registry as it stands so
/// far. `cargo package` resolves its dependencies even under `--no-verify`, so `ct-schema` cannot
/// be packaged until `ct-types` is already in the index — measured, after assuming otherwise and
/// watching it fail with `location searched: crates.io index`. [`TEST_REGISTRY`] is therefore
/// ordered, and that ordering is load-bearing rather than tidiness.
pub fn build(out: &Path, work: &Path) -> anyhow::Result<PathBuf> {
    // The index directory has to exist before the first `cargo package`, even for a crate with no
    // dependencies: source replacement makes cargo open the replacement source regardless.
    std::fs::create_dir_all(out.join("index"))
        .with_context(|| format!("creating {}/index", out.display()))?;

    for c in TEST_REGISTRY {
        let dir = work.join(format!("{}-{}", c.name, c.version));
        let src = dir.join("src");
        std::fs::create_dir_all(&src).with_context(|| format!("creating {}", src.display()))?;

        // Packaged against the partially-built registry, which is what lets a member depend on one
        // added earlier.
        let cargo_dir = dir.join(".cargo");
        std::fs::create_dir_all(&cargo_dir)
            .with_context(|| format!("creating {}", cargo_dir.display()))?;
        std::fs::write(cargo_dir.join("config.toml"), cargo_config(out))
            .with_context(|| format!("writing {}/config.toml", cargo_dir.display()))?;

        let mut toml = format!(
            "[package]\nname = {:?}\nversion = {:?}\nedition = \"2021\"\n\
             license = \"Apache-2.0\"\n\
             description = \"Canary test only. Never published; assembled on disk to exercise the \
             post-publish build canary.\"\n",
            c.name, c.version
        );
        if !c.deps.is_empty() {
            toml.push_str("\n[dependencies]\n");
            for (name, req) in c.deps {
                toml.push_str(&format!("{name} = {req:?}\n"));
            }
        }
        std::fs::write(dir.join("Cargo.toml"), toml)?;
        std::fs::write(src.join("lib.rs"), format!("{}\n", c.lib))?;

        let packaged = Command::new("cargo")
            .args([
                "package",
                "--quiet",
                "--no-verify",
                "--allow-dirty",
                "--offline",
            ])
            .current_dir(&dir)
            .output()
            .context("running cargo package")?;
        anyhow::ensure!(
            packaged.status.success(),
            "packaging {} {} failed: {}",
            c.name,
            c.version,
            String::from_utf8_lossy(&packaged.stderr).trim()
        );

        // `cargo package` puts the artefact under the package's own target directory.
        let built = dir
            .join("target")
            .join("package")
            .join(format!("{}-{}.crate", c.name, c.version));
        let dest = out.join(format!("{}-{}.crate", c.name, c.version));
        std::fs::copy(&built, &dest)
            .with_context(|| format!("copying {} to {}", built.display(), dest.display()))?;

        let cksum = sha256::try_digest(dest.as_path())
            .with_context(|| format!("hashing {}", dest.display()))?;
        let entry = out.join("index").join(index_path(c.name));
        std::fs::create_dir_all(entry.parent().expect("an index path has a parent"))?;
        // Appended, because one file holds every version of a crate, one per line.
        let mut lines = match std::fs::read_to_string(&entry) {
            Ok(existing) => existing,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(e) => return Err(e).with_context(|| format!("reading {}", entry.display())),
        };
        lines.push_str(&index_line(c, &cksum)?);
        lines.push('\n');
        std::fs::write(&entry, lines).with_context(|| format!("writing {}", entry.display()))?;
    }

    Ok(out.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    /// A wrong index path is not an error. It is a crate cargo silently cannot see, which would
    /// present as an unresolvable dependency in a leg and look like a `RESOLVE_FAIL` about the
    /// release.
    #[test]
    fn index_paths_follow_cargos_convention() {
        for (name, want) in [
            ("a", "1/a"),
            ("ab", "2/ab"),
            ("abc", "3/a/abc"),
            ("abcd", "ab/cd/abcd"),
            ("ct-types", "ct/-t/ct-types"),
            ("ct-schema", "ct/-s/ct-schema"),
            ("aws-smithy-types", "aw/s-/aws-smithy-types"),
        ] {
            assert_eq!(index_path(name), PathBuf::from(want), "for {name}");
        }
    }

    /// The property that keeps the test scenario from becoming a standing red leg.
    ///
    /// Every requirement in the family has to be satisfiable by exactly one version of the crate it
    /// names, so the committed registry resolves to a single major and the scenario is green on
    /// every release. Breaking it is then a deliberate edit, and this test is what fails to say so.
    #[test]
    fn the_committed_registry_resolves_to_one_major() {
        let mut majors: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        for c in TEST_REGISTRY {
            let major = c.version.split('.').take(2).collect::<Vec<_>>().join(".");
            majors
                .entry(c.name)
                .or_default()
                .push(Box::leak(major.into_boxed_str()));
        }
        for (name, mut found) in majors {
            found.sort();
            found.dedup();
            assert_eq!(
                found.len(),
                1,
                "{name} is published at {found:?} in the committed registry, so a consumer can \
                 resolve two of them and the test scenario is red on every release. That is only \
                 correct while a break is deliberately induced -- and then this test is the thing \
                 that has to be edited to say so."
            );
        }

        // And every requirement names a crate that exists at a compatible version.
        for c in TEST_REGISTRY {
            for (dep, req) in c.deps {
                let satisfied = TEST_REGISTRY.iter().any(|other| {
                    other.name == *dep && other.version.starts_with(req.trim_start_matches('^'))
                });
                assert!(
                    satisfied,
                    "{} {} requires {dep} {req}, which no version in the registry satisfies",
                    c.name, c.version
                );
            }
        }
    }

    /// The family must be self-contained, or the registry needs a crates.io proxy and the legs need
    /// a network — which is the complexity this whole approach exists to avoid.
    #[test]
    fn the_family_depends_on_nothing_outside_itself() {
        for c in TEST_REGISTRY {
            assert!(
                c.name.starts_with(PREFIX),
                "{} is not prefixed {PREFIX:?}, so a stray artefact would not be \
                 self-explanatory",
                c.name
            );
            for (dep, _) in c.deps {
                assert!(
                    TEST_REGISTRY.iter().any(|other| other.name == *dep),
                    "{} depends on {dep}, which is outside the family",
                    c.name
                );
            }
        }
    }

    /// Source replacement, not an alternative registry: the leg's manifest has to read exactly as a
    /// consumer's would, with a plain version requirement and no `registry = "..."` key.
    #[test]
    fn the_cargo_config_replaces_the_crates_io_source() {
        let config = cargo_config(Path::new("/tmp/reg"));
        assert!(config.contains("[source.crates-io]"), "{config}");
        assert!(
            config.contains("replace-with = \"canary-test-registry\""),
            "{config}"
        );
        assert!(config.contains("local-registry = \"/tmp/reg\""), "{config}");
        assert!(
            !config.contains("[registries"),
            "an alternative registry would force a `registry = ...` key into the manifest: {config}"
        );
    }

    #[test]
    fn an_index_line_is_the_shape_cargo_reads() {
        let c = TEST_REGISTRY
            .iter()
            .find(|c| c.name == "ct-client")
            .unwrap();
        let line = index_line(c, "deadbeef").unwrap();
        let v: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(v["name"], "ct-client");
        assert_eq!(v["cksum"], "deadbeef");
        assert_eq!(v["yanked"], false);
        let deps = v["deps"].as_array().unwrap();
        assert_eq!(deps.len(), 2);
        assert_eq!(deps[0]["name"], "ct-types");
        assert_eq!(deps[0]["kind"], "normal");
        assert_eq!(deps[0]["optional"], false);
    }
}
