//! This file showcases a rather minimal model plugin that is agnostic over the operation that it
//! is applied to.
//!
//! It is interesting because it is not trivial to figure out how to write one. As the
//! documentation for [`aws_smithy_http_server::plugin::ModelMarker`] calls out, most model
//! plugins' implementation are _operation-specific_, which are simpler.

use std::{marker::PhantomData, pin::Pin};

use aws_smithy_http_server::{
    body::BoxBody,
    operation::OperationShape,
    plugin::{ModelMarker, Plugin},
};
use pokemon_service_server_sdk::server::response::IntoResponse;
use tower::Service;

pub struct AuthorizationPlugin {
    // Private so that users are forced to use the `new` constructor.
    _private: (),
}

impl AuthorizationPlugin {
    pub fn new() -> Self {
        Self { _private: () }
    }
}

impl<Ser, Op, T> Plugin<Ser, Op, T> for AuthorizationPlugin {
    type Output = AuthorizeService<Op, T>;

    fn apply(&self, input: T) -> Self::Output {
        AuthorizeService {
            inner: input,
            _operation: PhantomData,
        }
    }
}

impl ModelMarker for AuthorizationPlugin {}

pub struct AuthorizeService<Op, S> {
    inner: S,

    _operation: PhantomData<Op>,
}

impl<Op, S> Clone for AuthorizeService<Op, S>
where
    S: Clone,
{
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            _operation: PhantomData,
        }
    }
}

/// The error returned by [`AuthorizeService`].
pub enum AuthorizeServiceError<E> {
    /// Authorization was succesful, but the inner service yielded an error.
    InnerServiceError(E),
    /// Authorization was not successful.
    AuthorizeError { message: String },
}

// Only the _outermost_ model plugin needs to apply a `Service` whose error type implements
// `IntoResponse` for the protocol the service uses (this requirement comes from the `Service`
// implementation of [`aws_smithy_http_server::operation::Upgrade`]). So if the model plugin is
// meant to be applied in any position, and to any Smithy service, one should implement
// `IntoResponse` for all protocols.
//
// Having model plugins apply a `Service` that has a `Service::Response` type or a `Service::Error`
// type that is different from those returned by the inner service hence diminishes the reusability
// of the plugin because it makes the plugin less composable. Most plugins should instead work with
// the inner service's types, and _at most_ require that those be `Op::Input` and `Op::Error`, for
// maximum composability:
//
// ```
// ...
// where
//     S: Service<(Op::Input, ($($var,)*)), Error = Op::Error>
//     ...
// {
//     type Response = S::Response;
//     type Error = S::Error;
//     type Future = Pin<Box<dyn Future<Output = Result<S::Response, S::Error>> + Send>>;
// }
//
// ```
//
// This plugin still exemplifies how changing a type can be done to make it more interesting.

impl<P, E> IntoResponse<P> for AuthorizeServiceError<E>
where
    E: IntoResponse<P>,
{
    fn into_response(self) -> http::Response<BoxBody> {
        match self {
            AuthorizeServiceError::InnerServiceError(e) => e.into_response(),
            AuthorizeServiceError::AuthorizeError { message } => http::Response::builder()
                .status(http::StatusCode::UNAUTHORIZED)
                .body(aws_smithy_http_server::body::to_boxed(message))
                .expect("attempted to build an invalid HTTP response; please file a bug report"),
        }
    }
}

macro_rules! impl_service {
    ($($var:ident),*) => {
        impl<S, Op, $($var,)*> Service<(Op::Input, ($($var,)*))> for AuthorizeService<Op, S>
        where
            Op: OperationShape,
            S: Service<(Op::Input, ($($var,)*)), Error = Op::Error> + Clone + Send + 'static,
            S::Future: Send,
            Op::Input: Send + 'static,
            $($var: Send + 'static,)*
        {
            type Response = S::Response;
            type Error = AuthorizeServiceError<Op::Error>;
            type Future =
                Pin<Box<dyn std::future::Future<Output = Result<S::Response, Self::Error>> + Send>>;

            fn poll_ready(
                &mut self,
                cx: &mut std::task::Context<'_>,
            ) -> std::task::Poll<Result<(), Self::Error>> {
                self.inner
                    .poll_ready(cx)
                    .map_err(|e| Self::Error::InnerServiceError(e))
            }

            fn call(&mut self, req: (Op::Input, ($($var,)*))) -> Self::Future {
                let (input, exts) = req;

                // Replacing the service is necessary to avoid readiness problems.
                // https://docs.rs/tower/latest/tower/trait.Service.html#be-careful-when-cloning-inner-services
                let service = self.inner.clone();
                let mut service = std::mem::replace(&mut self.inner, service);

                let fut = async move {
                    // You could imagine that here we would make a more complex async call to perform the
                    // actual authorization.
                    // We would likely need to add bounds on `Op::Input`, `Op::Error`, or on data
                    // types the plugin would hold if we wanted to do anything useful.
                    let is_authorized = true;
                    if !is_authorized {
                        return Err(Self::Error::AuthorizeError {
                            message: "Not authorized!".to_owned(),
                        });
                    }

                    service
                        .call((input, exts))
                        .await
                        .map_err(|e| Self::Error::InnerServiceError(e))
                };
                Box::pin(fut)
            }
        }
    };
}

// If we want our plugin to be as reusable as possible, the service it applies should work with
// inner services (i.e. operation handlers) that take a variable number of parameters. A Rust macro
// is helpful in providing those implementations concisely.

impl_service!();
impl_service!(T1);
impl_service!(T1, T2);
impl_service!(T1, T2, T3);
impl_service!(T1, T2, T3, T4);
impl_service!(T1, T2, T3, T4, T5);
impl_service!(T1, T2, T3, T4, T5, T6);
impl_service!(T1, T2, T3, T4, T5, T6, T7);
