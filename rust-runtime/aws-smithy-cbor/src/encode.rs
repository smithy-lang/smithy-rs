use aws_smithy_types::{Blob, DateTime};

#[derive(Debug, Clone)]
pub struct Encoder {
    encoder: minicbor::Encoder<Vec<u8>>,
}

// TODO docs
static INFALLIBLE_WRITE: &str = "write failed";

impl Encoder {
    pub fn new(writer: Vec<u8>) -> Self {
        Self {
            encoder: minicbor::Encoder::new(writer),
        }
    }

    // TODO Generate using a macro?

    pub fn array(&mut self, len: u64) -> &mut Self {
        self.encoder.array(len).expect(INFALLIBLE_WRITE);
        self
    }

    // Used when we know the size in advance, i.e.:
    //
    // - when a struct has all non-`Option`al members.
    // - when serializing `union` shapes (they can only have one member set).
    pub fn map(&mut self, len: u64) -> &mut Self {
        self.encoder.map(len).expect(INFALLIBLE_WRITE);
        self
    }

    // Used when it's not cheap to calculate the size, i.e. when the struct has one or more
    // `Option`al members.
    pub fn begin_map(&mut self) -> &mut Self {
        self.encoder.begin_map().expect(INFALLIBLE_WRITE);
        self
    }

    pub fn str(&mut self, x: &str) -> &mut Self {
        self.encoder.str(x).expect(INFALLIBLE_WRITE);
        self
    }

    pub fn boolean(&mut self, x: bool) -> &mut Self {
        self.encoder.bool(x).expect(INFALLIBLE_WRITE);
        self
    }

    pub fn byte(&mut self, x: i8) -> &mut Self {
        self.encoder.i8(x).expect(INFALLIBLE_WRITE);
        self
    }

    pub fn short(&mut self, x: i16) -> &mut Self {
        self.encoder.i16(x).expect(INFALLIBLE_WRITE);
        self
    }

    pub fn integer(&mut self, x: i32) -> &mut Self {
        self.encoder.i32(x).expect(INFALLIBLE_WRITE);
        self
    }

    pub fn long(&mut self, x: i64) -> &mut Self {
        self.encoder.i64(x).expect(INFALLIBLE_WRITE);
        self
    }

    pub fn float(&mut self, x: f32) -> &mut Self {
        self.encoder.f32(x).expect(INFALLIBLE_WRITE);
        self
    }

    pub fn double(&mut self, x: f64) -> &mut Self {
        self.encoder.f64(x).expect(INFALLIBLE_WRITE);
        self
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

    pub fn null(&mut self) -> &mut Self {
        self.encoder.null().expect(INFALLIBLE_WRITE);
        self
    }

    pub fn end(&mut self) -> &mut Self {
        self.encoder.end().expect(INFALLIBLE_WRITE);
        self
    }

    pub fn into_writer(self) -> Vec<u8> {
        self.encoder.into_writer()
    }
}
