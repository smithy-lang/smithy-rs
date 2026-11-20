/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_smithy_schema::{shape_id, Schema, StringTrait};

use crate::extension::RuntimeErrorExtension;

/// How a request turned out.
///
/// Outcomes are classified according to the following rules:
///
/// 1. If the final response was derived from the result of the [operation handler] or [model
///    plugin] and serialized successfully, then the [`source`] is [`OutcomeSource::Operation`],
///    and:
///     - If the result was [`Ok`], then the [`result`] is [`Ok`].
///     - If the result was [`Err`], then the [`result`] is either an [`Err`] whose [`fault`] is
///       [`Fault::Client`] or [`Fault::Server`] based on the applicable `@error` trait declared in
///       the model.
/// 2. If the final response was constructed and returned by a [layer], [HTTP plugin], or the
///    Smithy-rs server itself (i.e., not derived from the serialized result of the operation
///    handler or model plugin), then the [`source`] is [`OutcomeSource::Internal`], and the
///    [`result`] is based on the HTTP response status code as follows:
///     - `100..400`: [`Ok`]
///     - `400..500`: an [`Err`] whose [`fault`] is [`Fault::Client`]
///     - all others: an [`Err`] whose [`fault`] is [`Fault::Server`]
/// 3. If the response body generates an error while it is being streamed (after the response header
///    has already been determined), the [`result`] is an [`Err`] whose [`fault`] is
///    [`Fault::Server`] and the [`source`] is [`OutcomeSource::Internal`]. This rule takes
///    precedence over the other rules. (Network errors between client and server, such as
///    unexpected client disconnects during response streaming, do not trigger this rule.)
///
/// [`source`]: Self::source
/// [`result`]: Self::result
/// [`fault`]: ErrorOutcome::fault
/// [operation handler]: crate::operation::Handler
/// [model plugin]: crate::plugin::ModelMarker
/// [HTTP plugin]: crate::plugin::HttpMarker
/// [layer]: tower::Layer
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Outcome {
    /// Information about the request's success or failure.
    pub result: Result<SuccessOutcome, ErrorOutcome>,
    /// What the classification in [`result`](Self::result) is based on.
    pub source: OutcomeSource,
}

/// Details of a request that succeeded.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct SuccessOutcome {}

/// Details of a request that failed.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct ErrorOutcome {
    /// Whose fault the error was.
    pub fault: Fault,
}

/// Whose fault a request's error was, corresponding to the value of the [`@error`] trait in the
/// model.
///
/// [`@error`]: https://smithy.io/2.0/spec/type-refinement-traits.html#error-trait
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Fault {
    /// The request failed because of the client.
    Client,
    /// The request failed because of the server.
    Server,
}

/// What an [Outcome]'s classification is based on.
///
/// See the [Outcome] documentation for details on how outcomes are classified.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum OutcomeSource {
    /// The outcome was classified according to the result of the [operation handler] or [model
    /// plugin].
    ///
    /// [operation handler]: crate::operation::Handler
    /// [model plugin]: crate::plugin::ModelMarker
    Operation,
    /// The outcome was classified according to one of the following:
    ///
    /// - The status code of a response constructed by a [layer], [HTTP plugin], or the Smithy-rs
    ///   server itself.
    /// - A non-network error that occurred while streaming the response body to the client.
    ///
    /// [HTTP plugin]: crate::plugin::HttpMarker
    /// [layer]: tower::Layer
    Internal,
}

impl Outcome {
    /// Classifies `response`, removing the marker left by [`ModeledOutcome`] if there is one.
    pub(crate) fn take_from_response<B>(response: &mut http::Response<B>) -> Self {
        if let Some(ModeledOutcome(result)) = response.extensions_mut().remove() {
            return Self {
                result: result.map_err(|fault| ErrorOutcome { fault }),
                source: OutcomeSource::Operation,
            };
        }
        let result = match response.status().as_u16() {
            100..400 => Ok(SuccessOutcome {}),
            400..500 => Err(ErrorOutcome { fault: Fault::Client }),
            _ => Err(ErrorOutcome { fault: Fault::Server }),
        };
        Self {
            result,
            source: OutcomeSource::Internal,
        }
    }

    /// The outcome of a response body that failed while it was being sent.
    pub(crate) fn body_failure() -> Self {
        Self {
            result: Err(ErrorOutcome { fault: Fault::Server }),
            source: OutcomeSource::Internal,
        }
    }
}

impl Fault {
    /// The fault declared by an error shape's `@error` trait.
    pub(crate) fn of_error_schema(schema: &Schema<'_>) -> Option<Self> {
        let fault = schema
            .traits()
            .and_then(|traits| traits.get(&shape_id!("smithy.api", "error")))
            .and_then(|value| value.as_any().downcast_ref::<StringTrait>())
            .map(|value| value.value());
        match fault {
            Some("client") => Some(Self::Client),
            Some("server") => Some(Self::Server),
            _ => None,
        }
    }
}

/// Marks a response as serialized from an operation's modeled result, with the result's
/// classification.
///
/// Inserted into response extensions by the framework and generated code. Not part of the public
/// API.
#[doc(hidden)]
#[derive(Debug, Clone)]
pub struct ModeledOutcome(Result<SuccessOutcome, Fault>);

impl ModeledOutcome {
    #[doc(hidden)]
    pub fn new(result: Result<SuccessOutcome, Fault>) -> Self {
        Self(result)
    }

    /// Marks `response` with `result`, unless serializing the result failed and the framework
    /// responded with a runtime error instead.
    pub(crate) fn mark<B>(response: &mut http::Response<B>, result: Result<SuccessOutcome, Fault>) {
        if response.extensions().get::<RuntimeErrorExtension>().is_none() {
            response.extensions_mut().insert(Self(result));
        }
    }
}

#[cfg(test)]
impl Outcome {
    /// A comparable summary of this outcome for tests, since outcomes deliberately do not
    /// implement `PartialEq`: the result (`"success"`, `"client"`, or `"server"`) and the source
    /// (`"operation"` or `"internal"`).
    pub(crate) fn summary(&self) -> (&'static str, &'static str) {
        let result = match &self.result {
            Ok(_) => "success",
            Err(ErrorOutcome { fault: Fault::Client }) => "client",
            Err(ErrorOutcome { fault: Fault::Server }) => "server",
        };
        let source = match self.source {
            OutcomeSource::Operation => "operation",
            OutcomeSource::Internal => "internal",
        };
        (result, source)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn response(status: u16) -> http::Response<()> {
        let mut response = http::Response::new(());
        *response.status_mut() = http::StatusCode::from_u16(status).unwrap();
        response
    }

    #[test]
    fn unmarked_responses_are_classified_by_status() {
        for (status, result) in [
            (101, "success"),
            (200, "success"),
            (304, "success"),
            (404, "client"),
            (499, "client"),
            (500, "server"),
            (503, "server"),
            (600, "server"),
            (999, "server"),
        ] {
            let outcome = Outcome::take_from_response(&mut response(status));
            assert_eq!(outcome.summary(), (result, "internal"), "{status}");
        }
    }

    #[test]
    fn marked_responses_are_classified_by_marker() {
        let mut marked = response(400);
        ModeledOutcome::mark(&mut marked, Err(Fault::Server));
        let outcome = Outcome::take_from_response(&mut marked);
        assert_eq!(outcome.summary(), ("server", "operation"));
        assert!(
            marked.extensions().get::<ModeledOutcome>().is_none(),
            "the marker is removed"
        );
    }

    #[test]
    fn runtime_errors_are_not_marked() {
        let mut failed = response(400);
        failed
            .extensions_mut()
            .insert(RuntimeErrorExtension::new("SerializationException".to_owned()));
        ModeledOutcome::mark(&mut failed, Ok(SuccessOutcome {}));
        assert!(failed.extensions().get::<ModeledOutcome>().is_none());
    }
}
