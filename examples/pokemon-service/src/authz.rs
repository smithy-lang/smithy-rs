/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! This file showcases a rather minimal model plugin that is agnostic over the operation that it
//! is applied to.
//!
//! It is interesting because it is not trivial to figure out how to write one. As the
//! documentation for [`aws_smithy_http_server::plugin::ModelMarker`] calls out, most model
//! plugins' implementation are _operation-specific_, which are simpler.

use std::{marker::PhantomData, pin::Pin, sync::LazyLock};

use aws_smithy_schema::{
    serde::{SerdeError, SerializableStruct, ShapeSerializer},
    shape_id, Schema, ShapeType, StringTrait, TraitMap,
};

use pokemon_service_server_sdk::server::{
    operation::OperationShape,
    plugin::{ModelMarker, Plugin},
    schema::{HttpModeledError, ModeledError},
};
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

/// `T` is the inner service this plugin is applied to.
/// See the documentation for [`Plugin`] for details.
impl<Ser, Op, T> Plugin<Ser, Op, T> for AuthorizationPlugin {
    type Output = AuthorizeService<Op, T>;

    fn apply(&self, input: T) -> Self::Output {
        AuthorizeService {
            inner: input,
            authorizer: Authorizer::new(),
        }
    }
}

impl ModelMarker for AuthorizationPlugin {}

pub struct AuthorizeService<Op, S> {
    inner: S,
    authorizer: Authorizer<Op>,
}

/// We manually implement `Clone` instead of adding `#[derive(Clone)]` because we don't require
/// `Op` to be cloneable.
impl<Op, S> Clone for AuthorizeService<Op, S>
where
    S: Clone,
{
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            authorizer: self.authorizer.clone(),
        }
    }
}

/// The error returned by [`AuthorizeService`].
#[derive(Debug)]
pub enum AuthorizeServiceError<E> {
    /// Authorization was successful, but the inner service yielded an error.
    InnerServiceError(E),
    /// Authorization was not successful.
    AuthorizeError(AuthorizeError),
}

/// The authorization failure is described as a Smithy error shape. Each selected
/// protocol supplies its own content type, error discriminator, and serialized body.
#[derive(Debug)]
pub struct AuthorizeError {
    pub message: String,
}

static MESSAGE: Schema<'static> = Schema::new_member(
    shape_id!("pokemon_service.authz", "AuthorizeError", "message"),
    ShapeType::String,
    "message",
    0,
);
static ERROR_TRAITS: LazyLock<TraitMap> = LazyLock::new(|| {
    let mut traits = TraitMap::new();
    traits.insert(Box::new(StringTrait::new(
        shape_id!("smithy.api", "error"),
        "client",
    )));
    traits
});
static AUTHORIZE_ERROR: Schema<'static> = Schema::new_struct(
    shape_id!("pokemon_service.authz", "AuthorizeError"),
    ShapeType::Structure,
    &[&MESSAGE],
)
.with_traits(&ERROR_TRAITS);

impl std::fmt::Display for AuthorizeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for AuthorizeError {}

impl SerializableStruct for AuthorizeError {
    fn serialize_members(&self, serializer: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
        serializer.write_string(&MESSAGE, &self.message)
    }
}
impl ModeledError for AuthorizeError {
    fn schema(&self) -> &Schema<'_> {
        &AUTHORIZE_ERROR
    }
}
impl HttpModeledError for AuthorizeError {
    fn status_code(&self) -> u16 {
        401
    }
}

// A wrapper exposes the schema and members of its active error variant. It has
// no knowledge of the selected protocol or how HTTP responses are constructed.
impl<E: HttpModeledError> AuthorizeServiceError<E> {
    fn error(&self) -> &dyn HttpModeledError {
        match self {
            Self::InnerServiceError(error) => error,
            Self::AuthorizeError(error) => error,
        }
    }
}
impl<E: HttpModeledError> std::fmt::Display for AuthorizeServiceError<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(self.error(), f)
    }
}
impl<E: HttpModeledError> std::error::Error for AuthorizeServiceError<E> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.error())
    }
}
impl<E: HttpModeledError> SerializableStruct for AuthorizeServiceError<E> {
    fn serialize_members(&self, serializer: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
        self.error().serialize_members(serializer)
    }
}
impl<E: HttpModeledError> ModeledError for AuthorizeServiceError<E> {
    fn schema(&self) -> &Schema<'_> {
        self.error().schema()
    }
}
impl<E: HttpModeledError> HttpModeledError for AuthorizeServiceError<E> {
    fn status_code(&self) -> u16 {
        self.error().status_code()
    }
}

macro_rules! impl_service {
    ($($var:ident),*) => {
        impl<S, Op, $($var,)*> Service<(Op::Input, ($($var,)*))> for AuthorizeService<Op, S>
        where
            S: Service<(Op::Input, ($($var,)*)), Error = Op::Error> + Clone + Send + 'static,
            S::Future: Send,
            Op: OperationShape + Send + Sync + 'static,
            Op::Input: Send + Sync + 'static,
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

                let authorizer = self.authorizer.clone();

                let fut = async move {
                    let is_authorized = authorizer.authorize(&input).await;
                    if !is_authorized {
                        return Err(Self::Error::AuthorizeError(AuthorizeError {
                            message: "Not authorized!".to_owned(),
                        }));
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

struct Authorizer<Op> {
    operation: PhantomData<Op>,
}

/// We manually implement `Clone` instead of adding `#[derive(Clone)]` because we don't require
/// `Op` to be cloneable.
impl<Op> Clone for Authorizer<Op> {
    fn clone(&self) -> Self {
        Self {
            operation: PhantomData,
        }
    }
}

impl<Op> Authorizer<Op> {
    fn new() -> Self {
        Self {
            operation: PhantomData,
        }
    }

    async fn authorize(&self, _input: &Op::Input) -> bool
    where
        Op: OperationShape,
    {
        // We'd perform the actual authorization here.
        // We would likely need to add bounds on `Op::Input`, `Op::Error`, if we wanted to do
        // anything useful.
        true
    }
}

// If we want our plugin to be as reusable as possible, the service it applies should work with
// inner services (i.e. operation handlers) that take a variable number of parameters. A Rust macro
// is helpful in providing those implementations concisely.
// Each handler function registered must accept the operation's input type (if there is one).
// Additionally, it can take up to 7 different parameters, each of which must implement the
// `FromParts` trait. To ensure that this `AuthorizeService` works with any of those inner
// services, we must implement it to handle up to
// 7 different types. Therefore, we invoke the `impl_service` macro 8 times.

impl_service!();
impl_service!(T1);
impl_service!(T1, T2);
impl_service!(T1, T2, T3);
impl_service!(T1, T2, T3, T4);
impl_service!(T1, T2, T3, T4, T5);
impl_service!(T1, T2, T3, T4, T5, T6);
impl_service!(T1, T2, T3, T4, T5, T6, T7);

#[cfg(test)]
mod tests {
    use super::*;
    use http_body_util::BodyExt;
    use pokemon_service_server_sdk::server::protocol::{
        aws_json_10::AwsJson1_0Protocol, aws_json_11::AwsJson1_1Protocol,
        rest_json_1::RestJson1Protocol, rest_xml::RestXmlProtocol, rpc_v2_cbor::RpcV2CborProtocol,
    };
    use pokemon_service_server_sdk::server::schema::ServerProtocol;

    #[tokio::test]
    async fn authorization_error_uses_the_selected_protocol_wire_format() {
        let protocols: [Box<dyn ServerProtocol>; 5] = [
            Box::new(RestJson1Protocol::default()),
            Box::new(AwsJson1_0Protocol::default()),
            Box::new(AwsJson1_1Protocol::default()),
            Box::new(RestXmlProtocol::default()),
            Box::new(RpcV2CborProtocol::default()),
        ];
        for protocol in protocols {
            let error =
                AuthorizeServiceError::<std::convert::Infallible>::AuthorizeError(AuthorizeError {
                    message: "Not authorized!".into(),
                });
            let response = protocol.serialize_error(&error);
            assert_eq!(response.status(), http::StatusCode::UNAUTHORIZED);
            let headers = response.headers().clone();
            let bytes = response.into_body().collect().await.unwrap().to_bytes();
            match protocol.protocol_id().as_str() {
                "aws.protocols#restJson1" => {
                    assert_eq!(headers["content-type"], "application/json");
                    assert_eq!(headers["x-amzn-errortype"], "AuthorizeError");
                    assert_eq!(bytes, r#"{"message":"Not authorized!"}"#);
                }
                "aws.protocols#awsJson1_0" => {
                    assert_eq!(headers["content-type"], "application/x-amz-json-1.0");
                    assert_eq!(
                        bytes,
                        r#"{"message":"Not authorized!","__type":"pokemon_service.authz#AuthorizeError"}"#
                    );
                }
                "aws.protocols#awsJson1_1" => {
                    assert_eq!(headers["content-type"], "application/x-amz-json-1.1");
                    assert_eq!(
                        bytes,
                        r#"{"message":"Not authorized!","__type":"AuthorizeError"}"#
                    );
                }
                "aws.protocols#restXml" => {
                    assert_eq!(headers["content-type"], "application/xml");
                    assert!(std::str::from_utf8(&bytes)
                        .unwrap()
                        .contains("<message>Not authorized!</message>"));
                }
                "smithy.protocols#rpcv2Cbor" => {
                    assert_eq!(headers["smithy-protocol"], "rpc-v2-cbor");
                    assert_eq!(headers["content-type"], "application/cbor");
                    let shape_id = b"pokemon_service.authz#AuthorizeError";
                    assert!(bytes
                        .windows(shape_id.len())
                        .any(|window| window == shape_id));
                }
                other => panic!("unexpected protocol {other}"),
            }
        }
    }

    #[tokio::test]
    async fn wrapper_preserves_inner_error_and_protocol_escapes_its_message() {
        let inner = AuthorizeError {
            message: "inner \"message\"".into(),
        };
        let error = AuthorizeServiceError::InnerServiceError(inner);
        let response = RestJson1Protocol::default().serialize_error(&error);
        assert_eq!(response.status(), http::StatusCode::UNAUTHORIZED);
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(bytes, r#"{"message":"inner \"message\""}"#);
    }
}
