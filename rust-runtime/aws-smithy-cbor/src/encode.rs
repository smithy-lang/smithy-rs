use aws_smithy_types::{Blob, DateTime};

/// Macro for delegating method calls to the encoder.
///
/// This macro generates wrapper methods for calling specific encoder methods on the encoder
/// and returning a mutable reference to self for method chaining.
///
/// # Example
///
/// ```
/// delegate_method! {
///     /// Wrapper method for encoding method `encode_str` on the encoder.
///     encode_str_wrapper => encode_str(data: &str);
///     /// Wrapper method for encoding method `encode_int` on the encoder.
///     encode_int_wrapper => encode_int(value: i32);
/// }
/// ```
macro_rules! delegate_method {
    ($($(#[$meta:meta])* $wrapper_name:ident => $encoder_name:ident($($param_name:ident : $param_type:ty),*);)+) => {
        $(
            pub fn $wrapper_name(&mut self, $($param_name: $param_type),*) -> &mut Self {
                self.encoder.$encoder_name($($param_name)*).expect(INFALLIBLE_WRITE);
                self
            }
        )+
    };
}

#[derive(Debug, Clone)]
pub struct Encoder {
    encoder: minicbor::Encoder<Vec<u8>>,
}

// TODO docs
const INFALLIBLE_WRITE: &str = "write failed";

impl Encoder {
    pub fn new(writer: Vec<u8>) -> Self {
        Self {
            encoder: minicbor::Encoder::new(writer),
        }
    }

    delegate_method! {
        /// Writes a fixed length array of given length.
        array => array(len: u64);
        /// Used when we know the size in advance, i.e.:
        /// - when a struct has all non-`Option`al members.
        /// - when serializing `union` shapes (they can only have one member set).
        map => map(len: u64);
        /// Used when it's not cheap to calculate the size, i.e. when the struct has one or more
        /// `Option`al members.
        begin_map => begin_map();
        /// Writes a definite length string.
        str => str(x: &str);
        /// Writes a boolean value.
        boolean => bool(x: bool);
        /// Writes a byte value.
        byte => i8(x: i8);
        /// Writes a short value.
        short => i16(x: i16);
        /// Writes an integer value.
        integer => i32(x: i32);
        /// Writes an long value.
        long => i64(x: i64);
        /// Writes an float value.
        float => f32(x: f32);
        /// Writes an double value.
        double => f64(x: f64);
        /// Writes a null tag.
        null => null();
        /// Writes an end tag.
        end => end();
    }

    pub fn blob(&mut self, x: &Blob) -> &mut Self {
        self.encoder.bytes(x.as_ref()).expect(INFALLIBLE_WRITE);
        self
    }

    pub fn timestamp(&mut self, x: &DateTime) -> &mut Self {
        self.encoder
            .tag(minicbor::data::Tag::Timestamp)
            .expect(INFALLIBLE_WRITE);
        self.encoder.f64(x.as_secs_f64()).expect(INFALLIBLE_WRITE);
        self
    }

    pub fn into_writer(self) -> Vec<u8> {
        self.encoder.into_writer()
    }
}
