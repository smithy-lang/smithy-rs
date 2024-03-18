use std::borrow::Cow;

use aws_smithy_types::{byte_stream::error, Blob, DateTime};
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

    pub fn str(&mut self) -> Result<Cow<'b, str>, DeserializeError> {
        let mut iter = self.decoder.str_iter().map_err(DeserializeError::new)?;
        let head = iter.next();

        let decoded_string = match head {
            None => Cow::Borrowed(""),
            Some(head) => {
                let head = head.map_err(DeserializeError::new)?;
                if let Some(next) = iter.next() {
                    let next = next.map_err(DeserializeError::new)?;
                    let mut concatenated_string = String::from(head);
                    concatenated_string.push_str(next);
                    for i in iter {
                        concatenated_string.push_str(i.map_err(DeserializeError::new)?);
                    }
                    Cow::Owned(concatenated_string)
                } else {
                    Cow::Borrowed(head)
                }
            }
        };

        Ok(decoded_string)
    }

    pub fn str_alt(&mut self) -> Result<Cow<'b, str>, DeserializeError> {
        match self.decoder.str() {
            Ok(str_value) => Ok(Cow::Borrowed(str_value)),
            Err(e) if e.is_type_mismatch() => Ok(Cow::Owned(self.string()?)),
            Err(e) => Err(DeserializeError::new(e)),
        }
    }

    pub fn string(&mut self) -> Result<String, DeserializeError> {
        let mut iter = self.decoder.str_iter().map_err(DeserializeError::new)?;
        let head = iter.next();

        let decoded_string = match head {
            None => String::from(""),
            Some(head) => {
                let mut head = String::from(head.map_err(DeserializeError::new)?);
                for i in iter {
                    head.push_str(i.map_err(DeserializeError::new)?);
                }
                head
            }
        };

        Ok(decoded_string)
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
    fn test_definite_str_is_cow_borrowed() {
        // Definite length key `thisIsAKey`.
        let definite_bytes = [
            0x6a, 0x74, 0x68, 0x69, 0x73, 0x49, 0x73, 0x41, 0x4b, 0x65, 0x79,
        ];
        let mut decoder = Decoder::new(&definite_bytes);
        let member = decoder.str().expect(
            "
            could not decode str",
        );
        assert_eq!(member, "thisIsAKey");
        assert!(matches!(member, std::borrow::Cow::Borrowed(_)));
    }

    #[test]
    fn test_indefinite_str_is_cow_owned() {
        // Indefinite length key `this`, `Is`, `A` and `Key`.
        let indefinite_bytes = [
            0x7f, 0x64, 0x74, 0x68, 0x69, 0x73, 0x62, 0x49, 0x73, 0x61, 0x41, 0x63, 0x4b, 0x65,
            0x79, 0xff,
        ];
        let mut decoder = Decoder::new(&indefinite_bytes);
        let member = decoder.str().expect(
            "
            could not decode str",
        );
        assert_eq!(member, "thisIsAKey");
        assert!(matches!(member, std::borrow::Cow::Owned(_)));
    }

    #[test]
    fn test_empty_str_works() {
        let bytes = [0x60];
        let mut decoder = Decoder::new(&bytes);
        let member = decoder.str().expect(
            "
            could not decode str",
        );
        assert_eq!(member, "");
    }
}
