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

    // More than one union variant was detected: `unexpected_type` was unexpected.
    pub fn unexpected_union_variant(unexpected_type: Type, at: usize) -> Self {
        Self {
            _inner: Error::type_mismatch(unexpected_type.into_minicbor_type())
                .with_message("encountered unexpected union variant; expected end of union")
                .at(at),
        }
    }

    // More than one union variant was detected, but we never even got to parse the first one.
    pub fn mixed_union_variants(at: usize) -> Self {
        Self {
            _inner: Error::message("encountered mixed variants in union; expected end of union")
                .at(at),
        }
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

    // TODO An API to support both definite and indefinite strings could be:
    //
    //     pub fn str(&mut self, cap: u64) -> Result<&'b str, DeserializeError> {
    //
    pub fn str(&mut self) -> Result<&'b str, DeserializeError> {
        self.decoder.str().map_err(DeserializeError::new)
    }

    // TODO Support indefinite text strings.
    pub fn string(&mut self) -> Result<String, DeserializeError> {
        self.decoder
            .str()
            .map(String::from) // This allocates.
            .map_err(DeserializeError::new)
    }

    // TODO Support indefinite byte strings.
    pub fn blob(&mut self) -> Result<Blob, DeserializeError> {
        self.decoder
            .bytes()
            .map(Blob::new) // This allocates.
            .map_err(DeserializeError::new)
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
