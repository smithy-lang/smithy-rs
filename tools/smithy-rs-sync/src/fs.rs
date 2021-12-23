use super::here;
use anyhow::Context;
use once_cell::sync::Lazy;
use std::collections::HashSet;
use std::error::Error;
use std::ffi::OsStr;
use std::fmt::Display;
use std::path::{Path, PathBuf};

static HANDWRITTEN_FILES_DOTFILE: Lazy<&OsStr> = Lazy::new(|| OsStr::new(".handwritten_files"));

pub fn delete_all_generated_files_and_folders(directory: &Path) -> anyhow::Result<()> {
    fn callback(path: &Path) -> Result<(), std::io::Error> {
        if path.is_file() {
            std::fs::remove_file(path)?
        } else if path.is_dir() {
            std::fs::remove_dir_all(path)?
        };

        Ok(())
    }

    let handwritten_files =
        HandwrittenFiles::from_dotfile(&directory.join(*HANDWRITTEN_FILES_DOTFILE))
            .context(here!())?;

    handwritten_files
        .run_callback_on_generated_files(&directory, callback)
        .context(here!())
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
/// fn main() {
///   let handwritten_files = HandwrittenFiles::from_dotfile(
///       Path::new("/Users/zelda/project/.handwritten_files")
///     ).unwrap();
///
///   assert!(handwritten_files.is_handwritten(Path::new("file.txt")));
///   assert!(handwritten_files.is_handwritten(Path::new("folder1/")));
/// }
/// ```
#[derive(Debug)]
struct HandwrittenFiles {
    files_and_folders: HashSet<PathBuf>,
}

impl HandwrittenFiles {
    pub fn from_dotfile(dotfile_path: &Path) -> Result<Self, HandwrittenFilesError> {
        let file_contents = std::fs::read_to_string(dotfile_path)?;
        let dotfile_parent_folder = dotfile_path
            .parent()
            .expect("dotfile will always have a parent folder");
        let files_and_folders = file_contents
            .split("\n")
            .filter_map(|line| {
                if line.is_empty() || line.starts_with("#") {
                    // skip empty lines and comments
                    None
                } else {
                    Some(dotfile_parent_folder.join(line))
                }
            })
            .collect();

        Ok(Self { files_and_folders })
    }

    pub fn is_handwritten(&self, path: &Path) -> Result<bool, HandwrittenFilesError> {
        if path.is_file() || path.is_dir() {
            Ok(self.files_and_folders.contains(path))
        } else {
            Err(HandwrittenFilesError::NotAFileOrFolder(path.to_owned()))
        }
    }

    pub fn run_callback_on_generated_files(
        &self,
        directory: &Path,
        callback: impl Fn(&Path) -> Result<(), std::io::Error>,
    ) -> Result<(), HandwrittenFilesError> {
        let read_dir = std::fs::read_dir(directory)?;

        for entry in read_dir {
            let path = &entry.map(|e| e.path())?;

            // Skip the dotfile defining what is and isn't handwritten
            if path.file_name() == Some(&HANDWRITTEN_FILES_DOTFILE) {
                continue;
            }

            let is_generated = !self.is_handwritten(path)?;
            if is_generated {
                callback(path).map_err(|io| HandwrittenFilesError::Io(Box::new(io)))?;
            }
        }

        Ok(())
    }
}

#[derive(Debug)]
pub enum HandwrittenFilesError {
    NotAFileOrFolder(PathBuf),
    Io(Box<dyn Error + Send + Sync + 'static>),
}

impl Display for HandwrittenFilesError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(err) => {
                write!(f, "IO error: {}", err)
            }
            Self::NotAFileOrFolder(path) => {
                write!(f, "path '{}' is not a file", path.display())
            }
        }
    }
}

impl std::error::Error for HandwrittenFilesError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(err) => Some(err.as_ref() as _),
            Self::NotAFileOrFolder(_) => None,
        }
    }
}

impl From<std::io::Error> for HandwrittenFilesError {
    fn from(err: std::io::Error) -> Self {
        Self::Io(Box::new(err))
    }
}

#[cfg(test)]
mod tests {
    use super::{delete_all_generated_files_and_folders, HANDWRITTEN_FILES_DOTFILE};
    use pretty_assertions::assert_eq;
    use std::fs::File;
    use std::io::Write;
    use tempdir::TempDir;

    fn create_test_dir_and_handwritten_files_dotfile(handwritten_files: &[&str]) -> TempDir {
        let dir = TempDir::new("smithy-rs-sync_test-fs").unwrap();
        let file_path = dir.path().join(*HANDWRITTEN_FILES_DOTFILE);
        let handwritten_files = handwritten_files.join("\n");
        let mut f = File::create(file_path).unwrap();

        f.write_all(handwritten_files.as_bytes()).unwrap();
        f.sync_all().unwrap();

        dir
    }

    fn create_test_file(temp_dir: &TempDir, name: &str) {
        let file_path = temp_dir.path().join(name);
        let mut f = File::create(file_path).unwrap();

        // TODO do I have to write anything?
        f.write_all(b"").unwrap();
        f.sync_all().unwrap();
    }

    fn create_test_dir(temp_dir: &TempDir, name: &str) {
        let dir_path = temp_dir.path().join(name);
        std::fs::create_dir(dir_path).unwrap();
    }

    #[test]
    fn test_delete_all_generated_files_and_folders_doesnt_delete_handwritten_files_and_folders() {
        let handwritten_files = &["foo.txt", "bar/"];
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
        let handwritten_files = &["foo.txt", "bar/"];
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
        let handwritten_files = &["# a fake comment", "", "# another comment", ""];
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
}
