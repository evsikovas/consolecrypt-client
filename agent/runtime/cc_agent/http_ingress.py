"""Bound raw request-head reads while retaining the stdlib HTTP parser."""

import io
import time


MAX_HEAD_BYTES = 16384
HEAD_TIMEOUT_SECONDS = 2.0


class HeadBudgetExceeded(Exception):
    pass


class IncompleteHead(Exception):
    pass


class HeadReader(io.RawIOBase):
    """One connection's pre-admission byte/time budget, not a request timeout.

    The server owns the socket. Closing this reader only closes its file wrapper.
    Buffered read-ahead counts conservatively against the same byte budget.
    """

    def __init__(self, connection, *, clock=None):
        super().__init__()
        self._connection = connection
        self._clock = time.monotonic if clock is None else clock
        self.deadline = self._clock() + HEAD_TIMEOUT_SECONDS
        self.received = 0

    def readable(self):
        return True

    def remaining_seconds(self):
        remaining = self.deadline - self._clock()
        if remaining <= 0:
            raise TimeoutError("request_head_deadline")
        return remaining

    def readinto(self, buffer):
        if self.closed:
            raise ValueError("closed_head_reader")
        if not len(buffer):
            return 0
        remaining = self.remaining_seconds()
        allowance = MAX_HEAD_BYTES - self.received
        if allowance <= 0:
            raise HeadBudgetExceeded()
        self._connection.settimeout(remaining)
        count = self._connection.recv_into(memoryview(buffer)[:min(len(buffer), allowance)])
        self.received += count
        self.remaining_seconds()
        # stdlib's header parser accepts EOF as an end-of-headers marker. Do not
        # let a truncated peer head inherit that behavior before admission.
        if count == 0:
            raise IncompleteHead()
        return count
