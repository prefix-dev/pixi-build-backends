use pyo3::{PyResult, pyfunction};
use tracing::Level;

const DEFAULT_LOGGER_NAME: &str = "pixi.python.logger";
const TARGET: &str = "pixi.python.backend";

fn log_event(level: Level, message: &str, logger: Option<&str>) {
    let logger_name = logger.unwrap_or(DEFAULT_LOGGER_NAME);
    match level {
        Level::TRACE => tracing::event!(
            target: TARGET,
            Level::TRACE,
            py_logger = logger_name,
            py_level = "TRACE",
            message = %message
        ),
        Level::DEBUG => tracing::event!(
            target: TARGET,
            Level::DEBUG,
            py_logger = logger_name,
            py_level = "DEBUG",
            message = %message
        ),
        Level::INFO => tracing::event!(
            target: TARGET,
            Level::INFO,
            py_logger = logger_name,
            py_level = "INFO",
            message = %message
        ),
        Level::WARN => tracing::event!(
            target: TARGET,
            Level::WARN,
            py_logger = logger_name,
            py_level = "WARN",
            message = %message
        ),
        Level::ERROR => tracing::event!(
            target: TARGET,
            Level::ERROR,
            py_logger = logger_name,
            py_level = "ERROR",
            message = %message
        ),
    }
}

#[pyfunction]
#[pyo3(signature = (message, logger=None))]
pub fn trace(message: &str, logger: Option<&str>) -> PyResult<()> {
    log_event(Level::TRACE, message, logger);
    Ok(())
}

#[pyfunction]
#[pyo3(signature = (message, logger=None))]
pub fn debug(message: &str, logger: Option<&str>) -> PyResult<()> {
    log_event(Level::DEBUG, message, logger);
    Ok(())
}

#[pyfunction]
#[pyo3(signature = (message, logger=None))]
pub fn info(message: &str, logger: Option<&str>) -> PyResult<()> {
    log_event(Level::INFO, message, logger);
    Ok(())
}

#[pyfunction]
#[pyo3(signature = (message, logger=None))]
pub fn warn(message: &str, logger: Option<&str>) -> PyResult<()> {
    log_event(Level::WARN, message, logger);
    Ok(())
}

#[pyfunction]
#[pyo3(signature = (message, logger=None))]
pub fn error(message: &str, logger: Option<&str>) -> PyResult<()> {
    log_event(Level::ERROR, message, logger);
    Ok(())
}
