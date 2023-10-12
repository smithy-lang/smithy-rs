use std::error::Error;

use aws_smithy_http::{body::SdkBody, result::ConnectorError};
use http::Response;
use pokemon_service_client::error::SdkError;

/// An extension method that logs the error and triggers a panic. If the error indicates 
/// the server is unavailable, it displays a customized message.
pub trait ResultExt<T> {
    fn custom_expect_and_log(self, err: &str) -> T;
}

// Implement the extension trait on Result<T, SdkError>
impl<T, E> ResultExt<T> for Result<T, SdkError<E>>
where
    T: std::fmt::Debug,
    E: Error + Send + Sync + 'static,
{
    fn custom_expect_and_log(self, msg: &str) -> T {
        match self {
            Ok(response) => response,
            Err(e @ SdkError::DispatchFailure(_)) => {
                // DipatchFailure might be due to a ConnectionError. Get the detailed
                // message and use that in panic!.
                let details = extract_error_details(e);
                panic!("{msg}: {details}");
            }
            Err(e) => {
                panic!("{msg}: {e}");
            }
        }
    }
}

fn extract_error_details<E>(e: SdkError<E, Response<SdkBody>>) -> String
where
    E: Error + Send + Sync + 'static,
{
    // Get the source of SdkError or if there is no source then use the SdkError
    // itself as the source.
    let source = e.into_source().unwrap_or_else(|e| e.into());

    match source.downcast::<ConnectorError>() {
        Ok(connect_error) => {
            let source = connect_error.into_source();

            match source.downcast::<hyper::Error>() {
                Ok(hyper_error) if hyper_error.is_connect() => 
                    "Connection could not be made to Pokemon service. Please make sure you have the pokemon service running locally on tcp port:13734".to_string(),
                Ok(e) => e.to_string(),
                Err(e) => e.to_string(),
            }
        }
        Err(e) => e.to_string(),
    }
}

/// Sets up the tracing subscriber to print info! and error! messages on the console.
pub fn setup_tracing_subscriber() {
    // To capture backtraces in the event of an error, add RUST_LIB_BACKTRACE to the environment.
    // Note: Capturing a backtrace can significantly impact runtime performance. While it's utilized
    // here for demonstration, in production code it should be adjusted according to specific needs.
    if std::env::var("RUST_LIB_BACKTRACE").is_err() {
        std::env::set_var("RUST_LIB_BACKTRACE", "1");
    }

    // Install color_eyre to produce colored error outputs. Ignore errors if there are any.
    let _ = color_eyre::install();

    // By default set info! as the logging level.
    if std::env::var("RUST_LOG").is_err() {
        std::env::set_var("RUST_LOG", "info");
    }
    // Add a tracing subscriber that uses the environment variable RUST_LOG
    // to figure out which log level should be emitted.
    tracing_subscriber::fmt::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
}
