"""
Utilities that bridge Python's logging module with pixi's Rust tracing backend.
"""

from __future__ import annotations

import logging
from typing import Optional

from pixi_build_backend.pixi_build_backend import (
    debug as _native_debug,
    error as _native_error,
    info as _native_info,
    trace as _native_trace,
    warn as _native_warn,
)

TRACE_LEVEL = 5
logging.addLevelName(TRACE_LEVEL, "TRACE")


def _install_trace_method() -> None:
    if hasattr(logging.Logger, "trace"):
        return

    def trace(self: logging.Logger, msg, *args, **kwargs):
        if self.isEnabledFor(TRACE_LEVEL):
            self._log(TRACE_LEVEL, msg, args, **kwargs)

    setattr(logging.Logger, "trace", trace)


_install_trace_method()


def _dispatch(level: int, message: str, logger_name: str) -> None:
    if level >= logging.ERROR:
        _native_error(message, logger_name)
    elif level >= logging.WARNING:
        _native_warn(message, logger_name)
    elif level >= logging.INFO:
        _native_info(message, logger_name)
    elif level >= logging.DEBUG:
        _native_debug(message, logger_name)
    else:
        _native_trace(message, logger_name)


class PixiTracingHandler(logging.Handler):
    """Handler that forwards Python logging records to Rust tracing."""

    def __init__(self, *, logger_name: Optional[str] = None) -> None:
        super().__init__()
        self._logger_name = logger_name

    def emit(self, record: logging.LogRecord) -> None:
        target = self._logger_name or record.name
        try:
            message = self.format(record)
        except Exception:
            self.handleError(record)
            return

        _dispatch(record.levelno, message, target)


def _ensure_handler(logger: logging.Logger) -> PixiTracingHandler:
    for handler in logger.handlers:
        if isinstance(handler, PixiTracingHandler):
            return handler

    handler = PixiTracingHandler()
    handler.setFormatter(logging.Formatter("%(message)s"))
    logger.addHandler(handler)
    logger.propagate = False
    return handler


def get_logger(name: str = "pixi.python.backend", *, level: int = logging.INFO) -> logging.Logger:
    """Return a logger configured to emit Rust tracing events."""

    logger = logging.getLogger(name)
    logger.setLevel(level)
    _ensure_handler(logger)
    return logger


def configure_logger(
    name: str = "pixi.python.backend",
    *,
    level: int = logging.INFO,
) -> logging.Logger:
    """
    Configure and return a logger that sends records to pixi's tracing output.

    This is a small wrapper around :func:`get_logger` kept for readability.
    """

    return get_logger(name=name, level=level)


__all__ = [
    "TRACE_LEVEL",
    "PixiTracingHandler",
    "configure_logger",
    "get_logger",
]
