/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use serde::de::{SeqAccess, Visitor};
use serde::{Deserialize, Deserializer};
use std::fmt;
use wildmatch::WildMatch;

/// Struct reprepsentation of the TOML config files that specify which external types are allowed.
#[derive(Debug, Deserialize)]
pub struct Config {
    /// Whether or not to allow types from `alloc`. Defaults to true.
    #[serde(default = "default_allow_std")]
    pub allow_alloc: bool,

    /// Whether or not to allow types from `core`. Defaults to true.
    #[serde(default = "default_allow_std")]
    pub allow_core: bool,

    /// Whether or not to allow types from `std`. Defaults to true.
    #[serde(default = "default_allow_std")]
    pub allow_std: bool,

    /// List of globs for allowed external types
    ///
    /// For example, to allow every type in a crate:
    /// ```toml
    /// allowed_external_types = [
    ///     "crate_name::*"
    /// ]
    /// ```
    ///
    /// Or, to selectively allow just a module of that crate
    /// ```toml
    /// allowed_external_types = [
    ///     "crate_name::path::to_module::*"
    /// ]
    /// ```
    #[serde(deserialize_with = "deserialize_vec_wild_match")]
    pub allowed_external_types: Vec<WildMatch>,
}

impl Config {
    /// Returns true if the given `type_name` is allowed by this config for the given `root_crate_name`.
    pub fn allows_type(&self, root_crate_name: &str, type_name: &str) -> bool {
        let type_crate_name = &type_name[0..type_name.find("::").unwrap_or(type_name.len())];
        match type_crate_name {
            _ if type_crate_name == root_crate_name => true,
            "alloc" => self.allow_alloc,
            "core" => self.allow_core,
            "std" => self.allow_std,
            _ => self
                .allowed_external_types
                .iter()
                .any(|glob| glob.matches(type_name)),
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            allow_alloc: default_allow_std(),
            allow_core: default_allow_std(),
            allow_std: default_allow_std(),
            allowed_external_types: Default::default(),
        }
    }
}

const fn default_allow_std() -> bool {
    true
}

struct VecWildMatchDeserializer;

impl<'de> Visitor<'de> for VecWildMatchDeserializer {
    type Value = Vec<WildMatch>;

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("list of glob strings")
    }

    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let mut result = Vec::new();
        while let Some(value) = seq.next_element::<&str>()? {
            result.push(WildMatch::new(value));
        }
        Ok(result)
    }
}

fn deserialize_vec_wild_match<'de, D>(de: D) -> Result<Vec<WildMatch>, D::Error>
where
    D: Deserializer<'de>,
{
    de.deserialize_any(VecWildMatchDeserializer)
}

#[cfg(test)]
mod tests {
    use super::Config;
    use wildmatch::WildMatch;

    #[test]
    fn deserialize_config() {
        let config = r#"
            allow_std = false
            allowed_external_types = [
                "test::*",
                "another_test::something::*::something",
            ]
        "#;
        let config: Config = toml::from_str(config).unwrap();
        assert!(config.allow_alloc);
        assert!(config.allow_core);
        assert!(!config.allow_std);
        assert!(config.allowed_external_types[0].matches("test::something"));
        assert!(!config.allowed_external_types[0].matches("other::something"));
        assert!(config.allowed_external_types[1].matches("another_test::something::foo::something"));
        assert!(!config.allowed_external_types[1].matches("another_test::other::foo::something"));
    }

    #[test]
    fn test_allows_type() {
        let config = Config {
            allowed_external_types: vec![WildMatch::new("one::*"), WildMatch::new("two::*")],
            ..Default::default()
        };
        assert!(config.allows_type("root", "alloc::System"));
        assert!(config.allows_type("root", "std::vec::Vec"));
        assert!(config.allows_type("root", "std::path::Path"));

        assert!(config.allows_type("root", "root::thing"));
        assert!(config.allows_type("root", "one::thing"));
        assert!(config.allows_type("root", "two::thing"));
        assert!(!config.allows_type("root", "three::thing"));
    }
}
