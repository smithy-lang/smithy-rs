/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Rust `tracing` and Python `logging` setup and utilities.

use pyo3::prelude::*;
use tracing::Level;
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::{prelude::*, EnvFilter};

/// Setup `tracing::subscriber` reading the log level from RUST_LOG environment variable
/// and inject the custom Python `logger` into the interpreter.
pub fn setup(py: Python, level: LogLevel) -> PyResult<()> {
    let format = tracing_subscriber::fmt::layer()
        .with_ansi(true)
        .with_level(true);
    match EnvFilter::try_from_default_env() {
        Ok(filter) => {
            let level: LogLevel = filter.to_string().into();
            tracing_subscriber::registry()
                .with(format)
                .with(filter)
                .init();
            setup_python_logging(py, level)?;
        }
        Err(_) => {
            tracing_subscriber::registry()
                .with(format)
                .with(LevelFilter::from_level(level.into()))
                .init();
            setup_python_logging(py, level)?;
        }
    }
    Ok(())
}

/// This custom logger enum exported to Python can be used to configure the
/// both the Rust `tracing` and Python `logging` levels.
/// We cannot export directly `tracing::Level` to Python.
#[pyclass]
#[derive(Debug, Clone, Copy)]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

/// `From<LogLevel>` is used to convert `LogLevel` to the correct string
/// needed by Python `logging` module.
impl From<LogLevel> for String {
    fn from(other: LogLevel) -> String {
        match other {
            LogLevel::Error => "ERROR".into(),
            LogLevel::Warn => "WARN".into(),
            LogLevel::Info => "INFO".into(),
            _ => "DEBUG".into(),
        }
    }
}

/// `From<String>` is used to covert `tracing::EnvFilter` into `LogLevel`.
impl From<String> for LogLevel {
    fn from(other: String) -> LogLevel {
        match other.as_str() {
            "error" => LogLevel::Error,
            "warn" => LogLevel::Warn,
            "info" => LogLevel::Info,
            "debug" => LogLevel::Debug,
            _ => LogLevel::Trace,
        }
    }
}

/// `From<LogLevel>` is used to covert `LogLevel` into `tracing::EnvFilter`.
impl From<LogLevel> for Level {
    fn from(other: LogLevel) -> Level {
        match other {
            LogLevel::Debug => Level::DEBUG,
            LogLevel::Info => Level::INFO,
            LogLevel::Warn => Level::WARN,
            LogLevel::Error => Level::ERROR,
            _ => Level::TRACE,
        }
    }
}

/// Modifies the Python `logging` module to deliver its log messages using [tracing::Subscriber] events.
///
/// To achieve this goal, the following changes are made to the module:
/// - A new builtin function `logging.python_tracing` transcodes `logging.LogRecord`s to `tracing::Event`s. This function
///   is not exported in `logging.__all__`, as it is not intended to be called directly.
/// - A new class `logging.RustTracing` provides a `logging.Handler` that delivers all records to `python_tracing`.
/// - `logging.basicConfig` is changed to use `logging.HostHandler` by default.
///
/// Since any call like `logging.warn(...)` sets up logging via `logging.basicConfig`, all log messages are now
/// delivered to `crate::logging`, which will send them to `tracing::event!`.
fn setup_python_logging(py: Python, level: LogLevel) -> PyResult<()> {
    let logging = py.import("logging")?;
    logging.setattr("python_tracing", wrap_pyfunction!(python_tracing, logging)?)?;

    let level: String = level.into();
    let pycode = format!(
        r#"
class RustTracing(Handler):
    """ Python logging to Rust tracing handler. """
    def __init__(self, level=0):
        super().__init__(level=level)

    def emit(self, record):
        python_tracing(record)

# Store the old basicConfig in the local namespace.
oldBasicConfig = basicConfig

def basicConfig(*pargs, **kwargs):
    """ Reimplement basicConfig to hijack the root logger. """
    if "handlers" not in kwargs:
        kwargs["handlers"] = [RustTracing()]
    kwargs["level"] = {level}
    return oldBasicConfig(*pargs, **kwargs)
"#,
    );

    py.run(&pycode, Some(logging.dict()), None)?;
    let all = logging.index()?;
    all.append("RustTracing")?;
    Ok(())
}

/// Consumes a Python `logging.LogRecord` and emits a Rust [tracing::Event] instead.
#[cfg(not(test))]
#[pyfunction]
#[pyo3(text_signature = "(record)")]
fn python_tracing(record: &PyAny) -> PyResult<()> {
    let level = record.getattr("levelno")?;
    let message = record.getattr("getMessage")?.call0()?;
    let module = record.getattr("module")?;
    let filename = record.getattr("filename")?;
    let line = record.getattr("lineno")?;

    match level.extract()? {
        40u8 => tracing::event!(Level::ERROR, %module, %filename, %line, "{message}"),
        30u8 => tracing::event!(Level::WARN, %module, %filename, %line, "{message}"),
        20u8 => tracing::event!(Level::INFO, %module, %filename, %line, "{message}"),
        10u8 => tracing::event!(Level::DEBUG, %module, %filename, %line, "{message}"),
        _ => tracing::event!(Level::TRACE, %module, %filename, %line, "{message}"),
    };

    Ok(())
}

#[cfg(test)]
#[pyfunction]
#[pyo3(text_signature = "(record)")]
fn python_tracing(record: &PyAny) -> PyResult<()> {
    let message = record.getattr("getMessage")?.call0()?;
    pretty_assertions::assert_eq!(message.to_string(), "a message");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracing_handler_is_injected_in_python() {
        crate::tests::initialize();
        Python::with_gil(|py| {
            setup_python_logging(py, LogLevel::Info).unwrap();
            let logging = py.import("logging").unwrap();
            logging.call_method1("info", ("a message",)).unwrap();
        });
    }
}
