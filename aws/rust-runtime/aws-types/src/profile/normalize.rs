/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use crate::profile::parse::{ProfileParseError, RawProfileSet};
use crate::profile::source::FileKind;
use crate::profile::{Profile, ProfileSet, Property};

/// Normalize a raw profile into a `MergedProfile`
///
/// This function follows the following rules, codified in the tests & the reference Java implementation
/// - When the profile is a config file, strip `profile` and trim whitespace (`profile foo` => `foo`)
/// - Profile names are validated (see `validate_profile_name`)
/// - A profile named `profile default` takes priority over a profile named `default`.
/// - Profiles with identical names are merged
pub fn normalize(
    mut base: ProfileSet,
    raw_profile: RawProfileSet,
    kind: FileKind,
) -> Result<ProfileSet, ProfileParseError> {
    let mut default_profile_has_profile_prefix = false;
    // for each profile in the raw set of profiles, normalize, then merge it into the base set
    for (profile_name, raw_profile) in raw_profile {
        let normalized_profile_name = match (profile_name, kind) {
            // credentials files don't perform any profile name normalization
            (any, FileKind::Credentials) => any,
            // If we already have seen a `[profile default]` then we should skip any profiles that are
            // named `default`
            ("default", FileKind::Config) if default_profile_has_profile_prefix => {
                tracing::warn!("profile `default` ignored because `[profile default]` was found which takes priority");
                continue;
            }
            ("default", FileKind::Config) => "default",
            (other, FileKind::Config) => match other.strip_prefix("profile").map(str::trim) {
                Some("default") => {
                    // "In the configuration file, [default] and [profile default] are not
                    // considered duplicate profiles. When both of these profiles are defined,
                    // the properties in [profile default] must be used, and the properties
                    // in [default] must be dropped."
                    if !default_profile_has_profile_prefix {
                        tracing::warn!("profile `default` ignored because `[profile default]` was found which takes priority");
                        base.profiles.remove("default");
                        default_profile_has_profile_prefix = true;
                    }
                    "default"
                }
                Some(name) => name,
                // In config files, profiles MUST start with `profile ` except for the default profile
                None => {
                    tracing::warn!(profile = %other, "profile ignored: in config files, profiles MUST start with `profile `");
                    continue;
                }
            },
        };
        let normalized_name = match validate_identifier(normalized_profile_name) {
            Some(name) => name,
            // When a profile name is invalid, ignore it
            None => {
                tracing::warn!(name = ?normalized_profile_name, "profile ignored because `{}` was not a valid identifier", normalized_profile_name);
                continue;
            }
        };
        let profile = base
            .profiles
            .entry(normalized_name.to_string())
            .or_insert_with(|| Profile::new(normalized_name.to_owned(), Default::default()));
        for (k, v) in raw_profile {
            match validate_identifier(&k) {
                Some(k) => {
                    profile
                        .properties
                        .insert(k.to_owned(), Property::new(k.to_owned(), v.into()));
                }
                None => {
                    tracing::warn!(profile = %normalized_name, key = ?k, "key ignored because `{}` was not a valid identifier", k);
                }
            }
        }
    }
    Ok(base)
}

/// Validate that a string is a valid identifier
///
/// Identifiers must match `[A-Za-z0-9\-_]+`
fn validate_identifier(input: &str) -> Option<&str> {
    input
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '\\')
        .then(|| input)
}

#[cfg(test)]
mod tests {
    use crate::profile::normalize::normalize;
    use crate::profile::parse::RawProfileSet;
    use crate::profile::source::FileKind;
    use crate::profile::ProfileSet;
    use std::collections::HashMap;
    use tracing_test::traced_test;

    #[test]
    #[traced_test]
    fn ignored_key_generates_warning() {
        let mut profile: RawProfileSet = HashMap::new();
        profile.insert("default", {
            let mut out = HashMap::new();
            out.insert("invalid key", "value".into());
            out
        });
        let _ = normalize(ProfileSet::empty(), profile, FileKind::Config);
        assert!(logs_contain(
            "key ignored because `invalid key` was not a valid identifier"
        ));
    }

    #[test]
    #[traced_test]
    fn invalid_profile_generates_warning() {
        let mut profile: RawProfileSet = HashMap::new();
        profile.insert("foo", HashMap::new());
        let _ = normalize(ProfileSet::empty(), profile, FileKind::Config);
        assert!(logs_contain("profile ignored"));
        assert!(logs_contain("profiles MUST start with `profile `"));
    }
}
