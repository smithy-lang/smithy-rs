/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_smithy_http::result::{
    ConnectorError, ConstructionFailure, DispatchFailure, ResponseError, SdkError, ServiceError,
};
use std::any::Any;
use std::error::Error;
use std::fmt::{Debug, Display, Formatter};

#[derive(Debug)]
struct Event {
    event_type: EventType,
    data: Vec<Box<dyn Any>>,
    source: Option<BoxError>,
}

type BoxError = Box<dyn Error + Send + Sync>;

#[non_exhaustive]
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum EventType {
    Informational,
    Construction,
    Dispatch,
    Response,
}

#[derive(Debug)]
struct EventLogError {
    err: BoxError,
    source: Option<Box<EventLogError>>,
}

impl EventLogError {
    fn from_vec(errors: &mut Vec<BoxError>) -> Option<EventLogError> {
        errors.pop().map(|err| EventLogError {
            err,
            source: EventLogError::from_vec(errors).map(|e| Box::new(e) as _),
        })
    }
}

impl Display for EventLogError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.err)
    }
}

impl Error for EventLogError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.source.as_ref().map(|e| e.as_ref() as _)
    }
}

macro_rules! extract_data {
    ($from:ident, $($what:ident -> $into:ident,)+) => {
        let _chain = $from;
        $(extract_data!(_chain, $what -> $into);)+
    };
    ($from:ident, $what:ident -> $into:ident) => {
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

    pub fn push_connector_error(&mut self, connector_error: ConnectorError) {
        self.push(
            EventType::Dispatch,
            vec![Box::new(connector_error) as _],
            None,
        );
    }

    pub fn push_construction_error(&mut self, error: BoxError) {
        self.push(EventType::Construction, Vec::new(), Some(error));
    }

    pub fn push_service_error<E, R>(&mut self, error: E, raw_response: R)
    where
        E: Any + Send + Sync + 'static,
        R: Any + Send + Sync + 'static,
    {
        self.push(
            EventType::Response,
            vec![Box::new(error) as _, Box::new(raw_response) as _],
            Some("service responded with an error".into()),
        );
    }

    pub fn push_response_error<R: Any + Send + Sync + 'static>(
        &mut self,
        raw_response: R,
        parse_failure: impl Into<BoxError>,
    ) {
        self.push(
            EventType::Response,
            vec![Box::new(raw_response) as _],
            Some(parse_failure.into()),
        );
    }

    fn push(&mut self, event_type: EventType, data: Vec<Box<dyn Any>>, error: Option<BoxError>) {
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

        let mut response: Option<R> = None;
        let mut modeled_error: Option<E> = None;
        let mut connector_error: Option<ConnectorError> = None;
        let mut error_chain: Vec<BoxError> = vec![];
        let mut progress = EventType::Informational;
        while let Some(ev) = self.events.pop() {
            progress = progress.max(ev.event_type);
            if let Some(src) = ev.source {
                error_chain.push(src);
            }
            for data in ev.data {
                extract_data!(
                    data,
                    R -> response,
                    ConnectorError -> connector_error,
                    E -> modeled_error,
                );
            }
        }

        if let Some(connector_error) = connector_error {
            return Ok(SdkError::DispatchFailure(
                DispatchFailure::builder().source(connector_error).build(),
            ));
        }

        let error_chain = if let Some(chain) = EventLogError::from_vec(&mut error_chain) {
            chain
        } else {
            return Err(self);
        };

        match (modeled_error, response) {
            (None, Some(raw)) => {
                return Ok(SdkError::ResponseError(
                    ResponseError::builder()
                        .raw(raw)
                        .source(error_chain)
                        .build(),
                ))
            }
            (Some(modeled), Some(raw)) => {
                return Ok(SdkError::ServiceError(
                    ServiceError::builder()
                        .raw(raw)
                        .modeled_source(modeled)
                        .unmodeled_source(error_chain)
                        .build(),
                ))
            }
            (Some(_modeled), None) => {
                unreachable!("modeled error cannot exist in the absense of a raw response")
            }
            _ => {}
        };

        match progress {
            EventType::Informational => Err(self),
            EventType::Construction => Ok(SdkError::ConstructionFailure(
                ConstructionFailure::builder().source(error_chain).build(),
            )),
            _ => unreachable!(
                "these cases must be covered by modeled error/raw response extraction above"
            ),
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
        let mut error_log = EventLog::new();
        error_log.push_connector_error(connector_error());

        match error_log
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
        let mut error_log = EventLog::new();
        error_log.push_construction_error("[credentials] no environment variables set".into());

        let sdk_error = error_log
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
        let mut error_log = EventLog::new();
        error_log.push_service_error(
            original.clone(),
            http::Response::builder().status(500).body(()).unwrap(),
        );

        let sdk_error = error_log
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
        let mut error_log = EventLog::new();
        error_log.push_response_error(
            http::Response::builder().status(418).body(()).unwrap(),
            "failed to parse the response",
        );

        let sdk_error = error_log
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
}
