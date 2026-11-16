import copy
import importlib.util
import json
from pathlib import Path
import unittest

from consolecrypt_runner import event_privacy as privacy


class EventPrivacyTests(unittest.TestCase):
    def test_facts_and_module_inputs_are_omitted_without_mutating_execution(self):
        original = {'changed': True, 'stdout': 'public', 'ansible_facts': {'secret': 'fact-secret'},
                    'invocation': {'module_args': {'password': 'argument-secret'}},
                    'results': [{'ansible_facts': {'nested': 'nested-fact'}, 'changed': False}]}
        before = copy.deepcopy(original)
        projected = privacy.diagnostic_result(original)
        encoded = json.dumps(projected)
        for secret in ('fact-secret', 'argument-secret', 'nested-fact'):
            self.assertNotIn(secret, encoded)
        self.assertEqual(original, before)
        self.assertTrue(projected['changed'])
        self.assertEqual(projected['stdout'], 'public')

    def test_nested_no_log_censors_before_copying_any_result_fields(self):
        original = {'results': [{'stdout': 'visible'}, {'_ansible_no_log': True,
                    'stdout': 'loop-secret', 'results': [{'password': 'nested-secret'}]}]}
        projected = privacy.diagnostic_result(original)
        self.assertEqual(projected['results'][1], {'censored': True})
        self.assertNotIn('secret', json.dumps(projected))
        self.assertEqual(projected['results'][0]['stdout'], 'visible')

    def test_depth_nodes_and_text_are_bounded(self):
        deep = 'unreachable-marker'
        for _ in range(500): deep = {'child': deep}
        projected = privacy.diagnostic_result({'deep': deep, 'large': ['x' * 100000] * 10000})
        encoded = json.dumps(projected)
        self.assertLess(len(encoded), 100000)
        self.assertNotIn('unreachable-marker', encoded)
        self.assertIn('truncated', encoded.lower())


if __name__ == '__main__': unittest.main()
