"""Trusted one-read composition; implementations must use real retained authority.

No handle, transport, positive default guard or permission-from-boolean exists
here. The explicit local provider owns its private worker and opaque custody.
"""

from abc import ABC, abstractmethod

from .read_expiry import AuthorizationUnavailable


class ReadAccessUnavailable(AuthorizationUnavailable):
    pass


class ReadAccessGuard(ABC):
    @abstractmethod
    def check(self, *, agent_id, operation, resource):
        raise ReadAccessUnavailable()

    @abstractmethod
    def finalize(self, *, agent_id, operation, resource):
        raise ReadAccessUnavailable()

    @abstractmethod
    def retire(self):
        raise ReadAccessUnavailable()

    def __repr__(self):
        return "ReadAccessGuard([redacted])"

    def __reduce_ex__(self, protocol):
        raise TypeError("read_access_guard_not_serializable")


class ReadAdmission:
    """A non-serializing local wrapper, not a Permit or an authority proof."""

    __slots__ = ("_principal", "_guard")

    def __init__(self, principal, guard):
        if not isinstance(guard, ReadAccessGuard):
            raise TypeError("typed_read_access_guard_required")
        object.__setattr__(self, "_principal", principal)
        object.__setattr__(self, "_guard", guard)

    def __setattr__(self, name, value):
        raise AttributeError("immutable_read_admission")

    def __delattr__(self, name):
        raise AttributeError("immutable_read_admission")

    @property
    def principal(self):
        return self._principal

    @property
    def guard(self):
        return self._guard

    def __repr__(self):
        return "ReadAdmission([redacted])"

    def __reduce_ex__(self, protocol):
        raise TypeError("read_admission_not_serializable")
