/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Rust `tracing` and Python `logging` setup and utilities.
use std::path::PathBuf;

use pyo3::prelude::*;
use tracing::Level;
#[cfg(not(test))]
use tracing::Span;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{
    fmt::{self, writer::MakeWriterExt},
    layer::SubscriberExt,
    util::SubscriberInitExt,
};

use crate::error::PyException;

/// Setup `tracing::subscriber` reading the log level from RUST_LOG environment variable.
/// If the variable is not set, the logging for both Python and Rust will be set at the
/// level used by Python logging module.
pub fn setup_tracing(py: Python, logfile: Option<&PathBuf>) -> PyResult<Option<WorkerGuard>> {
    let logging = py.import("logging")?;
    let root = logging.getattr("root")?;
    let handlers = root.getattr("handlers")?;
    let handlers = handlers.extract::<Vec<PyObject>>()?;
    for handler in handlers.iter() {
        let name = handler.getattr(py, "__name__")?;
        if let Ok(name) = name.extract::<&str>(py) {
            if name == "SmithyRsTracingHandler" {
                return setup_tracing_subscriber(py, logfile);
            }
        }
    }
    Ok(None)
}

/// Setup tracing-subscriber to log on console or to a hourly rolling file.
fn setup_tracing_subscriber(
    py: Python,
    logfile: Option<&PathBuf>,
) -> PyResult<Option<WorkerGuard>> {
    let appender = match logfile {
        Some(logfile) => {
            let parent = logfile.parent().ok_or_else(|| {
                PyException::new_err(format!(
                    "Tracing setup failed: unable to extract dirname from path {}",
                    logfile.display()
                ))
            })?;
            let filename = logfile.file_name().ok_or_else(|| {
                PyException::new_err(format!(
                    "Tracing setup failed: unable to extract basename from path {}",
                    logfile.display()
                ))
            })?;
            let file_appender = tracing_appender::rolling::hourly(parent, filename);
            let (appender, guard) = tracing_appender::non_blocking(file_appender);
            Some((appender, guard))
        }
        None => None,
    };

    let logging = py.import("logging")?;
    let root = logging.getattr("root")?;
    let level: u8 = root.getattr("level")?.extract()?;
    let level = match level {
        40u8 => Level::ERROR,
        30u8 => Level::WARN,
        20u8 => Level::INFO,
        10u8 => Level::DEBUG,
        _ => Level::TRACE,
    };
    match appender {
        Some((appender, guard)) => {
            let layer = Some(
                fmt::Layer::new()
                    .with_writer(appender.with_max_level(level))
                    .with_ansi(true)
                    .with_line_number(true)
                    .with_level(true),
            );
            tracing_subscriber::registry().with(layer).init();
            Ok(Some(guard))
        }
        None => {
            let layer = Some(
                fmt::Layer::new()
                    .with_writer(std::io::stdout.with_max_level(level))
                    .with_ansi(true)
                    .with_line_number(true)
                    .with_level(true),
            );
            tracing_subscriber::registry().with(layer).init();
            Ok(None)
        }
    }
}

/// Modifies the Python `logging` module to deliver its log messages using [tracing::Subscriber] events.
///
/// To achieve this goal, the following changes are made to the module:
/// - A new builtin function `logging.py_tracing_event` transcodes `logging.LogRecord`s to `tracing::Event`s. This function
///   is not exported in `logging.__all__`, as it is not intended to be called directly.
/// - A new class `logging.TracingHandler` provides a `logging.Handler` that delivers all records to `python_tracing`.
#[pyclass(name = "TracingHandler")]
#[derive(Debug, Clone)]
pub struct PyTracingHandler;

#[pymethods]
impl PyTracingHandler {
    #[staticmethod]
    fn handle(py: Python) -> PyResult<Py<PyAny>> {
        let logging = py.import("logging")?;
        logging.setattr(
            "py_tracing_event",
            wrap_pyfunction!(py_tracing_event, logging)?,
        )?;

        let pycode = r#"
class TracingHandler(Handler):
    __name__ = "SmithyRsTracingHandler"
    """ Python logging to Rust tracing handler. """
    def emit(self, record):
        py_tracing_event(
            record.levelno, record.getMessage(), record.module,
            record.filename, record.lineno, record.process
        )
"#;
        py.run(pycode, Some(logging.dict()), None)?;
        let all = logging.index()?;
        all.append("TracingHandler")?;
        let handler = logging.getattr("TracingHandler")?;
        Ok(handler.call0()?.into_py(py))
    }
}

/// Consumes a Python `logging.LogRecord` and emits a Rust [tracing::Event] instead.
#[cfg(not(test))]
#[pyfunction]
#[pyo3(text_signature = "(level, record, message, module, filename, line, pid)")]
pub fn py_tracing_event(
    level: u8,
    message: &str,
    module: &str,
    filename: &str,
    line: usize,
    pid: usize,
) -> PyResult<()> {
    let span = Span::current();
    span.record("pid", pid);
    span.record("module", module);
    span.record("filename", filename);
    span.record("lineno", line);
    match level {
        40 => tracing::error!("{message}"),
        30 => tracing::warn!("{message}"),
        20 => tracing::info!("{message}"),
        10 => tracing::debug!("{message}"),
        _ => tracing::trace!("{message}"),
    };
    Ok(())
}

#[cfg(test)]
#[pyfunction]
#[pyo3(text_signature = "(level, record, message, module, filename, line, pid)")]
pub fn py_tracing_event(
    _level: u8,
    message: &str,
    _module: &str,
    _filename: &str,
    _line: usize,
    _pid: usize,
) -> PyResult<()> {
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
            setup_tracing(py, None).unwrap();
            let logging = py.import("logging").unwrap();
            logging.call_method1("info", ("a message",)).unwrap();
        });
    }
}
