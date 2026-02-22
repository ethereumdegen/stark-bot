"""Flask app factory for StarkBot modules."""

from flask import Flask
from starkbot_sdk.responses import status_response
import time


def create_app(module_name: str, *, status_extra_fn=None) -> Flask:
    """Create a Flask app with /rpc/status pre-wired.

    Args:
        module_name: The module identifier (used in status responses).
        status_extra_fn: Optional callable returning a dict of extra fields
            to include in the status response (e.g. database stats).

    Returns:
        A configured Flask application.
    """
    app = Flask(module_name)
    start_time = time.time()

    @app.route("/rpc/status")
    def _rpc_status():
        extra = status_extra_fn() if status_extra_fn else None
        return status_response(module_name, extra=extra, start_time=start_time)

    @app.errorhandler(404)
    def _not_found(e):
        from starkbot_sdk.responses import error
        return error("Not found", 404)

    @app.errorhandler(500)
    def _internal_error(e):
        from starkbot_sdk.responses import error
        return error("Internal server error", 500)

    return app
