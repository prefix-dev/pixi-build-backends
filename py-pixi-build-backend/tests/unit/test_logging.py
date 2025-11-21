import logging

from pixi_build_backend import logging as pixi_logging


def test_handler_dispatches_levels(monkeypatch):
    calls = []

    def recorder(kind):
        def _impl(message, logger_name):
            calls.append((kind, message, logger_name))

        return _impl

    monkeypatch.setattr(pixi_logging, "_native_trace", recorder("trace"))
    monkeypatch.setattr(pixi_logging, "_native_debug", recorder("debug"))
    monkeypatch.setattr(pixi_logging, "_native_info", recorder("info"))
    monkeypatch.setattr(pixi_logging, "_native_warn", recorder("warn"))
    monkeypatch.setattr(pixi_logging, "_native_error", recorder("error"))

    logger = logging.getLogger("pixi.test.logging.handler")
    logger.handlers.clear()
    handler = pixi_logging.PixiTracingHandler()
    logger.addHandler(handler)
    logger.setLevel(pixi_logging.TRACE_LEVEL)
    logger.propagate = False

    logger.trace("trace message")
    logger.debug("debug message")
    logger.info("info message")
    logger.warning("warn message")
    logger.error("error message")

    assert calls == [
        ("trace", "trace message", "pixi.test.logging.handler"),
        ("debug", "debug message", "pixi.test.logging.handler"),
        ("info", "info message", "pixi.test.logging.handler"),
        ("warn", "warn message", "pixi.test.logging.handler"),
        ("error", "error message", "pixi.test.logging.handler"),
    ]


def test_get_logger_installs_single_handler():
    logger_name = "pixi.test.logging.singleton"
    logger = pixi_logging.get_logger(logger_name)
    handlers = [h for h in logger.handlers if isinstance(h, pixi_logging.PixiTracingHandler)]
    assert len(handlers) == 1

    second = pixi_logging.get_logger(logger_name)
    assert second is logger
    handlers = [h for h in logger.handlers if isinstance(h, pixi_logging.PixiTracingHandler)]
    assert len(handlers) == 1

    for handler in logger.handlers[:]:
        logger.removeHandler(handler)
