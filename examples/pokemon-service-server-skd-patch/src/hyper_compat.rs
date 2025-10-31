// TODO: PATCH - Hyper 1.x compatibility module

use crate::types::ByteStream;

// Newtype wrapper to work around orphan rules
#[derive(Debug)]
pub struct HyperIncoming(pub hyper::body::Incoming);

impl From<HyperIncoming> for ByteStream {
    fn from(wrapper: HyperIncoming) -> Self {
        ByteStream::from_body_1_x(wrapper.0)
    }
}

impl From<hyper::body::Incoming> for HyperIncoming {
    fn from(body: hyper::body::Incoming) -> Self {
        HyperIncoming(body)
    }
}

// Provide a blanket implementation for hyper::body::Incoming -> ByteStream
// by going through our newtype
impl From<hyper::body::Incoming> for ByteStream {
    fn from(body: hyper::body::Incoming) -> Self {
        HyperIncoming::from(body).into()
    }
}
