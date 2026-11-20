from __future__ import annotations

import json
import math
import uuid
from typing import BinaryIO

from . import PROTOCOL_VERSION
from .errors import RunnerError, require

MAX_FRAME_BYTES = 1024 * 1024
MAX_DEPTH = 32


def _pairs(pairs):
    result = {}
    for key, value in pairs:
        require(key not in result, "duplicate_field", "Duplicate JSON field")
        result[key] = value
    return result


def validate_tree(value, depth=0):
    require(depth <= MAX_DEPTH, "message_depth", "Message nesting exceeds limit")
    if isinstance(value, dict):
        require(all(isinstance(key, str) for key in value), "message_type", "JSON keys must be strings")
        for key, child in value.items():
            require(len(key) <= 1024, "message_key", "Message key exceeds limit")
            validate_tree(child, depth + 1)
    elif isinstance(value, (list, tuple)):
        for child in value:
            validate_tree(child, depth + 1)
    elif isinstance(value, float):
        require(math.isfinite(value), "message_number", "Non-finite JSON number")
    elif value is not None and not isinstance(value, (str, int, bool)):
        raise RunnerError("message_type", "Unsupported JSON value")


def encode(value) -> bytes:
    validate_tree(value)
    return json.dumps(value, sort_keys=True, ensure_ascii=False, separators=(",", ":"), allow_nan=False).encode("utf-8")


def decode(raw: bytes, *, max_bytes=MAX_FRAME_BYTES):
    require(len(raw) <= max_bytes, "message_size", "Message exceeds its byte limit")
    try:
        value = json.loads(raw, object_pairs_hook=_pairs, parse_constant=lambda _: (_ for _ in ()).throw(
            RunnerError("message_number", "Non-finite JSON number")))
    except (ValueError, UnicodeError, RecursionError) as error:
        raise RunnerError("invalid_json", "Invalid JSON message") from error
    validate_tree(value)
    return value


def uuid_field(value, name="id") -> str:
    require(isinstance(value, str) and len(value) == 36, "invalid_id", f"Invalid {name}")
    try:
        parsed = uuid.UUID(value)
    except ValueError as error:
        raise RunnerError("invalid_id", f"Invalid {name}") from error
    require(str(parsed) == value, "invalid_id", f"Non-canonical {name}")
    return value


def read_message(stream: BinaryIO):
    raw = stream.readline(MAX_FRAME_BYTES + 2)
    if not raw:
        return None
    require(len(raw) <= MAX_FRAME_BYTES and raw.endswith(b"\n"), "message_size", "Truncated or oversized message")
    message = decode(raw)
    require(isinstance(message, dict), "invalid_request", "Request must be an object")
    return message


def validate_request(message):
    require(set(message) == {"version", "request_id", "operation", "body"}, "invalid_request", "Unknown request fields")
    require(type(message["version"]) is int and message["version"] == PROTOCOL_VERSION,
            "protocol_version", "Unsupported protocol version")
    uuid_field(message["request_id"], "request ID")
    require(isinstance(message["operation"], str) and len(message["operation"]) <= 64,
            "invalid_request", "Invalid operation")
    require(isinstance(message["body"], dict), "invalid_request", "Request body must be an object")
    return message


def read_request(stream: BinaryIO):
    message = read_message(stream)
    return None if message is None else validate_request(message)


def write_frame(stream: BinaryIO, frame):
    raw = encode(frame)
    require(len(raw) + 1 <= MAX_FRAME_BYTES, "message_size", "Reply exceeds one MiB")
    stream.write(raw + b"\n")
    stream.flush()
