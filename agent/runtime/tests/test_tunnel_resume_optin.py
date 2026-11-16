"""Policy DATA checks only; no network, child process, Permit or drain proof."""
import copy
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest

AGENT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(AGENT))
sys.path.insert(0, str(AGENT / 'runtime'))
from cc_agent.tunnel_policy_v2 import PolicyV2
from cc_agent.tunnel import TunnelUnavailable


class ExplicitResumePolicyTests(unittest.TestCase):
    def setUp(self):
        self.root = tempfile.TemporaryDirectory(prefix='cc-resume-intent-')
        self.addCleanup(self.root.cleanup)
        self.path = Path(self.root.name) / 'policy.json'
        route = json.loads((AGENT / 'identity/src/tunnel_policy_vector.json').read_text())
        self.value = dict(format='cc-agent-tunnel-route-policy/v2', route=route, resumeOriginal=False)

    def write(self, value):
        self.path.write_text(json.dumps(value))
        os.chmod(self.path, 0o600)
        return PolicyV2(self.path)

    def test_false_commits_a_distinct_first_admit_policy_and_denies_resume(self):
        policy = self.write(self.value)
        policy.require_mode('admit')
        with self.assertRaises(TunnelUnavailable): policy.require_mode('resume')
        first_digest = policy.digest
        other = copy.deepcopy(self.value); other['resumeOriginal'] = True
        policy = self.write(other)
        self.assertNotEqual(first_digest, policy.digest)
        policy.require_mode('admit'); policy.require_mode('resume')

    def test_policy_requires_an_exact_boolean_and_closed_mode(self):
        for value in [None, 0, 1, 'true', [], {}]:
            other = copy.deepcopy(self.value); other['resumeOriginal'] = value
            with self.assertRaises(TunnelUnavailable): self.write(other)
        missing = copy.deepcopy(self.value); del missing['resumeOriginal']
        with self.assertRaises(TunnelUnavailable): self.write(missing)
        policy = self.write(self.value)
        for mode in [None, True, 1, 'retry', 'Resume', 'admit ']:
            with self.assertRaises(TunnelUnavailable): policy.require_mode(mode)

    def test_changing_captured_choice_cannot_retarget_the_existing_policy(self):
        policy = self.write(self.value)
        changed = copy.deepcopy(self.value); changed['resumeOriginal'] = True
        self.path.write_text(json.dumps(changed))
        with self.assertRaises(TunnelUnavailable): policy.require_mode('admit')
        with self.assertRaises(TunnelUnavailable): policy.require_mode('resume')


if __name__ == '__main__':
    unittest.main()
