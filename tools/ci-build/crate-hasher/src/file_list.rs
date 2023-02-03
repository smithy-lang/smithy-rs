/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::collections::BTreeSet;
use std::fmt::Write;
use std::fs::Metadata;
use std::path::Path;

use anyhow::{Context, Result};

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
        let location = location.canonicalize().context("canonicalize input path")?;
        let mut file_list = FileList::new();

        let mut ignore_builder = ignore::WalkBuilder::new(&location);
        ignore_builder
            .ignore(false) // Don't consider .ignore files
            .git_ignore(true) // Do consider .gitignore files
            .require_git(false) // Don't require a git repository to consider .gitignore
            .hidden(true); // Ignore hidden files
        ignore_builder
            .add_ignore("/target") // Ignore root target directories
            .expect("valid ignore path");
        for dir_entry in ignore_builder.build() {
            let dir_entry = dir_entry.context("dir_entry")?;
            if !dir_entry.file_type().context("file_type")?.is_dir() {
                let path = dir_entry.path();
                let relative_path = path
                    .strip_prefix(&location)
                    .expect("location is the parent directory");

                file_list.insert(FileMetadata {
                    mode: file_mode(path, &dir_entry.metadata()?).context("file mode")?,
                    path: relative_path
                        .to_str()
                        .expect("not using unusual file names in crate source")
                        .into(),
                    sha256: sha256::try_digest(path).context("hash file")?,
                });
            }
        }
        Ok(file_list)
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
        write!(&mut entry, "{:06o} ", self.mode).unwrap();
        entry.push_str(&self.path);
        entry.push(' ');
        entry.push_str(&self.sha256);
        entry.push('\n');
        entry
    }
}

/// Returns the file mode (permissions) for the given path
fn file_mode(path: &Path, metadata: &Metadata) -> Result<u32> {
    use std::os::unix::fs::PermissionsExt;

    if metadata.file_type().is_symlink() {
        let actual_path = std::fs::read_link(path).context("follow symlink")?;
        let actual_metadata = std::fs::metadata(&actual_path).context("file metadata")?;
        file_mode(&actual_path, &actual_metadata)
    } else {
        Ok(metadata.permissions().mode())
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
