import unittest

from consolecrypt_runner.execution import InteractiveExecutor
from consolecrypt_runner.errors import RunnerError


class ExecutionLifecycleTests(unittest.TestCase):
    def test_delayed_unlock_message_cannot_override_newer_lock(self):
        executor = InteractiveExecutor(None, None, None)
        self.addCleanup(executor.close)
        self.assertFalse(executor.vault_state(False)['locked'])
        self.assertFalse(executor.vault_state(False, 1)['locked'])
        self.assertTrue(executor.vault_state(True, 3)['locked'])
        self.assertTrue(executor.vault_state(False, 2)['locked'])
        with self.assertRaises(RunnerError): executor.vault_state(False, 3)
        with self.assertRaises(RunnerError): executor.vault_state(False)
        with self.assertRaises(RunnerError): executor.vault_state(False, True)
        self.assertTrue(executor.vault_state(True, 3)['locked'])
        self.assertFalse(executor.vault_state(False, 4)['locked'])
