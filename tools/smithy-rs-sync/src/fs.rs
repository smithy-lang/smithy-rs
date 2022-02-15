/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use anyhow::Context;
use gitignore::Pattern;
use smithy_rs_tool_common::macros::here;
use std::error::Error;
use std::fmt::Display;
use std::path::{Path, PathBuf};

pub static HANDWRITTEN_DOTFILE: &str = ".handwritten";

pub fn delete_all_generated_files_and_folders(directory: &Path) -> anyhow::Result<()> {
    eprintln!("\tchecking for 'generated' files and folders in the current SDK...");
    let dotfile_path = directory.join(HANDWRITTEN_DOTFILE);
    eprintln!("\tloading dotfile at {}", dotfile_path.display());
    let handwritten_files =
        HandwrittenFiles::from_dotfile(&dotfile_path, directory).context(here!())?;
    let generated_files = handwritten_files
        .generated_files_and_folders_iter()
        .context(here!())?;

    let mut file_count = 0;
    let mut folder_count = 0;

    for path in generated_files {
        if path.is_file() {
            std::fs::remove_file(path)?;
            file_count += 1;
        } else if path.is_dir() {
            std::fs::remove_dir_all(path)?;
            folder_count += 1;
        };
    }

    eprintln!(
        "\tdeleted {} 'generated' files and {} folders in the current SDK folder",
        file_count, folder_count
    );

    Ok(())
}

pub fn find_handwritten_files_and_folders(
    aws_sdk_path: &Path,
    build_artifacts_path: &Path,
) -> anyhow::Result<Vec<PathBuf>> {
    eprintln!("\tchecking for 'handwritten' files and folders in the generated SDK folder...");
    let dotfile_path = aws_sdk_path.join(HANDWRITTEN_DOTFILE);
    eprintln!("\tloading dotfile at {}", dotfile_path.display());
    let handwritten_files =
        HandwrittenFiles::from_dotfile(&dotfile_path, build_artifacts_path).context(here!())?;

    let files: Vec<_> = handwritten_files
        .handwritten_files_and_folders_iter()
        .context(here!())?
        .collect();

    eprintln!(
        "\tfound {} 'handwritten' files and folders in the generated SDK folder",
        files.len()
    );

    Ok(files)
}

/// Similar to [`std::fs::remove_dir_all`] except that it doesn't error out if
/// the directory to be removed doesn't exist.
pub fn remove_dir_all_idempotent(path: impl AsRef<Path>) -> anyhow::Result<()> {
    match std::fs::remove_dir_all(path.as_ref()) {
        Ok(_) => Ok(()),
        Err(err) => match err.kind() {
            std::io::ErrorKind::NotFound => Ok(()),
            _ => Err(err).context(here!()),
        },
    }
}

/// A struct with methods that help when checking to see if a file is handwritten or
/// automatically generated.
///
/// # Examples
///
/// for a .handwritten_files that looks like this:
///
/// ```
/// # Only paths referring to siblings of the .handwritten_files file are valid
///
/// # this file will be protected
/// file.txt
/// # this folder will be protected
/// folder1/
/// ```
///
/// ```rust
///   let handwritten_files = HandwrittenFiles::from_dotfile(
///       Path::new("/Users/zelda/project/.handwritten_files")
///     ).unwrap();
///
///   assert!(handwritten_files.is_handwritten(Path::new("file.txt")));
///   assert!(handwritten_files.is_handwritten(Path::new("folder1/")));
/// }
/// ```
#[derive(Debug)]
pub struct HandwrittenFiles {
    patterns: String,
    root: PathBuf,
}

#[derive(Debug, PartialEq, Eq)]
pub enum FileKind {
    Generated,
    Handwritten,
}

impl HandwrittenFiles {
    pub fn from_dotfile(dotfile_path: &Path, root: &Path) -> Result<Self, HandwrittenFilesError> {
        let handwritten_files = Self {
            patterns: std::fs::read_to_string(dotfile_path)?,
            root: root.canonicalize()?,
        };

        let dotfile_kind = handwritten_files.file_kind(&root.join(HANDWRITTEN_DOTFILE));

        if dotfile_kind == FileKind::Generated {
            eprintln!(
                "warning: your handwritten dotfile at {} isn't marked as handwritten, is this intentional?",
                dotfile_path.display()
            );
        }

        Ok(handwritten_files)
    }

    fn patterns(&self) -> impl Iterator<Item = Pattern> {
        self.patterns
            .lines()
            .filter(|line| !line.is_empty())
            .flat_map(move |line| Pattern::new(line, &self.root))
    }

    pub fn file_kind(&self, path: &Path) -> FileKind {
        for pattern in self.patterns() {
            // if the gitignore=handwritten files matches this path, this is hand written
            if pattern.is_excluded(path, path.is_dir()) {
                return FileKind::Handwritten;
            }
        }

        FileKind::Generated
    }

    pub fn generated_files_and_folders_iter(
        &self,
    ) -> Result<impl Iterator<Item = PathBuf> + '_, HandwrittenFilesError> {
        self.files_and_folders_iter(FileKind::Generated)
    }

    pub fn handwritten_files_and_folders_iter(
        &self,
    ) -> Result<impl Iterator<Item = PathBuf> + '_, HandwrittenFilesError> {
        self.files_and_folders_iter(FileKind::Handwritten)
    }

    fn files_and_folders_iter(
        &self,
        kind: FileKind,
    ) -> Result<impl Iterator<Item = PathBuf> + '_, HandwrittenFilesError> {
        let files = std::fs::read_dir(&self.root)?.collect::<Result<Vec<_>, _>>()?;
        Ok(files
            .into_iter()
            .map(|entry| entry.path())
            .filter(move |path| self.file_kind(path) == kind))
    }
}

#[derive(Debug)]
pub enum HandwrittenFilesError {
    Io(std::io::Error),
    GitIgnore(gitignore::Error),
}

impl Display for HandwrittenFilesError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(err) => {
                write!(f, "IO error: {}", err)
            }
            Self::GitIgnore(err) => {
                write!(f, "gitignore error: {}", err)
            }
        }
    }
}

impl std::error::Error for HandwrittenFilesError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(err) => Some(err),
            Self::GitIgnore(err) => Some(err),
        }
    }
}

impl From<std::io::Error> for HandwrittenFilesError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<gitignore::Error> for HandwrittenFilesError {
    fn from(error: gitignore::Error) -> Self {
        Self::GitIgnore(error)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        delete_all_generated_files_and_folders, find_handwritten_files_and_folders,
        HANDWRITTEN_DOTFILE,
    };
    use pretty_assertions::assert_eq;
    use std::fs::File;
    use tempdir::TempDir;

    fn create_test_dir_and_handwritten_files_dotfile(handwritten_files: &[&str]) -> TempDir {
        let dir = TempDir::new("smithy-rs-sync_test-fs").unwrap();
        let file_path = dir.path().join(HANDWRITTEN_DOTFILE);
        // two newlines to test
        let handwritten_files = handwritten_files.join("\n\n");
        std::fs::write(file_path, handwritten_files).expect("failed to write");
        dir
    }

    fn create_test_file(temp_dir: &TempDir, name: &str) {
        let file_path = temp_dir.path().join(name);
        let f = File::create(file_path).unwrap();

        f.sync_all().unwrap();
    }

    fn create_test_dir(temp_dir: &TempDir, name: &str) {
        let dir_path = temp_dir.path().join(name);
        std::fs::create_dir(dir_path).unwrap();
    }

    #[test]
    fn test_delete_all_generated_files_and_folders_doesnt_delete_handwritten_files_and_folders() {
        let handwritten_files = &[HANDWRITTEN_DOTFILE, "foo.txt", "bar/"];
        let dir = create_test_dir_and_handwritten_files_dotfile(handwritten_files);
        create_test_file(&dir, "foo.txt");
        create_test_dir(&dir, "bar");

        // The files and folders in the temp dir should be the same
        // before and after running delete_all_generated_files_and_folders
        let expected_files_and_folders: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect();

        delete_all_generated_files_and_folders(dir.path()).unwrap();

        let actual_files_and_folders: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect();

        assert_eq!(expected_files_and_folders, actual_files_and_folders);
    }

    #[test]
    fn test_delete_all_generated_files_and_folders_deletes_generated_files_and_folders() {
        let handwritten_files = &[HANDWRITTEN_DOTFILE, "foo.txt", "bar/"];
        let dir = create_test_dir_and_handwritten_files_dotfile(handwritten_files);
        // We add the "handwritten" ones
        create_test_file(&dir, "foo.txt");
        create_test_dir(&dir, "bar");

        // The files and folders in the temp dir should be the same
        // before and after running delete_all_generated_files_and_folders
        let expected_files_and_folders: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect();

        // We add the "generated" ones that will be deleted
        create_test_file(&dir, "bar.txt");
        create_test_dir(&dir, "qux");

        delete_all_generated_files_and_folders(dir.path()).unwrap();

        let actual_files_and_folders: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect();

        assert_eq!(expected_files_and_folders, actual_files_and_folders);
    }

    #[test]
    fn test_delete_all_generated_files_and_folders_ignores_comment_and_empty_lines() {
        let handwritten_files = &[HANDWRITTEN_DOTFILE, "# a fake comment", ""];
        let dir = create_test_dir_and_handwritten_files_dotfile(handwritten_files);

        // The files and folders in the temp dir should be the same
        // before and after running delete_all_generated_files_and_folders
        let expected_files_and_folders: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect();

        delete_all_generated_files_and_folders(dir.path()).unwrap();

        let actual_files_and_folders: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect();

        assert_eq!(expected_files_and_folders, actual_files_and_folders);
        // the only file present will be the dotfile
        assert_eq!(actual_files_and_folders.len(), 1)
    }

    #[test]
    fn test_find_handwritten_files_works() {
        let handwritten_files = &[HANDWRITTEN_DOTFILE, "foo.txt", "bar/"];
        let sdk_dir = create_test_dir_and_handwritten_files_dotfile(handwritten_files);

        let build_artifacts_dir = create_test_dir_and_handwritten_files_dotfile(handwritten_files);
        // Add the "handwritten" ones to be found
        create_test_file(&build_artifacts_dir, "foo.txt");
        create_test_dir(&build_artifacts_dir, "bar");

        // The files and folders in the temp dir should be the same
        // before and after running delete_all_generated_files_and_folders
        let expected_files_and_folders: Vec<_> = std::fs::read_dir(build_artifacts_dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().path().canonicalize().unwrap())
            .collect();

        // Add the "generated" ones that won't be found
        create_test_file(&build_artifacts_dir, "bar.txt");
        create_test_dir(&build_artifacts_dir, "qux");

        let actual_files_and_folders =
            find_handwritten_files_and_folders(sdk_dir.path(), build_artifacts_dir.path()).unwrap();

        assert_eq!(expected_files_and_folders, actual_files_and_folders);
    }
}
