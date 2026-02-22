"""Standard RPC response helpers for StarkBot modules.

All module RPC endpoints should return JSON in the envelope format:
    {"success": true, "data": ...}
    {"success": false, "error": "..."}
"""

from flask import jsonify
import time


def success(data):
    """Return a successful RPC response."""
    return jsonify({"success": True, "data": data})


def error(msg, status=400):
    """Return an error RPC response with the given HTTP status code."""
    return jsonify({"success": False, "error": msg}), status


def status_response(module_name, *, extra=None, start_time=None):
    """Return a standard health-check response for /rpc/status.

    Args:
        module_name: The module identifier.
        extra: Optional dict of additional fields to include in the data.
        start_time: Optional epoch timestamp for uptime calculation.
    """
    data = {"status": "running", "module": module_name}
    if start_time is not None:
        data["uptime_secs"] = int(time.time() - start_time)
    if extra:
        data.update(extra)
    return jsonify({"success": True, "data": data})
