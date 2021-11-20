use crate::handler::Handler;
use crate::model::*;
use crate::routing::request_spec::{PathAndQuerySpec, PathSegment, PathSpec, QuerySegment, QuerySpec, UriSpec};
use crate::routing::{operation_handler, request_spec::RequestSpec, Router};
use std::marker::PhantomData;

pub struct SimpleServiceOperationRegistry<B, Op1, In1, Op2, In2> {
    pub health_check: Op1,
    pub register_service: Op2,
    _phantom: PhantomData<(Op1, Op2, B, In1, In2)>,
}

// ===========================================================
// `cargo expand`ed from `derive_builder` and cleaned up a bit
// ===========================================================

#[allow(clippy::all)]
///Builder for [`SimpleServiceOperationRegistry`](struct.SimpleServiceOperationRegistry.html).
pub struct SimpleServiceOperationRegistryBuilder<B, Op1, In1, Op2, In2> {
    health_check: ::derive_builder::export::core::option::Option<Op1>,
    register_service: ::derive_builder::export::core::option::Option<Op2>,
    _phantom: PhantomData<(Op1, Op2, B, In1, In2)>,
}
#[allow(clippy::all)]
#[allow(dead_code)]
impl<B, Op1, In1, Op2, In2> SimpleServiceOperationRegistryBuilder<B, Op1, In1, Op2, In2> {
    #[allow(unused_mut)]
    pub fn health_check(self, value: Op1) -> Self {
        let mut new = self;
        new.health_check = ::derive_builder::export::core::option::Option::Some(value);
        new
    }
    #[allow(unused_mut)]
    pub fn register_service(self, value: Op2) -> Self {
        let mut new = self;
        new.register_service = ::derive_builder::export::core::option::Option::Some(value);
        new
    }
    ///Builds a new `SimpleServiceOperationRegistry`.
    ///
    ///# Errors
    ///
    ///If a required field has not been initialized.
    pub fn build(
        self,
    ) -> ::derive_builder::export::core::result::Result<
        SimpleServiceOperationRegistry<B, Op1, In1, Op2, In2>,
        SimpleServiceOperationRegistryBuilderError,
    > {
        Ok(SimpleServiceOperationRegistry {
            health_check: match self.health_check {
                Some(value) => value,
                None => {
                    return ::derive_builder::export::core::result::Result::Err(
                        ::derive_builder::export::core::convert::Into::into(
                            ::derive_builder::UninitializedFieldError::from("health_check"),
                        ),
                    )
                }
            },
            register_service: match self.register_service {
                Some(value) => value,
                None => {
                    return ::derive_builder::export::core::result::Result::Err(
                        ::derive_builder::export::core::convert::Into::into(
                            ::derive_builder::UninitializedFieldError::from("register_service"),
                        ),
                    )
                }
            },
            _phantom: PhantomData,
        })
    }
}
impl<B, Op1, In1, Op2, In2> ::derive_builder::export::core::default::Default
    for SimpleServiceOperationRegistryBuilder<B, Op1, In1, Op2, In2>
{
    fn default() -> Self {
        Self {
            health_check: ::derive_builder::export::core::default::Default::default(),
            register_service: ::derive_builder::export::core::default::Default::default(),
            _phantom: PhantomData,
        }
    }
}
///Error type for SimpleServiceOperationRegistryBuilder
#[non_exhaustive]
pub enum SimpleServiceOperationRegistryBuilderError {
    /// Uninitialized field
    UninitializedField(&'static str),
    /// Custom validation error
    ValidationError(::derive_builder::export::core::string::String),
}
#[allow(unused_qualifications)]
impl ::core::fmt::Debug for SimpleServiceOperationRegistryBuilderError {
    fn fmt(&self, f: &mut ::core::fmt::Formatter) -> ::core::fmt::Result {
        match (&*self,) {
            (&SimpleServiceOperationRegistryBuilderError::UninitializedField(ref __self_0),) => {
                let debug_trait_builder = &mut ::core::fmt::Formatter::debug_tuple(f, "UninitializedField");
                let _ = ::core::fmt::DebugTuple::field(debug_trait_builder, &&(*__self_0));
                ::core::fmt::DebugTuple::finish(debug_trait_builder)
            }
            (&SimpleServiceOperationRegistryBuilderError::ValidationError(ref __self_0),) => {
                let debug_trait_builder = &mut ::core::fmt::Formatter::debug_tuple(f, "ValidationError");
                let _ = ::core::fmt::DebugTuple::field(debug_trait_builder, &&(*__self_0));
                ::core::fmt::DebugTuple::finish(debug_trait_builder)
            }
        }
    }
}
impl ::derive_builder::export::core::convert::From<::derive_builder::UninitializedFieldError>
    for SimpleServiceOperationRegistryBuilderError
{
    fn from(s: ::derive_builder::UninitializedFieldError) -> Self {
        Self::UninitializedField(s.field_name())
    }
}
impl ::derive_builder::export::core::convert::From<::derive_builder::export::core::string::String>
    for SimpleServiceOperationRegistryBuilderError
{
    fn from(s: ::derive_builder::export::core::string::String) -> Self {
        Self::ValidationError(s)
    }
}

// ==================================================================
// END of `cargo expand`ed from `derive_builder` and cleaned up a bit
// ==================================================================

impl<B, Op1, In1, Op2, In2> From<SimpleServiceOperationRegistry<B, Op1, In1, Op2, In2>> for Router<B>
where
    B: Send + 'static,
    Op1: Handler<B, In1, HealthcheckInput>,
    In1: 'static,
    Op2: Handler<B, In2, RegisterServiceInput>,
    In2: 'static,
{
    fn from(registry: SimpleServiceOperationRegistry<B, Op1, In1, Op2, In2>) -> Self {
        // `http localhost:8080/path/to/label/healthcheck`
        let health_check_request_spec = RequestSpec::new(
            http::Method::GET,
            UriSpec {
                host_prefix: None,
                path_and_query: PathAndQuerySpec {
                    path_segments: PathSpec::from_vector_unchecked(vec![
                        PathSegment::Literal(String::from("path")),
                        PathSegment::Literal(String::from("to")),
                        PathSegment::Label,
                        PathSegment::Literal(String::from("healthcheck")),
                    ]),
                    query_segments: QuerySpec::from_vector_unchecked(Vec::new()),
                },
            },
        );

        // `http POST "localhost:8080/register-service/gre/ee/dy/suffix?key&foo=bar"`
        let register_service_request_spec = RequestSpec::new(
            http::Method::POST,
            UriSpec {
                host_prefix: None,
                path_and_query: PathAndQuerySpec {
                    path_segments: PathSpec::from_vector_unchecked(vec![
                        PathSegment::Literal(String::from("register-service")),
                        PathSegment::Greedy,
                        PathSegment::Literal(String::from("suffix")),
                    ]),
                    query_segments: QuerySpec::from_vector_unchecked(vec![
                        QuerySegment::Key(String::from("key")),
                        QuerySegment::KeyValue(String::from("foo"), String::from("bar")),
                    ]),
                },
            },
        );

        let op1 = operation_handler::operation(registry.health_check);
        let op2 = operation_handler::operation(registry.register_service);
        Router::new().route(health_check_request_spec, op1).route(register_service_request_spec, op2)
    }
}
