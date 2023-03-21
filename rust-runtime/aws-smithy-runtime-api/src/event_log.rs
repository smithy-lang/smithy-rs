/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::interceptors::InterceptorError;
use aws_smithy_http::result::{
    ConnectorError, ConstructionFailure, DispatchFailure, ResponseError, SdkError, ServiceError,
    TimeoutError,
};
use std::any::Any;
use std::error::Error;
use std::fmt::Debug;

#[derive(Debug)]
struct Event {
    event_type: EventType,
    data: Option<Box<dyn Any>>,
    source: Option<BoxError>,
}

type BoxError = Box<dyn Error + Send + Sync>;

#[non_exhaustive]
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum EventType {
    Nothing,
    Construction,
    Dispatch,
    Response,
    Timeout,
}

macro_rules! extract_data {
    ($from:ident, $($what:ident -> $into:expr,)+) => {
        let _chain = $from;
        $(extract_data!(_chain, $what -> $into);)+
    };
    ($from:ident, $what:ident -> $into:expr) => {
        let $from = match $from.downcast::<$what>() {
            Ok(value) => {
                $into = Some(*value);
                continue;
            }
            Err(from) => from,
        };
    };
}

#[derive(Debug, Default)]
pub struct EventLog {
    events: Vec<Event>,
}

impl EventLog {
    pub fn new() -> Self {
        Self { events: vec![] }
    }

    pub fn start_construction(mut self) -> Self {
        self.push(EventType::Construction, None, None);
        self
    }

    pub fn start_dispatch(mut self) -> Self {
        self.push(EventType::Dispatch, None, None);
        self
    }

    pub fn start_response_handling<R>(mut self, raw_response: R) -> Self
    where
        R: Any + Send + Sync + 'static,
    {
        self.push(EventType::Response, Some(Box::new(raw_response) as _), None);
        self
    }

    pub fn handle_result(self, result: Result<(), BoxError>) -> Result<Self, Self> {
        match result {
            Ok(_) => Ok(self),
            Err(err) => Err(self.push_error(err)),
        }
    }

    pub fn handle_interceptor(self, result: Result<(), InterceptorError>) -> Result<Self, Self> {
        match result {
            Ok(_) => Ok(self),
            Err(err) => Err(self.push_interceptor_error(err)),
        }
    }

    pub fn push_error(mut self, error: BoxError) -> Self {
        self.push(EventType::Nothing, None, Some(error.into()));
        self
    }

    pub fn push_interceptor_error(mut self, interceptor_error: InterceptorError) -> Self {
        self.push(EventType::Nothing, None, Some(interceptor_error.into()));
        self
    }

    pub fn push_connector_error(mut self, connector_error: ConnectorError) -> Self {
        self.push(
            EventType::Dispatch,
            Some(Box::new(connector_error) as _),
            None,
        );
        self
    }

    pub fn push_construction_error(mut self, error: impl Into<BoxError>) -> Self {
        self.push(EventType::Construction, None, Some(error.into()));
        self
    }

    pub fn push_timeout_error(mut self, source: impl Into<BoxError>) -> Self {
        self.push(EventType::Timeout, None, Some(source.into()));
        self
    }

    pub fn push_service_error<E: Any + Send + Sync + 'static>(mut self, error: E) -> Self {
        self.push(
            EventType::Response,
            Some(Box::new(error) as _),
            Some("service responded with an error".into()),
        );
        self
    }

    pub fn push_deserialize_error(mut self, parse_failure: impl Into<BoxError>) -> Self {
        self.push(EventType::Response, None, Some(parse_failure.into()));
        self
    }

    fn push(&mut self, event_type: EventType, data: Option<Box<dyn Any>>, error: Option<BoxError>) {
        self.events.push(Event {
            event_type,
            data,
            source: error,
        })
    }

    pub fn try_into_sdk_error<E, R>(mut self) -> Result<SdkError<E, R>, Self>
    where
        E: 'static,
        R: 'static,
    {
        if self.events.is_empty() {
            return Err(self);
        }

        struct ExtractedData<E, R> {
            response: Option<R>,
            modeled_error: Option<E>,
            connector_error: Option<ConnectorError>,
        }
        let mut extracted: ExtractedData<E, R> = ExtractedData {
            response: None,
            modeled_error: None,
            connector_error: None,
        };
        let mut top_most_error: Option<BoxError> = None;
        let mut stage = EventType::Nothing;
        while let Some(ev) = self.events.pop() {
            stage = stage.max(ev.event_type);
            if top_most_error.is_none() {
                if let Some(src) = ev.source {
                    top_most_error = Some(src);
                }
            }
            if let Some(data) = ev.data {
                extract_data!(
                    data,
                    E -> extracted.modeled_error,
                    R -> extracted.response,
                    ConnectorError -> extracted.connector_error,
                );
            }
        }

        if let Some(connector_error) = extracted.connector_error {
            return Ok(SdkError::DispatchFailure(
                DispatchFailure::builder().source(connector_error).build(),
            ));
        }
        if let Some(top_most_error) = top_most_error {
            match (extracted.modeled_error, extracted.response) {
                (None, Some(raw)) => {
                    return Ok(SdkError::ResponseError(
                        ResponseError::builder()
                            .raw(raw)
                            .source(top_most_error)
                            .build(),
                    ))
                }
                (Some(modeled), Some(raw)) => {
                    return Ok(SdkError::ServiceError(
                        ServiceError::builder()
                            .raw(raw)
                            .modeled_source(modeled)
                            .unmodeled_source(top_most_error)
                            .build(),
                    ))
                }
                (Some(_modeled), None) => {
                    unreachable!("modeled error cannot exist in the absense of a raw response")
                }
                _ => {}
            };

            match stage {
                EventType::Nothing => Err(self),
                EventType::Construction => Ok(SdkError::ConstructionFailure(
                    ConstructionFailure::builder()
                        .source(top_most_error)
                        .build(),
                )),
                EventType::Timeout => Ok(SdkError::TimeoutError(
                    TimeoutError::builder().source(top_most_error).build(),
                )),
                _ => unreachable!(
                    "these cases must be covered by modeled error/raw response extraction above"
                ),
            }
        } else {
            Err(self)
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use aws_smithy_http::result::{ConnectorError, SdkError};
    use std::error::Error;
    use std::fmt::{Display, Formatter};

    #[derive(Clone, Debug, Eq, PartialEq)]
    struct GetObjectError {
        message: &'static str,
    }

    impl Display for GetObjectError {
        fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
            write!(f, "GetObjectError: {}", self.message)
        }
    }

    impl Error for GetObjectError {}

    #[test]
    fn extract_connector_error() {
        let connector_error = || ConnectorError::other("test".into(), None);
        let event_log = EventLog::new()
            .start_construction()
            .start_dispatch()
            .push_connector_error(connector_error());

        match event_log
            .try_into_sdk_error::<GetObjectError, http::Response<()>>()
            .unwrap()
        {
            SdkError::DispatchFailure(context) => {
                assert_eq!(
                    connector_error().to_string(),
                    context.as_connector_error().unwrap().to_string()
                );
            }
            _ => panic!("wrong sdk error type"),
        }
    }

    #[test]
    fn construction_failure() {
        let event_log = EventLog::new()
            .start_construction()
            .push_construction_error("[credentials] no environment variables set");

        let sdk_error = event_log
            .try_into_sdk_error::<GetObjectError, http::Response<()>>()
            .unwrap();
        assert!(matches!(sdk_error, SdkError::ConstructionFailure(_)));
        assert_eq!(
            "[credentials] no environment variables set",
            sdk_error.source().unwrap().to_string()
        )
    }

    #[test]
    fn service_error() {
        let original = GetObjectError { message: "foo" };
        let event_log = EventLog::new()
            .start_construction()
            .start_dispatch()
            .start_response_handling(http::Response::builder().status(500).body(()).unwrap())
            .push_service_error(original.clone());

        let sdk_error = event_log
            .try_into_sdk_error::<GetObjectError, http::Response<()>>()
            .unwrap();
        match &sdk_error {
            SdkError::ServiceError(context) => {
                assert_eq!(&original, context.err());
                assert_eq!(500, context.raw().status());
            }
            _ => panic!("wrong sdk error type"),
        }
    }

    #[test]
    fn response_error() {
        let event_log = EventLog::new()
            .start_construction()
            .start_dispatch()
            .start_response_handling(http::Response::builder().status(418).body(()).unwrap())
            .push_deserialize_error("failed to parse the response");

        let sdk_error = event_log
            .try_into_sdk_error::<GetObjectError, http::Response<()>>()
            .unwrap();
        match &sdk_error {
            SdkError::ResponseError(context) => {
                assert_eq!(
                    "failed to parse the response",
                    sdk_error.source().unwrap().to_string()
                );
                assert_eq!(418, context.raw().status());
            }
            _ => panic!("wrong sdk error type"),
        }
    }

    #[test]
    fn timeout_error() {
        let event_log = EventLog::new()
            .start_construction()
            .start_dispatch()
            .push_timeout_error("timed out after 1 second");

        let sdk_error = event_log
            .try_into_sdk_error::<GetObjectError, http::Response<()>>()
            .unwrap();
        assert!(matches!(sdk_error, SdkError::TimeoutError(_)));
        assert_eq!(
            "timed out after 1 second",
            sdk_error.source().unwrap().to_string()
        );
    }

    #[test]
    fn interceptor_error_during_construction() {
        let event_log = EventLog::new().start_construction().push_interceptor_error(
            InterceptorError::read_before_serialization("interceptor failed"),
        );

        let sdk_error = event_log
            .try_into_sdk_error::<GetObjectError, http::Response<()>>()
            .unwrap();
        assert!(matches!(sdk_error, SdkError::ConstructionFailure(_)));
        assert_eq!(
            "read_before_serialization interceptor encountered an error",
            sdk_error.source().unwrap().to_string()
        );
        assert_eq!(
            "interceptor failed",
            sdk_error.source().unwrap().source().unwrap().to_string()
        );
    }
}
