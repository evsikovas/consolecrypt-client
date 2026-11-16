"""Bounded corrupt audit timestamps do not abort startup or mutate the log."""

import json
from pathlib import Path
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from cc_agent.audit import AuditLog


AGENT = "be42e704-c1ac-4c4f-9dc5-b08d384e6ab0"
RESOURCE = "36fdf84b-e33a-46f3-b9b4-e301215b1dd3"


def event(timestamp):
    return dict(timestamp=timestamp, agent_id=AGENT, operation="telemetry.read",
                resource=RESOURCE, reason="admitted")


class AuditNumericTests(unittest.TestCase):
    def test_restore_overflowing_timestamp_preserves_corrupt_file_and_blocks_repair(self):
        huge = int("9" * 310)
        bad = json.dumps(event(huge), separators=(",", ":")).encode() + b"\n"
        self.assertLessEqual(len(bad), 512)
        prefix = json.dumps(event(100.0), separators=(",", ":")).encode() + b"\n"
        original = prefix + bad
        with tempfile.TemporaryDirectory(prefix="cc-audit-numeric-") as directory:
            path = Path(directory) / "audit.jsonl"
            path.write_bytes(original)
            audit = AuditLog(directory)
            self.assertEqual(audit.status()["state"], "corrupt_audit_refused")
            self.assertEqual(audit.events(), [])
            self.assertEqual(audit.status()["events"], 0)
            self.assertEqual(audit.status()["bytes"], 0)
            self.assertEqual(path.read_bytes(), original)
            before = audit.status()
            self.assertFalse(audit(event(101.0)))
            self.assertEqual(audit.status(), before)
            self.assertEqual(path.read_bytes(), original)
            self.assertFalse((Path(directory) / ".audit.pending").exists())
        self.assertFalse(Path(directory).exists())

    def test_direct_overflowing_timestamp_returns_false_before_mutation(self):
        audit = AuditLog()
        self.assertTrue(audit(event(100.0)))
        before_events, before_status = audit.events(), audit.status()
        self.assertFalse(audit(event(int("9" * 310))))
        self.assertEqual(audit.events(), before_events)
        self.assertEqual(audit.status(), before_status)
        self.assertTrue(audit(event(101.0)))
        self.assertEqual(len(audit.events()), 2)


if __name__ == "__main__":
    unittest.main()
