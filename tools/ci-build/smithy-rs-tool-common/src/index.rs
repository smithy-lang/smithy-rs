/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::retry::{run_with_retry_sync, ErrorClass};
use anyhow::{anyhow, Context, Error, Result};
use crates_index::Crate;
use reqwest::StatusCode;
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
