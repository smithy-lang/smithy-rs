/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use anyhow::bail;
use lazy_static::lazy_static;
use regex::Regex;
use semver::Version;
use std::cmp::Ordering;
use std::fmt;
use std::str::FromStr;

lazy_static! {
    static ref VERSION_TAG: Regex = Regex::new(r"^v(\d+)\.(\d+)\.(\d+)$").unwrap();
    static ref DATE_TAG: Regex = Regex::new(r"^release-(\d{4}-\d{2}-\d{2})$").unwrap();
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VersionReleaseTag {
    version: Version,
    original: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DateReleaseTag {
    date: String,
    original: String,
}

/// Represents a GitHub release tag used in aws-sdk-rust or smithy-rs.
///
/// Release tags can be compared with each other to see which is older/newer.
/// Date-based tags are always considered newer than version-based tags.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ReleaseTag {
    /// Version based tag (only used in older versions)
    Version(VersionReleaseTag),
    /// Date based tag
    Date(DateReleaseTag),
}

impl ReleaseTag {
    /// Returns the string representation of the tag
    pub fn as_str(&self) -> &str {
        match self {
            ReleaseTag::Version(v) => &v.original,
            ReleaseTag::Date(d) => &d.original,
        }
    }
}

impl FromStr for ReleaseTag {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if let Some(caps) = VERSION_TAG.captures(value) {
            Ok(ReleaseTag::Version(VersionReleaseTag {
                version: Version::new(
                    caps.get(1).expect("validated by regex").as_str().parse()?,
                    caps.get(2).expect("validated by regex").as_str().parse()?,
                    caps.get(3).expect("validated by regex").as_str().parse()?,
                ),
                original: value.into(),
            }))
        } else if let Some(caps) = DATE_TAG.captures(value) {
            Ok(ReleaseTag::Date(DateReleaseTag {
                date: caps.get(1).expect("validated by regex").as_str().into(),
                original: value.into(),
            }))
        } else {
            bail!("Tag `{value}` doesn't match a known format")
        }
    }
}

impl fmt::Display for ReleaseTag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Version(v) => write!(f, "{}", v.original),
            Self::Date(d) => write!(f, "{}", d.original),
        }
    }
}

impl Ord for ReleaseTag {
    fn cmp(&self, other: &Self) -> Ordering {
        self.partial_cmp(other)
            .expect("Tag::partial_cmp never returns None")
    }
}

impl PartialOrd for ReleaseTag {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        match (self, other) {
            (ReleaseTag::Date(_), ReleaseTag::Version(_)) => Some(Ordering::Greater),
            (ReleaseTag::Version(_), ReleaseTag::Date(_)) => Some(Ordering::Less),
            (ReleaseTag::Date(lhs), ReleaseTag::Date(rhs)) => Some(lhs.date.cmp(&rhs.date)),
            (ReleaseTag::Version(lhs), ReleaseTag::Version(rhs)) => {
                Some(lhs.version.cmp(&rhs.version))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tag(value: &str) -> ReleaseTag {
        ReleaseTag::from_str(value).unwrap()
    }

    #[test]
    fn test_parse() {
        assert_eq!(
            ReleaseTag::Version(VersionReleaseTag {
                version: Version::new(0, 4, 1),
                original: "v0.4.1".into(),
            }),
            tag("v0.4.1")
        );

        assert_eq!(
            ReleaseTag::Date(DateReleaseTag {
                date: "2022-07-26".into(),
                original: "release-2022-07-26".into(),
            }),
            tag("release-2022-07-26")
        );

        assert!(ReleaseTag::from_str("foo").is_err());
    }

    #[test]
    fn test_comparison() {
        assert!(tag("v0.4.2") < tag("v0.4.10"));
        assert!(tag("v0.4.1") < tag("v0.5.0"));
        assert!(tag("v0.4.1") < tag("release-2022-07-26"));
        assert!(tag("release-2022-07-20") < tag("release-2022-07-26"));
        assert!(tag("release-2022-06-20") < tag("release-2022-07-01"));
        assert!(tag("release-2021-06-20") < tag("release-2022-06-20"));
    }

    #[test]
    fn test_display() {
        assert_eq!("v0.4.2", format!("{}", tag("v0.4.2")));
        assert_eq!(
            "release-2022-07-26",
            format!("{}", tag("release-2022-07-26"))
        );
    }
}
