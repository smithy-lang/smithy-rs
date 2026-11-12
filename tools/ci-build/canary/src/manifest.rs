/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Rendering a leg's crate: its `Cargo.toml` and its body.
//!
//! # Two invariants worth stating, because both fail silently
//!
//! **No `resolver = "3"`, and edition 2021.** Resolver 3 is MSRV-aware: on an old toolchain it
//! selects *older* dependency versions rather than failing. A `stable-4` leg would then resolve away
//! from the crates the release just published, build a stale graph, and report **green** — a false
//! negative in a canary whose entire purpose is the new graph. Edition 2021 defaults to resolver 2,
//! which is not MSRV-aware and fails loudly instead. Edition 2024 would default to resolver 3, so
//! the edition is pinned here and asserted by a test.
//!
//! **Test-only crates stay out of `[dependencies]`.** `cargo build` compiles `[dependencies]` and
//! the bin target only, which is what makes `COMPILE_FAIL` mean *a consumer's build is broken*. The
//! replay client, `http` and the credential types are reachable only from `tests/`, so a break in
//! them yields `HARNESS_FAIL` — a real signal, but not one that wakes someone claiming customer
//! impact. Collapsing the two lists would route 29 `test-util` crates, including ones we publish
//! ourselves, into `COMPILE_FAIL`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::Context;

use crate::scenarios::{Body, Leg, Scenario};

/// Edition 2021, so the resolver is 2. See the module docs.
const EDITION: &str = "2021";

/// Extra `[dependencies]` the `canary-build-check` body needs beyond the scenario's own.
///
/// `tokio` for `#[tokio::main]` and `#[tokio::test]` — which need `macros` and `rt-multi-thread`,
/// so the features are not optional. `aws-smithy-types` because `main.rs` touches `Document`'s
/// object variant directly, the exact surface that changed shape in the yanked
/// `aws-smithy-types` 1.7.0.
const APP_DEPS: &[(&str, &str, &[&str])] = &[
    ("tokio", "1", &["macros", "rt-multi-thread"]),
    ("aws-smithy-types", "1", &[]),
];

/// `[dev-dependencies]` for the assertions. Never in `[dependencies]`.
///
/// `tokio` is absent here deliberately: it is a real dependency of the body, so it belongs in
/// `[dependencies]` with its features, and listing it twice would only invite the two declarations
/// to drift.
const APP_DEV_DEPS: &[(&str, &str, &[&str])] = &[
    ("http", "1", &[]),
    ("aws-smithy-http-client", "1", &["test-util"]),
    ("aws-credential-types", "1", &[]),
];

/// The body sources, compiled into this binary so a leg needs no path resolution at runtime.
mod source {
    pub const MAIN: &str = include_str!("../app/main.rs");
    pub const HERMETIC: &str = include_str!("../app/hermetic.rs");
    pub const PLATFORM_TLS: &str = include_str!("../app/platform_tls.rs");
    pub const STUB_MAIN: &str = include_str!("../app/stub_main.rs");
    pub const STUB_LIB: &str = include_str!("../app/stub_lib.rs");
}

/// A leg's crate, ready to write to disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rendered {
    pub cargo_toml: String,
    /// Relative path to contents. `src/main.rs`, `tests/hermetic.rs`, …
    pub files: BTreeMap<PathBuf, &'static str>,
    /// `.cargo/config.toml`, present only for a leg that resolves from the on-disk test registry.
    ///
    /// Not in `files` because that holds `&'static str` bodies and this one carries a path computed
    /// at run time — and because it is configuration rather than a body, which is the distinction a
    /// reader cares about.
    pub cargo_config: Option<String>,
}

/// Render the crate for one leg.
///
/// `registry` must be `Some` for a manifest that resolves from the on-disk test registry, and is
/// ignored otherwise. Getting that wrong is not a cosmetic mistake: a test-registry leg rendered
/// without the `.cargo/config.toml` would look for `ct-client` on crates.io, where it does not
/// exist, and report `RESOLVE_FAIL` about a release that is perfectly fine. So it is an error
/// rather than a default, and [`render`] refuses rather than guessing.
pub fn render_with_registry(
    leg: &Leg,
    scenario: &Scenario,
    registry: Option<&Path>,
) -> anyhow::Result<Rendered> {
    let needs = crate::scenarios::uses_test_registry(scenario);
    let mut rendered = render(leg, scenario);
    match (needs, registry) {
        (true, Some(path)) => {
            rendered.cargo_config = Some(crate::test_registry::cargo_config(path));
        }
        (true, None) => anyhow::bail!(
            "{} resolves from the test registry, so it needs one; without it the leg would look \
             for its crates on crates.io and report RESOLVE_FAIL about a healthy release",
            scenario.id
        ),
        (false, _) => {}
    }
    Ok(rendered)
}

/// Render the crate for one leg, without a test registry.
///
/// Prefer [`render_with_registry`] at any call site that could be handed a test-registry manifest.
pub fn render(leg: &Leg, scenario: &Scenario) -> Rendered {
    let mut deps: Vec<(String, String, Vec<&str>)> = scenario
        .deps
        .iter()
        .map(|(c, r)| ((*c).to_string(), (*r).to_string(), vec![]))
        .collect();

    let mut files = BTreeMap::new();

    match leg.body {
        Body::CanaryApp => {
            for (c, r, feats) in APP_DEPS {
                // The scenario's own declaration wins: a prior-line scenario pins `aws-config` to
                // `~1.12` and the body must not quietly widen it.
                if !deps.iter().any(|(name, _, _)| name == c) {
                    deps.push(((*c).to_string(), (*r).to_string(), feats.to_vec()));
                }
            }
            files.insert(PathBuf::from("src/main.rs"), source::MAIN);
            files.insert(PathBuf::from("tests/hermetic.rs"), source::HERMETIC);
            // Compiles, so the platform cert-store path stays in the `cargo test --no-run` surface.
            // Does not run: it is `#[ignore]`d because it needs network egress, which would let a
            // transient TLS failure masquerade as a RUNTIME_FAIL.
            files.insert(PathBuf::from("tests/platform_tls.rs"), source::PLATFORM_TLS);
        }
        Body::Stub => {
            files.insert(PathBuf::from("src/main.rs"), source::STUB_MAIN);
        }
        Body::StubLib => {
            files.insert(PathBuf::from("src/lib.rs"), source::STUB_LIB);
        }
    }

    let dev_deps: &[(&str, &str, &[&str])] = match leg.body {
        Body::CanaryApp => APP_DEV_DEPS,
        _ => &[],
    };

    Rendered {
        cargo_toml: cargo_toml(leg, &deps, dev_deps),
        files,
        cargo_config: None,
    }
}

fn cargo_toml(
    leg: &Leg,
    deps: &[(String, String, Vec<&str>)],
    dev_deps: &[(&str, &str, &[&str])],
) -> String {
    let mut s = String::new();
    s.push_str("# Generated by tools/ci-build/canary. Do not edit.\n");
    s.push_str(&format!("# leg: {}\n", leg.id));
    s.push_str("#\n");
    s.push_str(
        "# edition 2021 pins the dependency resolver to v2. Resolver v3 is MSRV-aware and\n",
    );
    s.push_str(
        "# would silently resolve older crates on an old toolchain, turning a break into a\n",
    );
    s.push_str("# false green. Do not change the edition without reading manifest.rs.\n");
    s.push_str("[package]\n");
    s.push_str("name = \"canary-build-check\"\n");
    s.push_str("version = \"0.1.0\"\n");
    s.push_str(&format!("edition = \"{EDITION}\"\n"));
    s.push_str("publish = false\n");
    // Its own workspace, so the leg never joins an enclosing one.
    s.push_str("\n[workspace]\n");

    s.push_str("\n[dependencies]\n");
    for (name, req, feats) in deps {
        s.push_str(&dep_line(name, req, feats));
    }

    if !dev_deps.is_empty() {
        s.push_str(
            "\n# Reachable only from tests/. A break here is HARNESS_FAIL, not COMPILE_FAIL.\n",
        );
        s.push_str("[dev-dependencies]\n");
        for (name, req, feats) in dev_deps {
            s.push_str(&dep_line(name, req, feats));
        }
    }
    s
}

fn dep_line(name: &str, req: &str, features: &[&str]) -> String {
    if features.is_empty() {
        format!("{name} = \"{req}\"\n")
    } else {
        let feats = features
            .iter()
            .map(|f| format!("\"{f}\""))
            .collect::<Vec<_>>()
            .join(", ");
        format!("{name} = {{ version = \"{req}\", features = [{feats}] }}\n")
    }
}

/// Write a rendered leg into `dir`, creating it if needed.
pub fn materialize(rendered: &Rendered, dir: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    std::fs::write(dir.join("Cargo.toml"), &rendered.cargo_toml)
        .with_context(|| format!("writing {}/Cargo.toml", dir.display()))?;
    if let Some(config) = &rendered.cargo_config {
        let cargo_dir = dir.join(".cargo");
        std::fs::create_dir_all(&cargo_dir)
            .with_context(|| format!("creating {}", cargo_dir.display()))?;
        std::fs::write(cargo_dir.join("config.toml"), config)
            .with_context(|| format!("writing {}/.cargo/config.toml", dir.display()))?;
    }
    for (rel, contents) in &rendered.files {
        let path = dir.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating {}", parent.display()))?;
        }
        std::fs::write(&path, contents).with_context(|| format!("writing {}", path.display()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenarios::{self, Checks};
    use crate::toolchain::{self, Version};

    fn leg_named(id: &str) -> Leg {
        let tcs = toolchain::resolve(Version::new(1, 99, 1), Version::new(1, 94, 1))
            .unwrap()
            .kept;
        scenarios::legs(&tcs)
            .into_iter()
            .find(|l| l.id == id)
            .unwrap_or_else(|| panic!("no leg {id}"))
    }

    fn render_named(id: &str) -> Rendered {
        let leg = leg_named(id);
        let scenario = scenarios::manifest(&leg.manifest).unwrap();
        render(&leg, scenario)
    }

    /// The invariant with the worst failure mode: resolver 3 would turn an old-toolchain break into
    /// a false green.
    #[test]
    fn no_rendered_manifest_opts_into_resolver_three() {
        let tcs = toolchain::resolve(Version::new(1, 99, 1), Version::new(1, 94, 1))
            .unwrap()
            .kept;
        for leg in scenarios::legs(&tcs) {
            let r = render(&leg, scenarios::manifest(&leg.manifest).unwrap());
            // Parse rather than grep: the rendered manifest *explains* the resolver choice in a
            // comment, so a textual search for the word matches our own documentation.
            let parsed: toml::Value = toml::from_str(&r.cargo_toml).unwrap();
            assert!(
                parsed["package"].get("resolver").is_none()
                    && parsed["workspace"].get("resolver").is_none(),
                "{} declares a resolver key",
                leg.id
            );
            assert_eq!(
                parsed["package"]["edition"].as_str(),
                Some(EDITION),
                "{} is not on edition {EDITION}, which would change the default resolver",
                leg.id
            );
        }
    }

    /// `cargo build` must not be able to reach a test-only crate, or `COMPILE_FAIL` stops meaning
    /// "a consumer's build is broken".
    #[test]
    fn test_only_crates_never_appear_in_dependencies() {
        let r = render_named("new-project--stable");
        let (deps, dev) = r.cargo_toml.split_once("[dev-dependencies]").unwrap();
        for crate_name in ["aws-smithy-http-client", "http", "aws-credential-types"] {
            assert!(
                !deps.contains(&format!("\n{crate_name} =")),
                "{crate_name} must not be in [dependencies]"
            );
            assert!(dev.contains(crate_name), "{crate_name} should be a dev-dep");
        }
        assert!(
            dev.contains("test-util"),
            "the replay client needs test-util"
        );
    }

    /// Rendering a test-registry leg without a registry must be an error, not a default.
    ///
    /// The leg would otherwise look for `ct-client` on crates.io, where it does not exist, and
    /// report `RESOLVE_FAIL` — a page claiming consumers cannot resolve the release, caused by a
    /// missing four-line config file.
    #[test]
    fn a_test_registry_leg_refuses_to_render_without_one() {
        let scenario = scenarios::manifest("canary-test-registry").expect("the test scenario");
        let leg = Leg {
            manifest: scenario.id.to_string(),
            ..leg_named("new-project--stable")
        };

        let err = render_with_registry(&leg, scenario, None)
            .expect_err("rendering without a registry must fail");
        assert!(err.to_string().contains("RESOLVE_FAIL"), "{err}");

        let ok = render_with_registry(&leg, scenario, Some(Path::new("/tmp/reg")))
            .expect("with a registry it renders");
        let config = ok.cargo_config.expect("a cargo config");
        assert!(config.contains("local-registry = \"/tmp/reg\""), "{config}");
    }

    /// And an ordinary leg gets no cargo config at all, so nothing redirects the one thing the
    /// canary exists to measure: what a consumer resolves from the real registry.
    #[test]
    fn an_ordinary_leg_gets_no_cargo_config() {
        let leg = leg_named("new-project--stable");
        let scenario = scenarios::manifest("new-project").unwrap();
        for registry in [None, Some(Path::new("/tmp/reg"))] {
            let r = render_with_registry(&leg, scenario, registry).unwrap();
            assert!(
                r.cargo_config.is_none(),
                "a crates.io leg must never be redirected, even when a registry is available"
            );
        }
    }

    /// A scenario that constrains `aws-config` deliberately must win over the body's own
    /// requirement, or it stops testing the thing it exists for.
    ///
    /// The subject is built here rather than taken from the table. It used to be
    /// `prior-aws-config-1-12`, which is now withdrawn for being red — and the guard matters more
    /// than any one row that happens to exercise it: the three withdrawn rows are *meant* to come
    /// back, and the next narrowing scenario someone writes needs this to already hold. A
    /// synthetic subject also means the live table no longer has to contain a narrowing row for
    /// the property to be checked at all.
    #[test]
    fn the_scenarios_requirement_wins_over_the_bodys() {
        let narrowing = Scenario {
            id: "a-pinned-consumer",
            deps: &[("aws-config", "~1.12"), ("aws-sdk-s3", "1")],
            body: Body::CanaryApp,
            reaches: "a consumer who constrains aws-config more narrowly than the body does",
            induced: None,
        };
        let leg = Leg {
            manifest: narrowing.id.to_string(),
            ..leg_named("new-project--stable")
        };

        let r = render(&leg, &narrowing);
        assert!(
            r.cargo_toml.contains("aws-config = \"~1.12\""),
            "{}",
            r.cargo_toml
        );
        assert_eq!(
            r.cargo_toml.matches("aws-config =").count(),
            1,
            "aws-config must be declared exactly once"
        );
    }

    #[test]
    fn the_app_body_ships_main_and_both_test_files() {
        let r = render_named("new-project--stable");
        let files: Vec<_> = r.files.keys().map(|p| p.to_str().unwrap()).collect();
        assert_eq!(
            files,
            ["src/main.rs", "tests/hermetic.rs", "tests/platform_tls.rs"]
        );
        assert!(r.files[Path::new("src/main.rs")].contains("Document::Object"));
    }

    /// platform_tls compiles but must never run: it needs network egress, and a transient TLS
    /// failure would be classified RUNTIME_FAIL — a hard verdict blaming the graph for a blip.
    /// The one test that needs network egress must stay `#[ignore]`d, or a transient TLS failure
    /// becomes a `RUNTIME_FAIL` — a hard verdict blaming the published graph for a blip.
    #[test]
    fn the_network_touching_test_is_ignore_gated() {
        let r = render_named("new-project--stable");
        assert!(
            r.files[Path::new("tests/platform_tls.rs")].contains("#[ignore"),
            "platform_tls.rs must stay #[ignore]d"
        );
    }

    /// The assertions that *do* run must stay hermetic.
    ///
    /// Note what is deliberately not asserted here: `hermetic.rs` is full of `amazonaws.com` URIs,
    /// and that is correct rather than a leak — they are the request URIs handed to
    /// `StaticReplayClient`, which matches them and returns a canned response without opening a
    /// socket. Stating the real endpoint is how a replay fixture says which request it expects. The
    /// checkable property is that every socket it opens is loopback.
    #[test]
    fn the_running_assertions_only_bind_loopback() {
        let hermetic = render_named("new-project--stable").files[Path::new("tests/hermetic.rs")];
        assert!(
            hermetic.contains("StaticReplayClient"),
            "the assertions should be replay-driven"
        );
        let binds = hermetic.matches("TcpListener::bind(").count();
        let loopback = hermetic
            .matches("TcpListener::bind(\"127.0.0.1:0\")")
            .count();
        assert!(binds > 0, "expected some loopback servers");
        assert_eq!(
            binds, loopback,
            "every listener must bind 127.0.0.1 on a kernel-assigned port"
        );
    }

    #[test]
    fn a_stub_leg_is_a_bare_main_with_no_dev_dependencies() {
        let r = render_named("aws-esdk--stable");
        assert_eq!(
            r.files
                .keys()
                .map(|p| p.to_str().unwrap())
                .collect::<Vec<_>>(),
            ["src/main.rs"]
        );
        // Byte-equality would break on the license header, so assert the property it stood for:
        // once comments are stripped there is nothing in the body but an empty `main`.
        let code = r.files[Path::new("src/main.rs")]
            .lines()
            .map(str::trim)
            .filter(|l| {
                !l.is_empty() && !l.starts_with("//") && !l.starts_with("/*") && !l.starts_with('*')
            })
            .collect::<Vec<_>>();
        assert_eq!(code, ["fn main() {}"]);
        assert!(!r.cargo_toml.contains("[dev-dependencies]"));
        assert!(r.cargo_toml.contains("aws-esdk = \"1\""));
    }

    /// The cross-compiled legs are lib-only so that producing an rlib needs no system linker — a
    /// `bin` target for i686 fails on `cannot find Scrt1.o` without gcc-multilib.
    #[test]
    fn a_cross_compiled_leg_is_lib_only() {
        for id in ["i686", "powerpc", "powerpc64"] {
            let r = render_named(id);
            assert_eq!(
                r.files
                    .keys()
                    .map(|p| p.to_str().unwrap())
                    .collect::<Vec<_>>(),
                ["src/lib.rs"],
                "{id} must have no bin target"
            );
            assert_eq!(leg_named(id).checks, Checks::BuildOnly);
            assert!(r.cargo_toml.contains("aws-smithy-types = \"1\""));
            assert!(
                !r.cargo_toml.contains("aws-config"),
                "{id} must not reach the client path"
            );
        }
    }

    #[test]
    fn every_rendered_manifest_parses_and_is_self_contained() {
        let tcs = toolchain::resolve(Version::new(1, 99, 1), Version::new(1, 94, 1))
            .unwrap()
            .kept;
        for leg in scenarios::legs(&tcs) {
            let r = render(&leg, scenarios::manifest(&leg.manifest).unwrap());
            let parsed: toml::Value = toml::from_str(&r.cargo_toml)
                .unwrap_or_else(|e| panic!("{} rendered invalid TOML: {e}", leg.id));
            assert!(
                parsed.get("workspace").is_some(),
                "{} must declare its own workspace",
                leg.id
            );
            assert_eq!(
                parsed["package"]["publish"].as_bool(),
                Some(false),
                "{} must not be publishable",
                leg.id
            );
            assert!(
                !parsed["dependencies"].as_table().unwrap().is_empty(),
                "{} has no dependencies to test",
                leg.id
            );
        }
    }

    #[test]
    fn materialize_writes_the_tree() {
        let dir = std::env::temp_dir().join(format!("canary-render-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let r = render_named("new-project--stable");
        materialize(&r, &dir).unwrap();
        assert!(dir.join("Cargo.toml").is_file());
        assert!(dir.join("src/main.rs").is_file());
        assert!(dir.join("tests/hermetic.rs").is_file());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
