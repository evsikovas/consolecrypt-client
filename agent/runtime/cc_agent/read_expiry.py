"""Request-local verified expiry observations, not authority or revocation."""

import math
import time


# Rust wire::Timestamp domain; no new signing/wire format is defined here.
MAX_TIMESTAMP = 253402300799


class AuthorizationUnavailable(Exception):
    def __init__(self):
        super().__init__("authorization_unavailable")


def _os_now():
    """Fixed OS observation; deliberately separate from telemetry clock injection."""
    return time.time()


class ReadExpiryGuard:
    """Capture a trusted admission's bounds and check before data exposure.

    Both None preserves legacy trusted callbacks with no expiry guarantee.
    A coarse verified_at cannot detect fractional rollback before the first
    observation; this local guard is neither a durable clock nor a revoke fence.
    """

    __slots__ = ("_verified_at", "_expires_at", "_highwater")

    def __init__(self, verified_at, verified_expires_at):
        self._highwater = None
        if not (verified_at is None and verified_expires_at is None):
            if (type(verified_at) is not int or type(verified_expires_at) is not int
                    or not 0 <= verified_at < verified_expires_at <= MAX_TIMESTAMP):
                raise AuthorizationUnavailable()
        self._verified_at, self._expires_at = verified_at, verified_expires_at
        self.check()

    def check(self):
        if self._verified_at is None:
            return
        try:
            now = _os_now()
            if (type(now) not in (int, float)
                    or type(now) is float and not math.isfinite(now)
                    or not 0 <= now <= MAX_TIMESTAMP
                    or now < self._verified_at or now >= self._expires_at
                    or self._highwater is not None and now < self._highwater):
                raise AuthorizationUnavailable()
            self._highwater = now
        except Exception:
            raise AuthorizationUnavailable() from None
