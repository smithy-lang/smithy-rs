/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use anyhow::{bail, Context, Result};
use std::collections::BTreeSet;
use std::path::Path;

#[derive(Debug, Default)]
pub struct FileList(BTreeSet<FileMetadata>);

impl FileList {
    pub fn new() -> Self {
        Self(BTreeSet::new())
    }

    /// Inserts file metadata into the list
    fn insert(&mut self, file_metadata: FileMetadata) {
        self.0.insert(file_metadata);
    }

    /// Returns the SHA-256 hash of the file list
    pub fn sha256(&self) -> String {
        sha256::digest(self.entries())
    }

    /// Returns the hashable entries in this file list
    pub fn entries(&self) -> String {
        let mut entries = String::new();
        for metadata in &self.0 {
            entries.push_str(&metadata.hash_entry());
        }
        entries
    }

    /// Uses Cargo's `PathSource` implementation to discover which files are relevant to building the crate
    pub fn discover(location: &Path) -> Result<FileList> {
        use cargo::core::manifest::EitherManifest;
        use cargo::core::package::Package;
        use cargo::core::source::SourceId;
        use cargo::sources::path::PathSource;
        use cargo::util::config::Config;
        use cargo::util::toml::read_manifest;

        let location = location.canonicalize().context("canonicalize input path")?;
        let config = Config::default().context("default cargo config")?;
        let source_id = SourceId::for_path(&location).context("resolve cargo source id")?;
        let path_source = PathSource::new(&location, source_id, &config);

        let manifest_path = location.join("Cargo.toml");
        // `_related_manifests` is a list of Cargo.toml paths for path dependencies used
        // by the crate, which is not relevant for this tool's use-case.
        let (either_manifest, _related_manifests) =
            read_manifest(&manifest_path, source_id, &config).context("read Cargo.toml file")?;
        if let EitherManifest::Real(manifest) = either_manifest {
            let package = Package::new(manifest, &manifest_path);
            let paths = path_source
                .list_files(&package)
                .context("list crate files")?;

            let mut file_list = FileList::new();
            for path in paths {
                let relative_path = path
                    .strip_prefix(&location)
                    .expect("location is the parent directory");

                file_list.insert(FileMetadata {
                    mode: file_mode(&path).context("file mode")?,
                    path: relative_path
                        .to_str()
                        .expect("not using unusual file names in crate source")
                        .into(),
                    sha256: sha256::digest_file(&path).context("hash file")?,
                });
            }
            Ok(file_list)
        } else {
            bail!("This tool doesn't support virtual cargo manifests");
        }
    }
}

/// Hashable metadata about a crate file
#[derive(Debug, Ord, PartialOrd, Eq, PartialEq)]
struct FileMetadata {
    // Order of these members MUST NOT change, or else the file order will change which will break hashing
    path: String,
    mode: u32,
    sha256: String,
}

impl FileMetadata {
    /// Returns a string to hash for this file
    fn hash_entry(&self) -> String {
        let mut entry = String::with_capacity(7 + self.path.len() + 1 + self.sha256.len() + 1);
        entry.push_str(&format!("{:06o} ", self.mode));
        entry.push_str(&self.path);
        entry.push(' ');
        entry.push_str(&self.sha256);
        entry.push('\n');
        entry
    }
}

/// Returns the file mode (permissions) for the given path
fn file_mode(path: &Path) -> Result<u32> {
    use std::os::unix::fs::PermissionsExt;

    let file_metadata = std::fs::metadata(&path).context("file metadata")?;
    if file_metadata.is_symlink() {
        let actual_path = std::fs::read_link(path).context("follow symlink")?;
        file_mode(&actual_path)
    } else {
        Ok(file_metadata.permissions().mode())
    }
}

#[cfg(test)]
mod tests {
    use super::FileMetadata;

    #[test]
    fn hash_entry() {
        let metadata = FileMetadata {
            path: "src/something.rs".into(),
            mode: 0o100644,
            sha256: "521fe5c9ece1aa1f8b66228171598263574aefc6fa4ba06a61747ec81ee9f5a3".into(),
        };
        assert_eq!("100644 src/something.rs 521fe5c9ece1aa1f8b66228171598263574aefc6fa4ba06a61747ec81ee9f5a3\n", metadata.hash_entry());
    }
}
