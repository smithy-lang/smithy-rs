/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
use std::cmp;
use std::ops::DerefMut;
use std::sync::Mutex;

use bytes::{Buf, Bytes, BytesMut};

use crate::io::error::Error;
use crate::io::path_body::PathBody;
use crate::io::stream::RawInputStream;
use crate::io::InputStream;
use crate::MEBIBYTE;

/// Builder for creating a `ReadPart` implementation.
#[derive(Debug)]
pub(crate) struct Builder {
    stream: Option<RawInputStream>,
    part_size: usize,
}

impl Builder {
    pub(crate) fn new() -> Self {
        Self {
            stream: None,
            part_size: 5 * MEBIBYTE as usize,
        }
    }

    /// Set the input stream to read from.
    pub(crate) fn stream(mut self, stream: InputStream) -> Self {
        self.stream = Some(stream.inner);
        self
    }

    /// Set the target part size that should be used when reading data.
    ///
    /// All parts except for possibly the last one should be of this size.
    pub(crate) fn part_size(mut self, part_size: usize) -> Self {
        self.part_size = part_size;
        self
    }

    pub(crate) fn build(self) -> impl ReadPart {
        let stream = self.stream.expect("input stream set");
        match stream {
            RawInputStream::Buf(buf) => {
                PartReader::Bytes(BytesPartReader::new(buf, self.part_size))
            }
            RawInputStream::Fs(path_body) => {
                PartReader::Fs(PathBodyPartReader::new(path_body, self.part_size))
            }
        }
    }
}

#[derive(Debug)]
enum PartReader {
    Bytes(BytesPartReader),
    Fs(PathBodyPartReader),
}

impl ReadPart for PartReader {
    async fn next_part(&self) -> Result<Option<PartData>, Error> {
        match self {
            PartReader::Bytes(bytes) => bytes.next_part().await,
            PartReader::Fs(path_body) => path_body.next_part().await,
        }
    }
}

/// Data for a single part
pub(crate) struct PartData {
    // 1-indexed 
    pub(crate) part_number: u64,
    pub(crate) data: Bytes,
}

/// The `ReadPart` trait allows for reading data from an `InputStream` and packaging the raw
/// data into `PartData` which carries additional metadata needed for uploading a part.
pub(crate) trait ReadPart {
    /// Request the next "part" of data.
    ///
    /// When there is no more data readers should return `Ok(None)`.
    /// NOTE: Implementations are allowed to return data in any order and consumers are
    /// expected to order data by the part number.
    fn next_part(
        &self,
    ) -> impl std::future::Future<Output = Result<Option<PartData>, Error>> + Send;
}

#[derive(Debug)]
struct PartReaderState {
    // current start offset
    offset: u64,
    // current part number
    part_number: u64,
    // total number of bytes remaining to be read
    remaining: u64,
}

impl PartReaderState {
    /// Create a new `PartReaderState`
    fn new(content_length: u64) -> Self {
        Self {
            offset: 0,
            part_number: 1,
            remaining: content_length,
        }
    }

    /// Set the initial offset to start reading from
    fn with_offset(self, offset: u64) -> Self {
        Self { offset, ..self }
    }
}

/// [ReadPart] implementation for in-memory input streams.
#[derive(Debug)]
struct BytesPartReader {
    buf: Bytes,
    part_size: usize,
    state: Mutex<PartReaderState>,
}

impl BytesPartReader {
    fn new(buf: Bytes, part_size: usize) -> Self {
        let content_length = buf.remaining() as u64;
        Self {
            buf,
            part_size,
            state: Mutex::new(PartReaderState::new(content_length)), // std Mutex
        }
    }
}

impl ReadPart for BytesPartReader {
    async fn next_part(&self) -> Result<Option<PartData>, Error> {
        let mut state = self.state.lock().expect("lock valid");
        if state.remaining == 0 {
            return Ok(None);
        }

        let start = state.offset as usize;
        let end = cmp::min(start + self.part_size, self.buf.len());
        let data = self.buf.slice(start..end);
        let part_number = state.part_number;
        state.part_number += 1;
        state.offset += data.len() as u64;
        state.remaining -= data.len() as u64;
        let part = PartData { data, part_number };
        Ok(Some(part))
    }
}

/// [ReadPart] implementation for path based input streams
#[derive(Debug)]
struct PathBodyPartReader {
    body: PathBody,
    part_size: usize,
    state: Mutex<PartReaderState>, // std Mutex
}

impl PathBodyPartReader {
    fn new(body: PathBody, part_size: usize) -> Self {
        let offset = body.offset;
        let content_length = body.length;
        Self {
            body,
            part_size,
            state: Mutex::new(PartReaderState::new(content_length).with_offset(offset)), // std Mutex
        }
    }
}

impl ReadPart for PathBodyPartReader {
    async fn next_part(&self) -> Result<Option<PartData>, Error> {
        let (offset, part_number, part_size) = {
            let mut state = self.state.lock().expect("lock valid");
            if state.remaining == 0 {
                return Ok(None);
            }
            let offset = state.offset;
            let part_number = state.part_number;

            let part_size = cmp::min(self.part_size as u64, state.remaining);
            state.offset += part_size;
            state.part_number += 1;
            state.remaining -= part_size;

            (offset, part_number, part_size)
        };
        let path = self.body.path.clone();
        let handle = tokio::task::spawn_blocking(move || {
            // TODO(aws-sdk-rust#1159) - replace allocation with memory pool
            let mut dst = BytesMut::with_capacity(part_size as usize);
            // we need to set the length so that the raw &[u8] slice has the correct
            // size, we are guaranteed to read exactly part_size data from file on success
            // FIXME(aws-sdk-rust#1159) - can we get rid of this use of unsafe?
            unsafe { dst.set_len(dst.capacity()) }
            file_util::read_file_chunk_sync(dst.deref_mut(), path, offset)?;
            let data = dst.freeze();
            Ok::<PartData, Error>(PartData { data, part_number })
        });

        handle.await?.map(Some)
    }
}

mod file_util {
    #[cfg(unix)]
    pub(super) use unix::read_file_chunk_sync;
    #[cfg(windows)]
    pub(super) use windows::read_file_chunk_sync;

    #[cfg(unix)]
    mod unix {
        use std::fs::File;
        use std::io;
        use std::os::unix::fs::FileExt;
        use std::path::Path;

        pub(crate) fn read_file_chunk_sync(
            dst: &mut [u8],
            path: impl AsRef<Path>,
            offset: u64,
        ) -> Result<(), io::Error> {
            let file = File::open(path)?;
            file.read_exact_at(dst, offset)
        }
    }

    #[cfg(windows)]
    mod windows {
        use std::fs::File;
        use std::io;
        use std::io::{Read, Seek, SeekFrom};
        use std::path::Path;

        pub(crate) fn read_file_chunk_sync(
            dst: &mut [u8],
            path: impl AsRef<Path>,
            offset: u64,
        ) -> Result<(), io::Error> {
            let mut file = File::open(path)?;
            file.seek(SeekFrom::Start(offset))?;
            file.read_exact(dst)
        }
    }
}

#[cfg(test)]
mod test {
    use std::io::Write;

    use bytes::{Buf, Bytes};
    use tempfile::NamedTempFile;

    use crate::io::part_reader::{PartData, Builder, ReadPart};
    use crate::io::InputStream;

    async fn collect_parts(reader: impl ReadPart) -> Vec<PartData> {
        let mut parts = Vec::new();
        let mut expected_part_number = 1;
        while let Some(part) = reader.next_part().await.unwrap() {
            assert_eq!(expected_part_number, part.part_number);
            expected_part_number += 1;
            parts.push(part);
        }
        parts
    }

    #[tokio::test]
    async fn test_bytes_part_reader() {
        let data = Bytes::from("a lep is a ball, a tay is a hammer, a flix is a comb");
        let stream = InputStream::from(data.clone());
        let expected = data.chunks(5).collect::<Vec<_>>();
        let reader = Builder::new().part_size(5).stream(stream).build();
        let parts = collect_parts(reader).await;
        let actual = parts.iter().map(|p| p.data.chunk()).collect::<Vec<_>>();

        assert_eq!(expected, actual);
    }

    async fn path_reader_test(limit: Option<usize>, offset: Option<usize>) {
        let part_size = 5;
        let mut tmp = NamedTempFile::new().unwrap();
        let mut data = Bytes::from("a lep is a ball, a tay is a hammer, a flix is a comb");
        tmp.write_all(data.chunk()).unwrap();

        let mut builder = InputStream::read_from().path(tmp.path());
        if let Some(limit) = limit {
            data.truncate(limit);
            builder = builder.length((limit - offset.unwrap_or_default()) as u64);
        }

        if let Some(offset) = offset {
            data.advance(offset);
            builder = builder.offset(offset as u64);
        }

        let expected = data.chunks(part_size).collect::<Vec<_>>();

        let stream = builder.build().unwrap();
        let reader = Builder::new()
            .part_size(part_size)
            .stream(stream)
            .build();

        let parts = collect_parts(reader).await;
        let actual = parts.iter().map(|p| p.data.chunk()).collect::<Vec<_>>();

        assert_eq!(expected, actual);
    }

    #[tokio::test]
    async fn test_path_part_reader() {
        path_reader_test(None, None).await;
    }

    #[tokio::test]
    async fn test_path_part_reader_with_offset() {
        path_reader_test(None, Some(8)).await;
    }

    #[tokio::test]
    async fn test_path_part_reader_with_explicit_length() {
        path_reader_test(Some(12), None).await;
    }

    #[tokio::test]
    async fn test_path_part_reader_with_length_and_offset() {
        path_reader_test(Some(23), Some(4)).await;
    }
}
