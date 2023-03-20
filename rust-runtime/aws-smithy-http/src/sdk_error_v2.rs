/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::result::{ConstructionFailure, ResponseError, SdkError, ServiceError};
use std::any::Any;
use std::convert::Infallible;
use std::error::Error;
use std::fmt::{Debug, Display, Formatter};

pub struct EventLog {
    events: Vec<Event>,
}

trait AnyDebug: Any + Debug {}
impl<T: Any + Debug> AnyDebug for T {}

#[derive(Debug)]
struct Event {
    tp: EventType,
    message: Box<dyn Debug>,
    data: Option<Box<dyn Any>>,
    source: Option<BoxError>,
}

impl Display for Event {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}: {:?}", self.tp, self.message)
    }
}

type BoxError = Box<dyn Error + Send + Sync>;

impl Error for Event {}

#[non_exhaustive]
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum EventType {
    Nothing,
    Construction,
    Dispatch,
    Response,
    Parsing,
    Modeled,
}

#[derive(Debug)]
struct LoggedErrorCause {
    err: BoxError,
    cause: Option<Box<LoggedErrorCause>>,
}

impl LoggedErrorCause {
    pub fn from_vec(errors: &mut Vec<BoxError>) -> Option<LoggedErrorCause> {
        match errors.pop() {
            Some(err) => Some(LoggedErrorCause {
                err,
                cause: LoggedErrorCause::from_vec(errors).map(|e| Box::new(e) as _),
            }),
            None => None,
        }
    }
}

impl Display for LoggedErrorCause {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.err)
    }
}

impl Error for LoggedErrorCause {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.cause.as_ref().map(|e| e.as_ref() as _)
    }
}

struct NoData;

impl EventLog {
    pub fn new() -> Self {
        Self { events: vec![] }
    }

    pub fn push_construction_error(&mut self, ctx: impl Into<String>, error: BoxError) {
        self.push(
            EventType::Construction,
            ctx.into(),
            Option::<NoData>::None,
            Some(error),
        )
    }

    pub fn push_service_error<E: Any + Send + Sync + 'static>(&mut self, error: E) {
        self.push(
            EventType::Parsing,
            "error received from service",
            Some(error),
            None,
        );
    }

    pub fn push(
        &mut self,
        tp: EventType,
        message: impl Debug + Send + Sync + 'static,
        data: Option<impl Any + 'static>,
        error: Option<BoxError>,
    ) {
        self.events.push(Event {
            tp,
            message: Box::new(message),
            data: data.map(|d| Box::new(d) as _),
            source: error,
        })
    }

    pub fn into_error_auto<E, R>(mut self) -> SdkError<E, R>
    where
        E: 'static,
        R: 'static,
    {
        let mut response: Option<R> = None;
        let mut modeled_error: Option<E> = None;
        let mut error_chain: Vec<BoxError> = vec![];
        let mut progress = EventType::Nothing;
        while let Some(ev) = self.events.pop() {
            progress = progress.max(ev.tp);
            if let Some(src) = ev.source {
                // TODO: include the message
                error_chain.push(src);
            }
            if let Some(data) = ev.data {
                let data = match data.downcast::<R>() {
                    Ok(r) => {
                        response = Some(*r);
                        continue;
                    }
                    Err(d) => d,
                };
                match data.downcast::<E>() {
                    Ok(e) => {
                        modeled_error = Some(*e);
                        continue;
                    }
                    Err(d) => {}
                }
            }
        }
        let error_chain = LoggedErrorCause::from_vec(&mut error_chain);

        match (modeled_error, response) {
            (Some(modeled), Some(raw)) => {
                return SdkError::ServiceError(
                    ServiceError::builder()
                        .raw(raw)
                        .source(modeled)
                        .cause(error_chain.expect("one error").into())
                        .build(),
                )
            }
            (Some(_modeled), None) => panic!("modeled error but no raw response :-/"),
            (None, Some(raw)) => {
                return SdkError::ResponseError(
                    ResponseError::builder()
                        .raw(raw)
                        .source(error_chain.expect("at least one error..."))
                        .build(),
                )
            }
            _ => {}
        };

        match progress {
            EventType::Construction => SdkError::ConstructionFailure(
                ConstructionFailure::builder()
                    .source(error_chain.expect("have one error..."))
                    .build(),
            ),
            _ => todo!(),
        }
    }
}

#[cfg(test)]
mod test {
    use crate::body::SdkBody;
    use crate::operation;
    use crate::result::{CreateUnhandledError, SdkError};
    use crate::sdk_error_v2::{EventLog, EventType};
    use aws_smithy_types::error::ErrorMetadata;
    use std::error::Error;
    use std::fmt::{Display, Formatter};

    impl Display for GetObjectError {
        fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
            write!(f, "GetObjectError: {}", self.message)
        }
    }
    #[derive(Debug, Eq, PartialEq)]
    struct GetObjectError {
        message: &'static str,
    }

    impl Error for GetObjectError {}

    impl CreateUnhandledError for GetObjectError {
        fn create_unhandled_error(
            source: Box<dyn Error + Send + Sync + 'static>,
            meta: Option<ErrorMetadata>,
        ) -> Self {
            todo!()
        }
    }

    #[test]
    fn example_error_flow() {
        let mut error_log = EventLog::new();
        error_log.push_construction_error(
            "No credentials from the environment",
            "[credentials] no environment variables set".into(),
        );
        error_log.push_construction_error(
            "No credentials from the profile",
            "[credentials] the profile failed to parse! aborting credentials".into(),
        );
        /*error_log.push(
            EventType::Response,
            "received response from service",
            Some(operation::Response::new(
                http::Response::builder()
                    .body(SdkBody::from("hello!"))
                    .unwrap(),
            )),
            None,
        );*/

        error_log.push_construction_error(
            "no credentials",
            "The credentials provider did not return credentials".into(),
        );
        /*error_log.push_service_error(GetObjectError {
            message: "this is an error!",
        });*/

        let error = error_log.into_error_auto::<GetObjectError, operation::Response>();
        /*assert!(matches!(error, SdkError::ServiceError(_)));
        assert_eq!(
            error.into_service_error(),
            GetObjectError {
                message: "this is an error!"
            }
        );*/
        report(error);
    }

    fn report<E: 'static>(err: E)
    where
        E: std::error::Error,
        E: Send + Sync,
    {
        eprintln!("[ERROR] {}", err);
        if let Some(cause) = err.source() {
            eprintln!();
            eprintln!("Caused by:");
            for (i, e) in std::iter::successors(Some(cause), |e| (*e).source()).enumerate() {
                eprintln!("   {}: {}", i, e);
            }
        }
    }
}
