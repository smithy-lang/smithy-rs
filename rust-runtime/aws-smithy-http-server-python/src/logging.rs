/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Rust `tracing` and Python `logging` setup and utilities.

use std::{path::PathBuf, str::FromStr};

use pyo3::prelude::*;
#[cfg(not(test))]
use tracing::span;
use tracing::Level;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{
    fmt::{self, writer::MakeWriterExt},
    layer::SubscriberExt,
    util::SubscriberInitExt,
    Layer,
};

use crate::error::PyException;

#[derive(Debug, Default)]
enum Format {
    Json,
    Pretty,
    #[default]
    Compact,
}

#[derive(Debug, PartialEq, Eq)]
struct InvalidFormatError;

impl FromStr for Format {
    type Err = InvalidFormatError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "pretty" => Ok(Self::Pretty),
            "json" => Ok(Self::Json),
            "compact" => Ok(Self::Compact),
            _ => Err(InvalidFormatError),
        }
    }
}

/// Setup tracing-subscriber to log on console or to a hourly rolling file.
fn setup_tracing_subscriber(
    level: Option<u8>,
    logfile: Option<PathBuf>,
    format: Option<String>,
) -> PyResult<Option<WorkerGuard>> {
    let format = match format {
        Some(format) => Format::from_str(&format).unwrap_or_else(|_| {
            tracing::error!("unknown format '{format}', falling back to default formatter");
            Format::default()
        }),
        None => Format::default(),
    };

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

    let tracing_level = match level {
        Some(40u8) => Level::ERROR,
        Some(30u8) => Level::WARN,
        Some(20u8) => Level::INFO,
        Some(10u8) => Level::DEBUG,
        None => Level::INFO,
        _ => Level::TRACE,
    };

    let formatter = fmt::Layer::new().with_line_number(true).with_level(true);

    match appender {
        Some((appender, guard)) => {
            let formatter = formatter.with_writer(appender.with_max_level(tracing_level));
            let formatter = match format {
                Format::Json => formatter.json().boxed(),
                Format::Compact => formatter.compact().boxed(),
                Format::Pretty => formatter.pretty().boxed(),
            };
            tracing_subscriber::registry().with(formatter).init();
            Ok(Some(guard))
        }
        None => {
            let formatter = formatter.with_writer(std::io::stdout.with_max_level(tracing_level));
            let formatter = match format {
                Format::Json => formatter.json().boxed(),
                Format::Compact => formatter.compact().boxed(),
                Format::Pretty => formatter.pretty().boxed(),
            };
            tracing_subscriber::registry().with(formatter).init();
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
///
/// :param level typing.Optional\[int\]:
/// :param logfile typing.Optional\[pathlib.Path\]:
/// :param format typing.Optional\[typing.Literal\['compact', 'pretty', 'json'\]\]:
/// :rtype None:
#[pyclass(name = "TracingHandler")]
#[derive(Debug)]
pub struct PyTracingHandler {
    _guard: Option<WorkerGuard>,
}

#[pymethods]
impl PyTracingHandler {
    #[pyo3(text_signature = "($self, level=None, logfile=None, format=None)")]
    #[new]
    fn newpy(
        py: Python,
        level: Option<u8>,
        logfile: Option<PathBuf>,
        format: Option<String>,
    ) -> PyResult<Self> {
        let _guard = setup_tracing_subscriber(level, logfile, format)?;
        let logging = py.import("logging")?;
        let root = logging.getattr("root")?;
        root.setattr("level", level)?;
        // TODO(Investigate why the file appender just create the file and does not write anything, event after holding the guard)
        Ok(Self { _guard })
    }

    /// :rtype typing.Any:
    fn handler(&self, py: Python) -> PyResult<Py<PyAny>> {
        let logging = py.import("logging")?;
        logging.setattr(
            "py_tracing_event",
            wrap_pyfunction!(py_tracing_event, logging)?,
        )?;

        let pycode = r#"
class TracingHandler(Handler):
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
    lineno: usize,
    pid: usize,
) -> PyResult<()> {
    let span = span!(
        Level::TRACE,
        "python",
        pid = pid,
        module = module,
        filename = filename,
        lineno = lineno
    );
    let _guard = span.enter();
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
    use pyo3::types::PyDict;

    use super::*;

    #[test]
    fn tracing_handler_is_injected_in_python() {
        crate::tests::initialize();
        Python::with_gil(|py| {
            let handler = PyTracingHandler::newpy(py, Some(10), None, None).unwrap();
            let kwargs = PyDict::new(py);
            kwargs
                .set_item("handlers", vec![handler.handler(py).unwrap()])
                .unwrap();
            let logging = py.import("logging").unwrap();
            let basic_config = logging.getattr("basicConfig").unwrap();
            basic_config.call((), Some(kwargs)).unwrap();
            logging.call_method1("info", ("a message",)).unwrap();
        });
    }
}
