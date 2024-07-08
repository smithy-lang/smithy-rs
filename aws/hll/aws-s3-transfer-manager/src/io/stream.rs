use std::default::Default;
use std::path::PathBuf;

use bytes::Bytes;

use crate::types::SizeHint;

/// Source of binary data.
///
/// `InputStream` wraps a stream of data for ease of use.
#[derive(Debug)]
pub struct InputStream {
    pub(super) inner: RawInputStream,
}

impl InputStream {
    /// Create a new `InputStream` from a static byte slice
    pub fn from_static(bytes: &'static [u8]) -> Self {
        let inner = RawInputStream::Buf(bytes.into());
        Self { inner }
    }

    // FIXME - we really don't want size hint, we want streams with known content size I think or else we can't respect 10K max part size,
    //         I suppose we can make that an error for the time being and keep it open to unbounded streams if possible later...
    /// Return the bounds on the remaining length of the `InputStream`
    pub fn size_hint(&self) -> SizeHint {
        self.inner.size_hint()
    }

    // pub fn read_from() -> crate::io::FsBuilder
}

#[derive(Debug)]
pub(super) enum RawInputStream {
    /// In-memory buffer to read from
    Buf(Bytes),
    /// File based input
    // FIXME - replace with PathBody
    Fs(PathBuf),
    // Dyn(Box<dyn io::Read>),
}

impl RawInputStream {
    pub(super) fn size_hint(&self) -> SizeHint {
        // match self {
        //     Inner::Buf(bytes) => SizeHint::exact(bytes.remaining() as u64),
        //     // Inner::Fs(path) => SizeHint::exact(path.)
        //     // Inner::Dyn(st) => st.
        // }
        unimplemented!()
    }

    // fn into_part_reader(self) -> PartReader {
    //     unimplemented!()
    // }
}

impl Default for InputStream {
    fn default() -> Self {
        Self {
            inner: RawInputStream::Buf(Bytes::default()),
        }
    }
}

impl From<Bytes> for InputStream {
    fn from(value: Bytes) -> Self {
        Self {
            inner: RawInputStream::Buf(value),
        }
    }
}

impl From<Vec<u8>> for InputStream {
    fn from(value: Vec<u8>) -> Self {
        Self::from(Bytes::from(value))
    }
}

// impl<T> TryFrom<T> for InputStream
// where
//     T: AsRef<std::path::Path>,
// {
//     type Error = ();
//     fn try_from(value: T) -> Result<Self, Self::Error> {
//         todo!()
//     }
// }
