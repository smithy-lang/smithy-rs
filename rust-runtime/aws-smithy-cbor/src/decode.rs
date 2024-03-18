use aws_smithy_types::{Blob, DateTime};
use minicbor::decode::Error;

use crate::data::Type;

#[derive(Debug, Clone)]
pub struct Decoder<'b> {
    decoder: minicbor::Decoder<'b>,
}

#[derive(Debug)]
pub struct DeserializeError {
    #[allow(dead_code)]
    _inner: Error,
}

impl std::fmt::Display for DeserializeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // TODO? Is this good enough?
        self._inner.fmt(f)
    }
}

impl std::error::Error for DeserializeError {}

impl DeserializeError {
    fn new(inner: Error) -> Self {
        Self { _inner: inner }
    }

    /// More than one union variant was detected: `unexpected_type` was unexpected.
    pub fn unexpected_union_variant(unexpected_type: Type, at: usize) -> Self {
        Self {
            _inner: Error::type_mismatch(unexpected_type.into_minicbor_type())
                .with_message("encountered unexpected union variant; expected end of union")
                .at(at),
        }
    }

    /// More than one union variant was detected, but we never even got to parse the first one.
    pub fn mixed_union_variants(at: usize) -> Self {
        Self {
            _inner: Error::message("encountered mixed variants in union; expected end of union")
                .at(at),
        }
    }

    /// An unexpected type was encountered.
    // We handle this one when decoding sparse collections: we have to expect either a `null` or an
    // item, so we try decoding both.
    pub fn is_type_mismatch(&self) -> bool {
        self._inner.is_type_mismatch()
    }
}

impl<'b> Decoder<'b> {
    pub fn new(bytes: &'b [u8]) -> Self {
        Self {
            decoder: minicbor::Decoder::new(bytes),
        }
    }

    pub fn map(&mut self) -> Result<Option<u64>, DeserializeError> {
        self.decoder.map().map_err(DeserializeError::new)
    }

    pub fn datatype(&self) -> Result<Type, DeserializeError> {
        self.decoder
            .datatype()
            .map(Type::new)
            .map_err(DeserializeError::new)
    }

    pub fn skip(&mut self) -> Result<(), DeserializeError> {
        self.decoder.skip().map_err(DeserializeError::new)
    }

    // Only definite length strings can be converted using `str` because indefinite
    // length strings have an identifier in between each chunk.
    pub fn str(&mut self) -> Result<&'b str, DeserializeError> {
        self.decoder.str().map_err(DeserializeError::new)
    }

    pub fn string(&mut self) -> Result<String, DeserializeError> {
        let iter = self.decoder.str_iter().map_err(DeserializeError::new)?;
        let parts: Vec<&str> = iter
            .collect::<Result<_, _>>()
            .map_err(DeserializeError::new)?;

        Ok(if parts.len() == 1 {
            parts[0].into() // Directly convert &str to String if there's only one part.
        } else {
            parts.concat() // Concatenate all parts into a single String.
        })
    }

    pub fn blob(&mut self) -> Result<Blob, DeserializeError> {
        let iter = self.decoder.bytes_iter().map_err(DeserializeError::new)?;
        let parts: Vec<&[u8]> = iter
            .collect::<Result<_, _>>()
            .map_err(DeserializeError::new)?;

        Ok(if parts.len() == 1 {
            Blob::new(parts[0]) // Directly convert &[u8] to Blob if there's only one part.
        } else {
            Blob::new(parts.concat()) // Concatenate all parts into a single Blob.
        })
    }

    pub fn boolean(&mut self) -> Result<bool, DeserializeError> {
        self.decoder.bool().map_err(DeserializeError::new)
    }

    pub fn position(&self) -> usize {
        self.decoder.position()
    }

    pub fn byte(&mut self) -> Result<i8, DeserializeError> {
        self.decoder.i8().map_err(DeserializeError::new)
    }

    pub fn short(&mut self) -> Result<i16, DeserializeError> {
        self.decoder.i16().map_err(DeserializeError::new)
    }

    pub fn integer(&mut self) -> Result<i32, DeserializeError> {
        self.decoder.i32().map_err(DeserializeError::new)
    }

    pub fn long(&mut self) -> Result<i64, DeserializeError> {
        self.decoder.i64().map_err(DeserializeError::new)
    }

    pub fn float(&mut self) -> Result<f32, DeserializeError> {
        self.decoder.f32().map_err(DeserializeError::new)
    }

    pub fn double(&mut self) -> Result<f64, DeserializeError> {
        self.decoder.f64().map_err(DeserializeError::new)
    }

    pub fn timestamp(&mut self) -> Result<DateTime, DeserializeError> {
        let tag = self.decoder.tag().map_err(DeserializeError::new)?;

        if !matches!(tag, minicbor::data::Tag::Timestamp) {
            // TODO
            todo!()
        } else {
            let epoch_seconds = self.decoder.f64().map_err(DeserializeError::new)?;
            Ok(DateTime::from_secs_f64(epoch_seconds))
        }
    }

    pub fn null(&mut self) -> Result<(), DeserializeError> {
        self.decoder.null().map_err(DeserializeError::new)
    }

    pub fn list(&mut self) -> Result<Option<u64>, DeserializeError> {
        self.decoder.array().map_err(DeserializeError::new)
    }
}

#[derive(Debug)]
pub struct ArrayIter<'a, 'b, T> {
    inner: minicbor::decode::ArrayIter<'a, 'b, T>,
}

impl<'a, 'b, T: minicbor::Decode<'b, ()>> Iterator for ArrayIter<'a, 'b, T> {
    type Item = Result<T, DeserializeError>;

    fn next(&mut self) -> Option<Self::Item> {
        self.inner
            .next()
            .map(|opt| opt.map_err(DeserializeError::new))
    }
}

#[derive(Debug)]
pub struct MapIter<'a, 'b, K, V> {
    inner: minicbor::decode::MapIter<'a, 'b, K, V>,
}

impl<'a, 'b, K, V> Iterator for MapIter<'a, 'b, K, V>
where
    K: minicbor::Decode<'b, ()>,
    V: minicbor::Decode<'b, ()>,
{
    type Item = Result<(K, V), DeserializeError>;

    fn next(&mut self) -> Option<Self::Item> {
        self.inner
            .next()
            .map(|opt| opt.map_err(DeserializeError::new))
    }
}

#[cfg(test)]
mod tests {
    use crate::Decoder;

    #[test]
    fn test_str_with_direct_indirect() {
        let bytes = [
            0x7f, 0x66, 0x64, 0x6f, 0x75, 0x62, 0x6c, 0x65, 0x65, 0x56, 0x61, 0x6c, 0x75, 0x65,
            0xff,
        ];
        let mut decoder = Decoder::new(&bytes);
        let member = decoder.str().expect(
            "
            could not decode str",
        );
        assert_eq!(member, "doubleValue");
    }
}
