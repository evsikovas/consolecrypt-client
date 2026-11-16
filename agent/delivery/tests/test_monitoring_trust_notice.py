"""Offline placement-disclosure tests. No host installation or external process except the plan CLI."""
import hashlib
import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

import yaml

sys.dont_write_bytecode = True
DIRECTORY = Path(__file__).resolve().parents[1]

def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module

plan = load("cc_mon11_candidate_plan", DIRECTORY / "monitoring_plan.py")
sys.path.insert(0, str(DIRECTORY))
from monitoring_prefix import parse_plan

class TrustNoticeTests(unittest.TestCase):
    def test_owned_render_preserves_legacy_plan_contract_and_exposes_fixed_notice(self):
        with tempfile.TemporaryDirectory(prefix="mon11-render-") as own:
            output = plan.MonitoringPlan().render(Path(own) / "review")
            self.assertEqual((output / "TRUST_NOTICE.txt").read_text(encoding="utf8"), plan.METRICS_TRUST_NOTICE + "\n")
            payload = json.loads((output / "plan.json").read_bytes())
            self.assertEqual(set(payload), {"cc_monitoring_plan", "cc_prometheus_version", "cc_prometheus_sha256", "cc_prometheus_config"})
            self.assertEqual(set(payload["cc_monitoring_plan"]), {"cpu_cores", "memory_mib", "disk_gib", "retention_days", "retention_gib", "receiver_enabled", "alertmanager_enabled", "expected_samples_per_second", "estimated_sample_bytes", "estimate_excludes", "reference_benchmark"})
            self.assertFalse(parse_plan(payload).receiver_enabled)
            self.assertEqual(json.loads((output / "prometheus.yml").read_bytes()), payload["cc_prometheus_config"])
            self.assertEqual(set(path.name for path in output.iterdir()), {"plan.json", "prometheus.yml", "TRUST_NOTICE.txt"})

    def test_real_cli_keeps_stdout_path_and_discloses_trust_without_host_action(self):
        with tempfile.TemporaryDirectory(prefix="mon11-cli-") as own:
            target = Path(own) / "review"
            result = subprocess.run([sys.executable, "-I", "-B", str(DIRECTORY / "monitoring_plan.py"), "--output", str(target)], stdin=subprocess.DEVNULL, capture_output=True, timeout=10, check=True, encoding="utf8")
            self.assertEqual(result.stdout.strip(), str(target))
            self.assertEqual(result.stderr.strip(), plan.METRICS_TRUST_NOTICE)
            self.assertIn("do not automatically inherit ConsoleCrypt vault end-to-end encryption", result.stderr)
            self.assertTrue((target / "TRUST_NOTICE.txt").is_file())

    def test_actual_yaml_discloses_before_any_approval_or_install_and_preserves_all_existing_guards(self):
        document = yaml.safe_load((DIRECTORY / "monitoring-node.yml").read_text(encoding="utf8"))
        notice = document[0]["pre_tasks"].pop(0)
        self.assertEqual(notice["ansible.builtin.debug"]["msg"], plan.METRICS_TRUST_NOTICE)
        self.assertIs(notice["become"], False)
        self.assertEqual(notice["tags"], ["always"])
        self.assertEqual(set(notice), {"name", "ansible.builtin.debug", "become", "tags"})
        self.assertIs(document[0]["vars"]["cc_monitoring_install_approved"], False)
        self.assertIn("cc_monitoring_install_approved | bool", document[0]["pre_tasks"][0]["ansible.builtin.assert"]["that"])
        # This is the actual pre-change playbook's parsed semantic digest, not an approval witness.
        self.assertEqual(hashlib.sha256(json.dumps(document,sort_keys=True,separators=(",", ":")).encode()).hexdigest(), "84a423edbdadde266e002b02a032b1796580d2ed6b330502e5b8eb263b5b0d4a")

if __name__ == "__main__":
    unittest.main(verbosity=2)
