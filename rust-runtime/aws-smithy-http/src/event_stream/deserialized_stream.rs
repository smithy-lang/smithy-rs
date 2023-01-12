use receiver::{RawMessage, Receiver};

use super::*;
use aws_smithy_eventstream::frame::UnmarshallMessage;
use std::{fmt::Debug, marker::PhantomData};
/// This data is used to fill a field when the users try to deserialize data that has a Receiver in one of the field.
pub struct DeserializedReceiverStream<T, E>(PhantomData<(T, E)>);
impl<T, E> Debug for DeserializedReceiverStream<T, E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("DeserializedStream")
    }
}
impl<T, E> DeserializedReceiverStream<T, E>
where
    T: std::fmt::Debug,
    E: std::fmt::Debug,
{
    /// default value
    pub fn create() -> impl UnmarshallMessage<Output = T, Error = E> {
        Self(PhantomData::<(T, E)>)
    }
}

impl<T, E> UnmarshallMessage for DeserializedReceiverStream<T, E>
where
    T: Debug,
    E: Debug,
{
    type Error = E;
    type Output = T;
    fn unmarshall(
        &self,
        _: &aws_smithy_eventstream::frame::Message,
    ) -> Result<
        aws_smithy_eventstream::frame::UnmarshalledMessage<Self::Output, Self::Error>,
        aws_smithy_eventstream::error::Error,
    > {
        Err(aws_smithy_eventstream::error::Error::deserialized_stream())
    }
}

/// Error returned from Deserialized Stream.
#[derive(Debug)]
pub struct DeserializedStreamError;

impl StdError for DeserializedStreamError {}
impl std::fmt::Display for DeserializedStreamError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Stream was deserialized")
    }
}
