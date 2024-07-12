/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::io::error::{Error, ErrorKind};
use crate::io::stream::{InputStream, RawInputStream};
use std::fs;
use std::path::PathBuf;

/// Input stream designed to wrap file based input.
#[derive(Debug)]
pub(super) struct PathBody {
    // The path to the file
    pub(super) path: PathBuf,
    // The total number of bytes to read
    pub(super) length: u64,
    // The byte-offset to start reading from
    pub(super) offset: u64,
}

/// Builder for creating [`InputStream`](InputStream) from a file/path.
///
/// ```no_run
/// # {
/// use aws_s3_transfer_manager::io::InputStream;
/// use std::path::Path;
///
/// async fn input_stream_from_file() -> InputStream {
///     let stream = InputStream::read_from()
///         .path("docs/some-large-file.csv")
///         // Specify the length of the file used (skips an additional call to retrieve the size)
///         .build()
///         .expect("valid path");
///     stream
/// }
/// # }
/// ```
#[derive(Debug, Default)]
pub struct PathBodyBuilder {
    path: Option<PathBuf>,
    length: Option<u64>,
    offset: Option<u64>,
}

impl PathBodyBuilder {
    /// Create a new [`PathBodyBuilder`].
    ///
    /// You must call [`path`](PathBodyBuilder::path) to specify what to read from.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the path to read from.
    pub fn path(mut self, path: impl AsRef<std::path::Path>) -> Self {
        self.path = Some(path.as_ref().to_path_buf());
        self
    }

    /// Specify the offset to start reading from (in bytes)
    ///
    /// When used in conjunction with [`length`](PathBodyBuilder::length), allows for reading a single "chunk" of a file.
    pub fn offset(mut self, offset: u64) -> Self {
        self.offset = Some(offset);
        self
    }

    /// Specify the length to read (in bytes).
    ///
    /// By pre-specifying the length, this API skips an additional call to retrieve the size from file-system metadata.
    ///
    /// When used in conjunction with [`offset`](PathBodyBuilder::offset), allows for reading a single "chunk" of a file.
    ///
    /// <div class="warning">
    /// Setting the length manually will trigger no validation related to any offset provided or the actual size of
    /// the file. This is an advanced setting mainly used to avoid an additional syscall if you know the
    /// size of the file already.
    /// </div>
    pub fn length(mut self, length: u64) -> Self {
        self.length = Some(length);
        self
    }

    /// Returns a [`InputStream`] from this builder.
    pub fn build(self) -> Result<InputStream, Error> {
        let path = self.path.expect("path set");
        let offset = self.offset.unwrap_or_default();

        let length = match self.length {
            None => {
                // TODO(aws-sdk-rust#1159, design) - evaluate if we want build() to be async and to use tokio for stat() call (bytestream FsBuilder::build() is async)
                let metadata = fs::metadata(path.clone())?;
                let file_size = metadata.len();

                if offset >= file_size {
                    return Err(ErrorKind::OffsetGreaterThanFileSize.into());
                }

                file_size - offset
            }
            Some(explicit) => explicit,
        };

        let body = PathBody {
            path,
            length,
            offset,
        };

        let stream = InputStream {
            inner: RawInputStream::Fs(body),
        };

        Ok(stream)
    }
}

#[cfg(test)]
mod test {
    use std::io::Write;
    use tempfile::NamedTempFile;

    use crate::io::{path_body::PathBodyBuilder, InputStream};

    use super::PathBody;

    fn path_body(stream: &InputStream) -> &PathBody {
        match &stream.inner {
            crate::io::stream::RawInputStream::Buf(_) => panic!("unexpected inner body"),
            crate::io::stream::RawInputStream::Fs(path_body) => path_body,
        }
    }

    #[test]
    fn test_from_path() {
        let mut tmp = NamedTempFile::new().unwrap();
        let content = "hello path body";
        tmp.write_all(content.as_bytes()).unwrap();

        let stream = PathBodyBuilder::new().path(tmp.path()).build().unwrap();
        let body = path_body(&stream);
        assert_eq!(0, body.offset);
        assert_eq!(content.as_bytes().len() as u64, body.length);
    }

    #[test]
    fn test_explicit_content_length() {
        let mut tmp = NamedTempFile::new().unwrap();

        let stream = PathBodyBuilder::new()
            .path(tmp.path())
            .length(64)
            .build()
            .unwrap();

        let body = path_body(&stream);
        assert_eq!(0, body.offset);
        // we don't validate this
        assert_eq!(64, body.length);
    }

    #[test]
    fn test_length_with_offset() {
        let mut tmp = NamedTempFile::new().unwrap();
        let content = "hello path body";
        tmp.write_all(content.as_bytes()).unwrap();
        let offset = 5;

        let stream = PathBodyBuilder::new()
            .path(tmp.path())
            .offset(offset)
            .build()
            .unwrap();

        let body = path_body(&stream);
        assert_eq!(offset, body.offset);
        assert_eq!(content.len() as u64 - offset, body.length);
    }

    #[test]
    fn test_explicit_content_length_and_offset() {
        let mut tmp = NamedTempFile::new().unwrap();

        let stream = PathBodyBuilder::new()
            .path(tmp.path())
            .length(64)
            .offset(12)
            .build()
            .unwrap();

        let body = path_body(&stream);
        assert_eq!(12, body.offset);
        assert_eq!(64, body.length);
    }

    #[should_panic]
    #[test]
    fn test_invalid_offset() {
        let mut tmp = NamedTempFile::new().unwrap();
        let content = "hello path body";
        tmp.write_all(content.as_bytes()).unwrap();

        let stream = PathBodyBuilder::new()
            .path(tmp.path())
            .offset(22)
            .build()
            .unwrap();
    }
}
