use aws_smithy_http::byte_stream::ByteStream;
use aws_smithy_http::operation;
use aws_smithy_http::property_bag::PropertyBag;

/// A container for the data currently available to an interceptor.
pub struct InterceptorContext {
    property_bag: PropertyBag,
}

impl InterceptorContext {
    /// Retrieve the modeled request for the operation being invoked.
    pub fn request() -> operation::Request {
        todo!()
    }

    /// Retrieve the transmittable request for the operation being invoked.
    /// This will only be available once request marshalling has completed.
    pub fn transmit_request() -> http::Request<ByteStream> {
        todo!()
    }

    /// Retrieve the response to the transmittable request for the operation
    /// being invoked. This will only be available once transmission has
    /// completed.
    pub fn transmit_response() -> http::Response<ByteStream> {
        todo!()
    }

    /// Retrieve the response to the customer. This will only be available
    /// once the [transmit_response] has been unmarshalled or the
    /// attempt/execution has failed.
    pub fn response() -> operation::Response {
        todo!()
    }

    /// Add an attribute to this interceptor context, which will be made
    /// available to all other interceptors or hooks that are called for this
    /// execution.
    ///
    /// If the attribute is already set, this overrides the existing value.
    pub fn put_attribute<T: Send + Sync + 'static>(&mut self, value: T) {
        let _ = self.property_bag.insert(value);
    }

    /// Retrieve an attribute previously added to this interceptor context.
    pub fn get_attribute<T: Send + Sync + 'static>(&self) -> Option<&T> {
        self.property_bag.get()
    }
}
