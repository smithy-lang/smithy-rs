/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Helpers shared between modules' unit tests.
//!
//! Compiled only under `cfg(test)`, so none of this reaches a release build.

use std::path::{Path, PathBuf};

/// A unique directory under the system temp dir, removed when the guard drops.
///
/// The crate has no `tempfile` dependency and this is the only thing it would be used for. Keeping
/// it here means the dependency list stays as short as the crate's job, which matters for something
/// that has to build before the release it is checking.
pub fn tempdir(tag: &str) -> TempDir {
    let unique = format!(
        "canary-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("the clock is after 1970")
            .as_nanos()
    );
    let path = std::env::temp_dir().join(unique);
    std::fs::create_dir_all(&path).expect("creating a temp dir");
    TempDir(path)
}

pub struct TempDir(PathBuf);

impl TempDir {
    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        // Best effort: a leftover directory under /tmp is not worth failing a test over.
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
