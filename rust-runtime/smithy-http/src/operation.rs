use crate::body::SdkBody;
use crate::property_bag::PropertyBag;
use std::borrow::Cow;
use std::cell::{Ref, RefCell, RefMut};
use std::rc::Rc;

pub struct Metadata {
    operation: Cow<'static, str>,
    service: Cow<'static, str>,
}

impl Metadata {
    pub fn name(&self) -> &str {
        &self.operation
    }

    pub fn service(&self) -> &str {
        &self.service
    }

    pub fn new(
        operation: impl Into<Cow<'static, str>>,
        service: impl Into<Cow<'static, str>>,
    ) -> Self {
        Metadata {
            operation: operation.into(),
            service: service.into(),
        }
    }
}

#[non_exhaustive]
pub struct Parts<H, R> {
    pub response_handler: H,
    pub retry_policy: R,
    pub metadata: Option<Metadata>,
}

pub struct Operation<H, R> {
    request: Request,
    parts: Parts<H, R>,
}

impl<H, R> Operation<H, R> {
    pub fn into_request_response(self) -> (Request, Parts<H, R>) {
        (self.request, self.parts)
    }

    pub fn with_metadata(mut self, metadata: Metadata) -> Self {
        self.parts.metadata = Some(metadata);
        self
    }
}

impl<H> Operation<H, ()> {
    pub fn new(request: Request, response_handler: H) -> Self {
        Operation {
            request,
            parts: Parts {
                response_handler,
                retry_policy: (),
                metadata: None,
            },
        }
    }
}

#[derive(Debug)]
pub struct Request {
    /// The underlying HTTP Request
    inner: http::Request<SdkBody>,

    /// Property bag of configuration options
    ///
    /// Middleware can read and write from the property bag and use its
    /// contents to augment the request (see `Request::augment`)
    ///
    /// configuration is stored in an `Rc<RefCell>>` to facilitate cloning requests during retries
    /// We should consider if this should instead be an `Arc<Mutex>`. I'm not aware of times where
    /// we'd need to modify the request concurrently, but perhaps such a thing may some day exist.
    configuration: Rc<RefCell<PropertyBag>>,
}

impl Request {
    pub fn new(base: http::Request<SdkBody>) -> Self {
        Request {
            inner: base,
            configuration: Rc::new(RefCell::new(PropertyBag::new())),
        }
    }

    pub fn augment<T>(
        self,
        f: impl FnOnce(http::Request<SdkBody>, &mut PropertyBag) -> Result<http::Request<SdkBody>, T>,
    ) -> Result<Request, T> {
        let inner = {
            let configuration: &mut PropertyBag = &mut self.configuration.as_ref().borrow_mut();
            f(self.inner, configuration)?
        };
        Ok(Request {
            inner,
            configuration: self.configuration,
        })
    }

    pub fn config_mut(&mut self) -> RefMut<'_, PropertyBag> {
        self.configuration.as_ref().borrow_mut()
    }

    pub fn config(&self) -> Ref<'_, PropertyBag> {
        self.configuration.as_ref().borrow()
    }

    pub fn try_clone(&self) -> Option<Request> {
        let cloned_body = self.inner.body().try_clone()?;
        let mut cloned_request = http::Request::builder()
            .uri(self.inner.uri().clone())
            .method(self.inner.method());
        *cloned_request
            .headers_mut()
            .expect("builder has not been modified, headers must be valid") =
            self.inner.headers().clone();
        let inner = cloned_request
            .body(cloned_body)
            .expect("a clone of a valid request should be a valid request");
        Some(Request {
            inner,
            configuration: self.configuration.clone(),
        })
    }

    pub fn into_parts(self) -> (http::Request<SdkBody>, Rc<RefCell<PropertyBag>>) {
        (self.inner, self.configuration)
    }
}

#[cfg(test)]
mod test {
    use crate::body::SdkBody;
    use crate::operation::Request;
    use http::header::{AUTHORIZATION, CONTENT_LENGTH};
    use http::Uri;

    #[test]
    fn try_clone_clones_all_data() {
        let mut request = Request::new(
            http::Request::builder()
                .uri(Uri::from_static("http://www.amazon.com"))
                .method("POST")
                .header(CONTENT_LENGTH, 456)
                .header(AUTHORIZATION, "Token: hello")
                .body(SdkBody::from("hello world!"))
                .expect("valid request"),
        );
        request.config_mut().insert("hello");
        let cloned = request.try_clone().expect("request is cloneable");

        let (request, config) = cloned.into_parts();
        assert_eq!(request.uri(), &Uri::from_static("http://www.amazon.com"));
        assert_eq!(request.method(), "POST");
        assert_eq!(request.headers().len(), 2);
        assert_eq!(
            request.headers().get(AUTHORIZATION).unwrap(),
            "Token: hello"
        );
        assert_eq!(request.headers().get(CONTENT_LENGTH).unwrap(), "456");
        assert_eq!(request.body().bytes().unwrap(), "hello world!".as_bytes());
        assert_eq!(config.as_ref().borrow().get::<&str>(), Some(&"hello"));
    }
}
