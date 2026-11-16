"""Closed reader error namespace over the existing private owned transport."""
from .sqlite_worker_gate import WorkerChannel, WorkerProtocolError

READER_DENIED = frozenset({"InvalidWire", "InvalidSignature", "WrongIssuer", "WrongContext",
    "WrongAssociation", "WrongDevice", "WrongTarget", "WrongOperation", "WrongHead",
    "Expired", "Replay", "NotFound", "Revoked", "CapabilityDenied"})
READER_UNAVAILABLE = frozenset({"InvalidTime", "ClockUnavailable", "Quota", "Conflict",
    "Deadline", "LostHandle", "Storage", "UncertainCommit", "Entropy", "Protocol"})
_FATAL = frozenset({"ClockUnavailable", "Storage", "UncertainCommit", "Entropy", "Protocol"})


class ReaderWorkerChannel(WorkerChannel):
    def _validate_frame(self, frame):
        if frame["ok"] is False:
            code = frame["code"]
            if code not in READER_DENIED | READER_UNAVAILABLE or code in _FATAL:
                # Executed under the exchange lock before returning a frame.
                # Closing cannot accidentally terminate a replacement generation.
                raise WorkerProtocolError("reader_worker_unavailable")
