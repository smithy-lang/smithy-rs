use crate::io::stream::RawInputStream;
use crate::MEBIBYTE;
use bytes::Bytes;
use std::{cmp, io};
// TODO - replace w/parking_lot
use crate::io::InputStream;
use std::sync::Mutex;

/// Build a `PartReader`
#[derive(Debug)]
pub(crate) struct PartReaderBuilder {
    stream: Option<RawInputStream>,
    part_size: usize,
}

impl PartReaderBuilder {
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
        let reader = match stream {
            RawInputStream::Buf(buf) => BytesPartReader::new(buf, self.part_size),
            RawInputStream::Fs(_) => unimplemented!(),
        };

        reader
    }
}

/// [ReadPart] implementation for `Bytes` instance.
#[derive(Debug)]
struct BytesPartReader {
    buf: Bytes,
    part_size: usize,
    state: Mutex<PartReaderState>,
}

impl BytesPartReader {
    fn new(buf: Bytes, part_size: usize) -> Self {
        Self {
            buf,
            part_size,
            state: Mutex::new(PartReaderState::new()),
        }
    }
}

#[derive(Debug)]
struct PartReaderState {
    // current start offset
    offset: usize,
    // current part number
    part_number: u64,
}

impl PartReaderState {
    fn new() -> Self {
        Self {
            offset: 0,
            part_number: 1,
        }
    }
}

impl ReadPart for BytesPartReader {
    async fn next_part(&self) -> Result<Option<PartData>, io::Error> {
        let mut state = self.state.lock().expect("lock valid");
        let start = state.offset;
        if start >= self.buf.len() {
            return Ok(None);
        }

        let end = cmp::min(start + self.part_size, self.buf.len());
        let data = self.buf.slice(start..end);
        let part_number = state.part_number;
        state.part_number += 1;
        state.offset += data.len();
        let part = PartData { data, part_number };
        Ok(Some(part))
    }
}

/// Data for a single part
pub(crate) struct PartData {
    data: Bytes,
    part_number: u64,
}

/// The `ReadPart` trait allows for reading data from an `InputStream` and packaging the raw
/// data into `PartData` which carries additional metadata needed for uploading a part.
pub(crate) trait ReadPart {
    /// Request the next "part" of data.
    ///
    /// When there is no more data readers should return `Ok(None)`.
    /// NOTE: Implementations are allowed to return data in any order and consumers are
    /// expected to order data by the part number.
    async fn next_part(&self) -> Result<Option<PartData>, io::Error>;
}

#[cfg(test)]
mod test {
    use crate::io::part_reader::{PartReaderBuilder, ReadPart};
    use crate::io::InputStream;
    use bytes::{Buf, Bytes};

    #[tokio::test]
    async fn test_bytes_part_reader() {
        let data = Bytes::from("a lep is a ball, a tay is a hammer, a flix is a comb");
        let stream = InputStream::from(data.clone());
        let expected = data.chunks(5).collect::<Vec<_>>();
        let reader = PartReaderBuilder::new().part_size(5).stream(stream).build();

        let mut parts = Vec::new();
        let mut expected_part_number = 1;
        while let Some(part) = reader.next_part().await.unwrap() {
            assert_eq!(expected_part_number, part.part_number);
            expected_part_number += 1;
            parts.push(part);
        }

        let actual = parts.iter().map(|p| p.data.chunk()).collect::<Vec<_>>();

        assert_eq!(expected, actual);
    }
}
