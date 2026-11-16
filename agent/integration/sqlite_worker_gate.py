"""Private persistent Rust admission channel; never SQL or raw signing over HTTP."""
import json
import math
import os
from pathlib import Path
import queue
import subprocess
import threading
import time

from .identity_gate import GateConfig, IdentityGate, strict_json


class WorkerProtocolError(ValueError):
    pass


class WorkerChannel:
    def __init__(self, argv, *, timeout=5.0, process_factory=subprocess.Popen):
        self.argv, self.timeout, self.factory = list(argv), timeout, process_factory
        self.lock = threading.RLock()
        self.process = None
        self.reader = None
        self.writer = None
        self.frames = None

    @staticmethod
    def _read(process, frames):
        try:
            while True:
                raw = process.stdout.readline(4097)
                if not raw or len(raw) > 4096 or not raw.endswith(b"\n"):
                    frames.put_nowait(WorkerProtocolError("worker frame unavailable or oversized"))
                    return
                frames.put_nowait(raw)
        except (OSError, ValueError, queue.Full):
            pass

    def _start(self):
        if self.process is not None and self.process.poll() is None:
            return
        self._close()
        env = {key: value for key, value in os.environ.items()
               if key.upper() in {"SYSTEMROOT", "WINDIR", "TEMP", "TMP", "LANG", "LC_ALL"}}
        self.process = self.factory(self.argv, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL, shell=False, bufsize=0, env=env)
        self.frames = queue.Queue(maxsize=1)
        self.reader = threading.Thread(target=self._read, args=(self.process, self.frames), daemon=True)
        self.reader.start()

    def exchange(self, request):
        return self.exchange_bound(request)[1]

    def exchange_bound(self, request, *, expected_process=None, timeout=None):
        """Capture the serving generation with its response under one lock.

        A bound follow-up never starts or closes another generation. A shorter
        internal budget still covers queue, full write and response together.
        """
        budget = self.timeout
        if timeout is not None:
            if (type(timeout) not in (int, float) or not 0 < timeout <= budget
                    or not math.isfinite(timeout)):
                raise WorkerProtocolError("invalid shortened worker budget")
            budget = timeout
        deadline = time.monotonic() + budget
        raw = json.dumps(request, separators=(",", ":"), allow_nan=False).encode() + b"\n"
        if len(raw) > 16 * 1024:
            raise WorkerProtocolError("worker input budget")
        if not self.lock.acquire(timeout=max(0, deadline - time.monotonic())):
            # This caller owns no exchange and must not cancel another one.
            raise subprocess.TimeoutExpired(self.argv[0], budget)
        try:
            if time.monotonic() >= deadline:
                raise subprocess.TimeoutExpired(self.argv[0], budget)
            if expected_process is not None and (self.process is not expected_process
                    or expected_process.poll() is not None):
                # Known not dispatched: do not enter the failure-close path or
                # start/terminate the current generation owned by another call.
                raise WorkerProtocolError("worker generation unavailable")
            try:
                if expected_process is None:
                    self._start()
                process = self.process
                if time.monotonic() >= deadline:
                    raise queue.Empty
                if not self.frames.empty():
                    raise WorkerProtocolError("unsolicited worker response")
                # Raw pipes may short-write or block before the worker reads.
                # Queue wait, full write and response share one deadline.
                written = queue.Queue(maxsize=1)
                self.writer = threading.Thread(target=self._write,
                    args=(self.process, raw, written), daemon=True)
                self.writer.start()
                outcome = written.get(timeout=max(0, deadline - time.monotonic()))
                if isinstance(outcome, Exception):
                    raise outcome
                self.writer.join(timeout=0)
                response = self.frames.get(timeout=max(0, deadline - time.monotonic()))
                if isinstance(response, Exception):
                    raise response
                frame = strict_json(response)
                if not isinstance(frame, dict) or type(frame.get("ok")) is not bool:
                    raise WorkerProtocolError("invalid worker response")
                if frame["ok"]:
                    if set(frame) != {"ok", "metadata"} or not isinstance(frame["metadata"], dict):
                        raise WorkerProtocolError("invalid worker metadata")
                elif set(frame) != {"ok", "code"} or not isinstance(frame["code"], str) or len(frame["code"]) > 64:
                    raise WorkerProtocolError("invalid worker denial")
                self._validate_frame(frame)
                return process, frame
            except queue.Empty:
                self._close(interrupt=True)
                raise subprocess.TimeoutExpired(self.argv[0], budget) from None
            except (OSError, ValueError, RecursionError):
                self._close(interrupt=True)
                raise
        finally:
            self.lock.release()

    def _validate_frame(self, frame):
        """Optional stricter protocol checks while the serving generation is locked.

        A protocol-specific subclass may reject additional outcomes. Exceptions
        follow the owned failure-close path; this hook supplies no authority.
        """

    @staticmethod
    def _write(process, raw, written):
        try:
            remaining = memoryview(raw)
            while remaining:
                count = process.stdin.write(remaining)
                if not count:
                    raise BrokenPipeError("worker input closed")
                remaining = remaining[count:]
            process.stdin.flush()
            written.put_nowait(True)
        except (OSError, ValueError) as error:
            written.put_nowait(error)

    def _close(self, interrupt=False):
        process, reader, writer = self.process, self.reader, self.writer
        self.process, self.reader, self.writer, self.frames = None, None, None, None
        if process is not None:
            # Terminate before closing a pipe whose writer may be blocked.
            if interrupt and process.poll() is None:
                process.terminate()
            try:
                if not interrupt:
                    process.stdin.close()
            except (OSError, ValueError):
                pass
            try:
                process.wait(timeout=0.5)
            except subprocess.TimeoutExpired:
                process.terminate()
                try:
                    process.wait(timeout=1)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait(timeout=1)
            process.stdin.close()
            process.stdout.close()
        if reader is not None and reader is not threading.current_thread():
            reader.join(timeout=1)
        if writer is not None and writer is not threading.current_thread():
            writer.join(timeout=1)

    def close(self):
        with self.lock:
            self._close()


class SqliteWorkerGate(IdentityGate):
    def __init__(self, config, agent_id, resources, **kwargs):
        self.channel = WorkerChannel([str(config.executable), "worker-sqlite", "--db-dir", str(config.store),
            "--anchor-dir", str(config.anchor), "--owner-pin", str(config.owner_pin)],
            timeout=config.timeout_seconds)
        super().__init__(config, agent_id, resources, run=self._run_worker, **kwargs)

    def _run_worker(self, argv, **kwargs):
        request = strict_json(kwargs["input"])
        if set(request) != {"now", "lease", "proof", "freshness", "operation"}:
            raise WorkerProtocolError("unexpected local admission fields")
        # The Rust worker observes its own OS clock, not a caller-supplied now.
        request.pop("now")
        response = self.channel.exchange({**request, "action": "admit"})
        if not response["ok"]:
            return subprocess.CompletedProcess(argv, 1, b"", b"")
        metadata = response["metadata"]
        if set(metadata) != {"agentId", "operation", "expiresAt"}:
            self.channel.close()
            raise WorkerProtocolError("unexpected admission metadata")
        return subprocess.CompletedProcess(argv, 0, json.dumps(metadata).encode(), b"")

    def close(self):
        self.channel.close()


