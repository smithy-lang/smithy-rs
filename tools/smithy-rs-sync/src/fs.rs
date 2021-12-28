/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use super::here;
use anyhow::Context;
use std::error::Error;
use std::fmt::Display;
use std::path::{Path, PathBuf};

pub static HANDWRITTEN_DOTFILE: &str = ".handwritten";

pub fn delete_all_generated_files_and_folders(directory: &Path) -> anyhow::Result<()> {
    let dotfile_path = directory.join(HANDWRITTEN_DOTFILE);
    let handwritten_files = HandwrittenFiles::from_dotfile(&dotfile_path).context(here!())?;

    for path in handwritten_files
        .generated_files_and_folders_iter(directory)
        .context(here!())?
    {
        if path.is_file() {
            std::fs::remove_file(path)?
        } else if path.is_dir() {
            std::fs::remove_dir_all(path)?
        };
    }

    Ok(())
}

pub fn find_handwritten_files_and_folders(
    aws_sdk_path: &Path,
    build_artifacts_path: &Path,
) -> anyhow::Result<Vec<PathBuf>> {
    let dotfile_path = aws_sdk_path.join(HANDWRITTEN_DOTFILE);
    let handwritten_files = HandwrittenFiles::from_dotfile(&dotfile_path).context(here!())?;

    let files = handwritten_files
        .handwritten_files_and_folders_iter(build_artifacts_path)
        .context(here!())?
        .collect();

    Ok(files)
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
pub struct HandwrittenFiles<'file> {
    files_and_folders: gitignore::File<'file>,
}

#[derive(Debug, PartialEq, Eq)]
enum FileKind {
    Generated,
    Handwritten,
}

impl<'file> HandwrittenFiles<'file> {
    pub fn from_dotfile(dotfile_path: &'file Path) -> Result<Self, HandwrittenFilesError> {
        let files_and_folders = gitignore::File::new(dotfile_path)?;

        Ok(Self { files_and_folders })
    }

    pub fn is_handwritten(&self, path: &'file Path) -> Result<bool, HandwrittenFilesError> {
        self.files_and_folders
            .is_excluded(path)
            // Flip the boolean because we're inclusive
            .map(|is_excluded| !is_excluded)
            .map_err(Into::into)
    }

    pub fn generated_files_and_folders_iter(
        &'file self,
        directory: &'file Path,
    ) -> Result<impl Iterator<Item = PathBuf>, HandwrittenFilesError> {
        self.files_and_folders_iter(directory, FileKind::Generated)
    }

    pub fn handwritten_files_and_folders_iter(
        &self,
        directory: &Path,
    ) -> Result<impl Iterator<Item = PathBuf>, HandwrittenFilesError> {
        self.files_and_folders_iter(directory, FileKind::Handwritten)
    }

    fn files_and_folders_iter(
        &self,
        directory: &Path,
        kind: FileKind,
    ) -> Result<impl Iterator<Item = PathBuf>, HandwrittenFilesError> {
        let mut err = Ok(());
        // All for the lack of try_filter
        let scan_results: Vec<_> = std::fs::read_dir(directory)?
            .scan(
                &mut err,
                |err: &mut &mut Result<(), HandwrittenFilesError>,
                 res: std::io::Result<std::fs::DirEntry>| {
                    let res = res
                        .map(|entry| entry.path())
                        .map(|path| (self.is_handwritten(&path), path))
                        .map_err(HandwrittenFilesError::from);

                    match res {
                        Ok((Ok(is_handwritten), path)) => match kind {
                            FileKind::Generated => Some((!is_handwritten, path)),
                            FileKind::Handwritten => Some((is_handwritten, path)),
                        },
                        // entry was bad, stop iteration and surface the error
                        Ok((Err(e), _)) | Err(e) => {
                            **err = Err(e.into());
                            None
                        }
                    }
                },
            )
            .filter_map(|(is_handwritten, path)| (!is_handwritten).then(move || path))
            .collect();
        // If the scan failed, bubble up the error
        err?;

        // return an iter of paths for the requested kind of files and folders
        Ok(scan_results.into_iter())
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
    use std::io::Write;
    use tempdir::TempDir;

    fn create_test_dir_and_handwritten_files_dotfile(handwritten_files: &[&str]) -> TempDir {
        let dir = TempDir::new("smithy-rs-sync_test-fs").unwrap();
        let file_path = dir.path().join(HANDWRITTEN_DOTFILE);
        let handwritten_files = handwritten_files.join("\n");
        let mut f = File::create(file_path).unwrap();

        f.write_all(handwritten_files.as_bytes()).unwrap();
        f.sync_all().unwrap();

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
        let dir = create_test_dir_and_handwritten_files_dotfile(handwritten_files);
        // Add the "handwritten" ones to be found
        create_test_file(&dir, "foo.txt");
        create_test_dir(&dir, "bar");

        // The files and folders in the temp dir should be the same
        // before and after running delete_all_generated_files_and_folders
        let expected_files_and_folders: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect();

        // Add the "generated" ones that won't be found
        create_test_file(&dir, "bar.txt");
        create_test_dir(&dir, "qux");

        // In practice, these would be two different folders but using the same folder is fine for the test
        let actual_files_and_folders =
            find_handwritten_files_and_folders(dir.path(), dir.path()).unwrap();

        assert_eq!(expected_files_and_folders, actual_files_and_folders);
    }
}
