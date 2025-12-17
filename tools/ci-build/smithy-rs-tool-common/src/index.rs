/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::retry::{run_with_retry_sync, ErrorClass};
use anyhow::{anyhow, Context, Error, Result};
use crates_index::Crate;
use reqwest::StatusCode;
use semver::Version;
use std::{collections::HashMap, time::Duration};
use std::{fs, path::Path};

pub struct CratesIndex(Inner);

enum Inner {
    Fake(FakeIndex),
    Real(crates_index::SparseIndex),
}

impl CratesIndex {
    /// Returns a real sparse crates.io index.
    pub fn real() -> Result<Self> {
        Ok(Self(Inner::Real(
            crates_index::SparseIndex::new_cargo_default()
                .context("failed to initialize the sparse crates.io index")?,
        )))
    }

    /// Returns a fake crates.io index from file, panicking if loading fails.
    pub fn fake(path: impl AsRef<Path>) -> Self {
        Self(Inner::Fake(FakeIndex::from_file(path)))
    }

    /// Returns a fake crates.io index from a hashmap
    pub fn fake_from_map(versions: HashMap<String, Vec<String>>) -> Self {
        Self(Inner::Fake(FakeIndex { crates: versions }))
    }

    /// Retrieves the published versions for the given crate name.
    pub fn published_versions(&self, crate_name: &str) -> Result<Vec<String>> {
        match &self.0 {
            Inner::Fake(index) => Ok(index.crates.get(crate_name).cloned().unwrap_or_default()),
            Inner::Real(index) => Ok(run_with_retry_sync(
                "retrieve published versions",
                3,
                Duration::from_secs(1),
                || published_versions(index, crate_name),
                |_err| ErrorClass::Retry,
            )?),
        }
    }
}

pub fn is_published(index: &CratesIndex, crate_name: &str, version: &Version) -> Result<bool> {
    let crate_name = crate_name.to_string();
    let versions = index.published_versions(&crate_name)?;
    Ok(versions.contains(&version.to_string()))
}

fn published_versions(index: &crates_index::SparseIndex, crate_name: &str) -> Result<Vec<String>> {
    let url = index
        .crate_url(crate_name)
        .expect("crate name is not empty string");
    let crate_meta: Option<Crate> = reqwest::blocking::get(url)
        .map_err(Error::from)
        .and_then(|response| {
            let status = response.status();
            response.bytes().map(|b| (status, b)).map_err(Error::from)
        })
        .and_then(|(status, bytes)| match status {
            status if status.is_success() => {
                Crate::from_slice(&bytes).map_err(Error::from).map(Some)
            }
            StatusCode::NOT_FOUND => Ok(None),
            status => {
                let body = String::from_utf8_lossy(&bytes);
                Err(anyhow!(
                    "request to crates.io index failed ({status}):\n{body}"
                ))
            }
        })
        .with_context(|| format!("failed to retrieve crates.io metadata for {crate_name}"))?;
    Ok(crate_meta
        .map(|meta| {
            meta.versions()
                .iter()
                .map(|v| v.version().to_string())
                .collect()
        })
        .unwrap_or_default())
}

/// Fake crates.io index for testing
pub struct FakeIndex {
    crates: HashMap<String, Vec<String>>,
}

impl FakeIndex {
    fn from_file(path: impl AsRef<Path>) -> FakeIndex {
        let bytes = fs::read(path.as_ref()).unwrap();
        let toml: toml::Value = toml::from_slice(&bytes).unwrap();
        let crates: HashMap<String, Vec<_>> = toml["crates"]
            .as_table()
            .expect("missing crates table")
            .into_iter()
            .map(|(k, v)| {
                (
                    k.into(),
                    v.as_array()
                        .expect("value must be array")
                        .iter()
                        .map(|v| v.as_str().expect("must be string").to_string())
                        .collect::<Vec<_>>(),
                )
            })
            .collect();
        FakeIndex { crates }
    }
}

#[cfg(test)]
mod test {
    use crate::index::{is_published, CratesIndex};
    use semver::Version;
    use std::collections::HashMap;
    use std::sync::Arc;

    /// Ignored test against the real index
    #[ignore]
    #[test]
    fn test_known_published_versions() {
        let index = Arc::new(CratesIndex::real().unwrap());
        let known_published = Version::new(1, 1, 7);
        let known_never_published = Version::new(999, 999, 999);
        assert!(
            is_published(&index, "aws-smithy-runtime-api", &known_published).unwrap()
        );

        assert!(
            !is_published(&index, "aws-smithy-runtime-api", &known_never_published).unwrap()
        );
    }

    /// Ignored test against the real index
    #[test]
    fn test_against_fake_index() {
        let mut crates = HashMap::new();
        crates.insert(
            "aws-smithy-runtime-api".to_string(),
            vec!["1.1.7".to_string()],
        );
        let index = Arc::new(CratesIndex::fake_from_map(crates));
        let known_published = Version::new(1, 1, 7);
        let known_never_published = Version::new(999, 999, 999);
        assert!(
            is_published(&index, "aws-smithy-runtime-api", &known_published).unwrap()
        );

        assert!(
            !is_published(&index, "aws-smithy-runtime-api", &known_never_published).unwrap()
        );
    }
}
