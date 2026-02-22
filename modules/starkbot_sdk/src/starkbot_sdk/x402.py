"""x402 payment helpers for StarkBot modules.

Provides utilities for building HTTP 402 responses with payment-required
headers and extracting/verifying x402 payment receipts from incoming requests.
"""

import base64
import json
from flask import jsonify, Response


def payment_required(
    price: str,
    currency: str = "USDC",
    payee: str = "",
    network: str = "base-sepolia",
    *,
    description: str = "",
    extra: dict | None = None,
) -> Response:
    """Return an HTTP 402 response with x402 payment-required headers.

    Args:
        price: Amount required (e.g. "0.01").
        currency: Token symbol (e.g. "USDC").
        payee: Wallet address to receive payment.
        network: Chain identifier (e.g. "base-sepolia", "base").
        description: Human-readable description of what the payment is for.
        extra: Additional fields to include in the payment requirement.
    """
    requirement = {
        "price": price,
        "currency": currency,
        "payee": payee,
        "network": network,
    }
    if description:
        requirement["description"] = description
    if extra:
        requirement.update(extra)

    encoded = base64.b64encode(json.dumps(requirement).encode()).decode()

    resp = jsonify({
        "error": "Payment Required",
        "payment_required": requirement,
    })
    resp.status_code = 402
    resp.headers["Payment-Required"] = encoded
    resp.headers["Content-Type"] = "application/json"
    return resp


def extract_payment(request) -> dict | None:
    """Extract and decode an x402 payment receipt from the request.

    Looks for the `X-PAYMENT` header. Returns the decoded JSON dict,
    or None if no payment header is present.
    """
    header = request.headers.get("X-Payment") or request.headers.get("X-PAYMENT")
    if not header:
        return None
    try:
        decoded = base64.b64decode(header)
        return json.loads(decoded)
    except (ValueError, json.JSONDecodeError):
        # Try as raw JSON (some clients may not base64-encode)
        try:
            return json.loads(header)
        except (ValueError, json.JSONDecodeError):
            return None


def has_payment(request) -> bool:
    """Check if the request includes an x402 payment header."""
    return bool(request.headers.get("X-Payment") or request.headers.get("X-PAYMENT"))
