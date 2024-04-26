use crate::http::RequestCompressor;
use crate::{Compression, CompressionOptions};
use aws_smithy_runtime_api::box_error::BoxError;
use flate2::write::GzEncoder;
use http::HeaderValue;
use std::fmt;
use std::io::prelude::*;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct Gzip {
    compression: flate2::Compression,
}

impl Gzip {
    fn compress_bytes(&self, bytes: &[u8], writer: impl Write) -> Result<(), Error> {
        let mut encoder = GzEncoder::new(writer, self.compression);
        encoder.write_all(bytes)?;
        encoder.try_finish()?;

        Ok(())
    }
}

impl Compression for Gzip {
    fn compress_bytes(&mut self, bytes: &[u8], writer: &mut dyn Write) -> Result<(), BoxError> {
        Gzip::compress_bytes(self, bytes, writer).map_err(Into::into)
    }
}

impl RequestCompressor for Gzip {
    fn header_value(self: Box<Self>) -> HeaderValue {
        HeaderValue::from_static("gzip")
    }
}

// TODO Is there a better way to define these From impls?
impl From<&CompressionOptions> for Gzip {
    fn from(options: &CompressionOptions) -> Self {
        Gzip {
            compression: flate2::Compression::new(options.level),
        }
    }
}

impl From<CompressionOptions> for Gzip {
    fn from(options: CompressionOptions) -> Self {
        Gzip {
            compression: flate2::Compression::new(options.level),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
struct Error {
    message: String,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "compression failed: {}", self.message)
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(err: std::io::Error) -> Self {
        Error {
            message: err.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Gzip;
    use crate::CompressionOptions;
    use flate2::read::GzDecoder;
    use pretty_assertions::assert_eq;
    use std::io::Read;

    fn gettysburg_address() -> &'static [u8] {
        include_bytes!("../test-data/gettysburg_address.txt")
    }

    fn gzip_compressed_gettysburg_address() -> &'static [u8] {
        // This file was compressed using Apple gzip with the following command:
        // `gzip -k gettysburg_address.txt -6`
        include_bytes!("../test-data/gettysburg_address.txt.gz")
    }

    #[test]
    fn test_gzip_compression() {
        let gzip = Gzip::from(&CompressionOptions::default());
        let mut compressed_output = Vec::new();
        gzip.compress_bytes(gettysburg_address(), &mut compressed_output)
            .expect("compression succeeds");

        let uncompressed_expected = {
            let mut s = String::new();
            GzDecoder::new(gzip_compressed_gettysburg_address())
                .read_to_string(&mut s)
                .unwrap();
            s
        };
        let uncompressed_actual = {
            let mut s = String::new();
            GzDecoder::new(&compressed_output[..])
                .read_to_string(&mut s)
                .unwrap();
            s
        };

        assert_eq!(uncompressed_expected, uncompressed_actual);
    }
}
