/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Deriving the `stable-N` toolchain span, and clamping it at the MSRV the crates under test
//! declare.
//!
//! The clamp is not a convenience. A compiler below the declared `rust-version` is not a supported
//! configuration, so a failure there says nothing about the published graph — it is a `rust-version`
//! error dressed up as a break. Measured 2026-11: `stable` is 1.99.1 and the crates under test
//! declare 1.94.1, so `stable-4` is 1.95 and the span survives with exactly one release of margin.
//! The next MSRV bump removes that margin, which is why [`clamp`] reports what it dropped rather
//! than silently shrinking coverage.

use std::fmt;
use std::str::FromStr;

/// How many releases below `stable` the span reaches. `stable`, `stable-1` … `stable-4`.
pub const SPAN: u32 = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
}

impl Version {
    pub const fn new(major: u64, minor: u64, patch: u64) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

impl FromStr for Version {
    type Err = anyhow::Error;

    /// Accepts `1.94.1` and `1.94`, since `rust-version` may omit the patch.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut parts = s.trim().split('.');
        let mut next = |what: &str| -> anyhow::Result<u64> {
            parts
                .next()
                .ok_or_else(|| anyhow::anyhow!("version {s:?} has no {what}"))?
                .parse()
                .map_err(|e| anyhow::anyhow!("version {s:?} has a non-numeric {what}: {e}"))
        };
        let major = next("major")?;
        let minor = next("minor")?;
        let patch = match parts.next() {
            Some(p) => p
                .parse()
                .map_err(|e| anyhow::anyhow!("version {s:?} has a non-numeric patch: {e}"))?,
            None => 0,
        };
        if parts.next().is_some() {
            anyhow::bail!("version {s:?} has more than three components");
        }
        Ok(Self::new(major, minor, patch))
    }
}

/// A toolchain the canary asks `rustup` to install.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Toolchain {
    /// `stable`, `stable-1`, … — the leg's stable name, used as the matrix key.
    pub name: String,
    /// How many releases below `stable`.
    pub offset: u32,
    /// The concrete version `rustup` is given, so the leg pins rather than drifting mid-window.
    pub version: Version,
}

/// The toolchains of the span, newest first, before any clamp.
///
/// Rust ships minor releases without skipping, so `stable-N` is `stable` with the minor decremented
/// by `N`. The patch drops to zero for `N >= 1` only: the newest patch of an older minor is not
/// derivable, whereas `stable`'s is, because the caller read it off `rustc +stable --version`.
/// Zeroing it there too would point the leg named `stable` at an older compiler than the one CI and
/// consumers actually use, and would hide a regression shipped in a patch release.
pub fn span(stable: Version, span: u32) -> Vec<Toolchain> {
    (0..=span)
        .map_while(|offset| {
            let minor = stable.minor.checked_sub(offset as u64)?;
            Some(Toolchain {
                name: if offset == 0 {
                    "stable".to_string()
                } else {
                    format!("stable-{offset}")
                },
                offset,
                version: if offset == 0 {
                    stable
                } else {
                    Version::new(stable.major, minor, 0)
                },
            })
        })
        .collect()
}

/// What [`clamp`] decided, including what it dropped so the caller can report it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Clamped {
    /// Toolchains at or above the MSRV, newest first. Never empty: see [`clamp`].
    pub kept: Vec<Toolchain>,
    /// Toolchains below the MSRV, which would have failed with a `rust-version` error.
    pub dropped: Vec<Toolchain>,
    /// The MSRV the clamp was taken against.
    pub msrv: Version,
}

impl Clamped {
    /// A one-line summary for the job log. Coverage shrinking silently is the failure this guards.
    pub fn report(&self) -> String {
        let kept: Vec<_> = self.kept.iter().map(|t| t.name.as_str()).collect();
        if self.dropped.is_empty() {
            format!(
                "toolchains: {} — all {} survive the MSRV clamp at {}",
                kept.join(", "),
                self.kept.len(),
                self.msrv
            )
        } else {
            let dropped: Vec<_> = self
                .dropped
                .iter()
                .map(|t| format!("{} ({})", t.name, t.version))
                .collect();
            format!(
                "toolchains: {} — {} of {} survive the MSRV clamp at {}; DROPPED {} \
                 (below the declared rust-version, so a failure there is not a break)",
                kept.join(", "),
                self.kept.len(),
                self.kept.len() + self.dropped.len(),
                self.msrv,
                dropped.join(", "),
            )
        }
    }
}

/// Drop any toolchain below `msrv`.
///
/// Errors rather than returning an empty set when even `stable` is below the MSRV: that means the
/// crates under test declare a `rust-version` newer than the released compiler, which is a defect in
/// the release rather than something for the canary to paper over.
pub fn clamp(toolchains: Vec<Toolchain>, msrv: Version) -> anyhow::Result<Clamped> {
    let (kept, dropped): (Vec<_>, Vec<_>) = toolchains
        .into_iter()
        .partition(|t| t.version >= msrv_floor(msrv));
    if kept.is_empty() {
        anyhow::bail!(
            "every toolchain in the span is below the declared MSRV {msrv}; \
             the crates under test require a compiler newer than stable"
        );
    }
    Ok(Clamped {
        kept,
        dropped,
        msrv,
    })
}

/// `rust-version = "1.94.1"` admits 1.94.1 but not 1.94.0, and `span` produces `x.y.0`, so compare
/// against the MSRV as given rather than rounding it down to the minor.
fn msrv_floor(msrv: Version) -> Version {
    msrv
}

/// The span for `stable`, clamped at `msrv`.
pub fn resolve(stable: Version, msrv: Version) -> anyhow::Result<Clamped> {
    clamp(span(stable, SPAN), msrv)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(major: u64, minor: u64, patch: u64) -> Version {
        Version::new(major, minor, patch)
    }

    #[test]
    fn parses_two_and_three_component_versions() {
        assert_eq!("1.94.1".parse::<Version>().unwrap(), v(1, 94, 1));
        assert_eq!("1.94".parse::<Version>().unwrap(), v(1, 94, 0));
        assert!("1".parse::<Version>().is_err());
        assert!("1.94.1.2".parse::<Version>().is_err());
        assert!("1.x.1".parse::<Version>().is_err());
    }

    #[test]
    fn span_decrements_the_minor_and_keeps_the_patch_only_for_stable() {
        let got = span(v(1, 99, 1), SPAN);
        let names: Vec<_> = got.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(
            names,
            ["stable", "stable-1", "stable-2", "stable-3", "stable-4"]
        );
        let versions: Vec<_> = got.iter().map(|t| t.version).collect();
        assert_eq!(
            versions,
            [
                v(1, 99, 1),
                v(1, 98, 0),
                v(1, 97, 0),
                v(1, 96, 0),
                v(1, 95, 0)
            ],
            "the stable leg must build with the stable the runner reported, patch included"
        );
    }

    /// The leg named `stable` is the one toolchain whose exact version is known, so it must not be
    /// rounded down. A regression shipped in 1.99.1 would be invisible to a leg pinned at 1.99.0.
    #[test]
    fn the_stable_leg_pins_the_exact_reported_patch() {
        for patch in [0, 1, 7] {
            let got = span(v(1, 99, patch), SPAN);
            assert_eq!(got[0].name, "stable");
            assert_eq!(got[0].version, v(1, 99, patch));
        }
    }

    /// The state measured on 2026-11: stable 1.99.1 against a declared 1.94.1. All five survive,
    /// with `stable-4` at 1.95 one release above the floor.
    #[test]
    fn today_all_five_survive_with_one_release_of_margin() {
        let c = resolve(v(1, 99, 1), v(1, 94, 1)).unwrap();
        assert_eq!(c.kept.len(), 5);
        assert!(c.dropped.is_empty());
        assert_eq!(c.kept.last().unwrap().version, v(1, 95, 0));
        assert!(c.report().contains("all 5 survive"));
    }

    /// One more MSRV bump and the margin is gone. The point of the test is that the leg count falls
    /// and the report says so, rather than ten legs turning red on a `rust-version` error.
    #[test]
    fn an_msrv_bump_drops_toolchains_and_says_which() {
        let c = resolve(v(1, 99, 1), v(1, 96, 0)).unwrap();
        assert_eq!(
            c.kept.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(),
            ["stable", "stable-1", "stable-2", "stable-3"]
        );
        assert_eq!(
            c.dropped
                .iter()
                .map(|t| t.name.as_str())
                .collect::<Vec<_>>(),
            ["stable-4"]
        );
        let report = c.report();
        assert!(report.contains("4 of 5 survive"), "{report}");
        assert!(report.contains("DROPPED stable-4 (1.95.0)"), "{report}");
    }

    /// 1.94.0 is below `rust-version = "1.94.1"`, so the floor is the MSRV as written and not its
    /// minor. This is the boundary that decides whether the span can be five or must be four.
    #[test]
    fn the_floor_is_the_msrv_as_written_not_its_minor() {
        let c = clamp(span(v(1, 99, 1), 5), v(1, 94, 1)).unwrap();
        assert_eq!(
            c.dropped.iter().map(|t| t.version).collect::<Vec<_>>(),
            [v(1, 94, 0)],
            "stable-5 is 1.94.0, which rust-version 1.94.1 does not admit"
        );
    }

    #[test]
    fn only_stable_survives_when_msrv_is_current() {
        let c = resolve(v(1, 99, 1), v(1, 99, 0)).unwrap();
        assert_eq!(c.kept.len(), 1);
        assert_eq!(c.dropped.len(), 4);
    }

    #[test]
    fn an_msrv_above_stable_is_an_error_not_an_empty_matrix() {
        let err = resolve(v(1, 99, 1), v(1, 100, 0)).unwrap_err();
        assert!(err.to_string().contains("newer than stable"), "{err}");
    }

    #[test]
    fn the_span_stops_at_the_major_boundary_rather_than_underflowing() {
        let got = span(v(1, 2, 0), SPAN);
        assert_eq!(got.len(), 3, "1.2, 1.1, 1.0 and then stop");
    }
}
