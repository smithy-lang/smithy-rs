use crate::body::SdkBody;
use crate::property_bag::PropertyBag;

pub struct Operation<H> {
    request: Request,
    response_handler: Box<H>,
}

impl<H> Operation<H> {
    pub fn into_request_response(self) -> (Request, Box<H>) {
        (self.request, self.response_handler)
    }

    pub fn new(request: Request, response_handler: impl Into<Box<H>>) -> Self {
        Operation {
            request,
            response_handler: response_handler.into(),
        }
    }
}

pub struct Request {
    base: http::Request<SdkBody>,
    configuration: PropertyBag,
}

impl Request {
    pub fn new(base: http::Request<SdkBody>) -> Self {
        Request {
            base,
            configuration: PropertyBag::new(),
        }
    }

    pub fn into_parts(self) -> (http::Request<SdkBody>, PropertyBag) {
        (self.base, self.configuration)
    }
}
