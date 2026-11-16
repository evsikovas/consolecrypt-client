import copy
import unittest

from consolecrypt_runner.errors import RunnerError
from consolecrypt_runner.parameters import parameters_digest, validate_parameters


class ParametersTests(unittest.TestCase):
    def test_all_options_are_bound_and_copied(self):
        value = {'extra_vars': {'service': 'nginx'}, 'tags': [], 'skip_tags': [], 'diff': False}
        original = parameters_digest(value)
        for key, changed in (('extra_vars', {'service': 'postgres'}), ('tags', ['deploy']),
                             ('skip_tags', ['delete']), ('diff', True)):
            self.assertNotEqual(original, parameters_digest(value | {key: changed}))
        frozen = validate_parameters(value)
        value['extra_vars'].clear()
        self.assertEqual(frozen['extra_vars'], {'service': 'nginx'})

    def test_parameters_cannot_smuggle_a_cli_option_or_credential(self):
        value = {'extra_vars': {}, 'tags': [], 'skip_tags': [], 'diff': False}
        for change in ({'tags': ['--connection=local']}, {'diff': 1}, {'skip_tags': ['x', 'x']},
                       {'extra_vars': {'ansible_password': 'forbidden'}}, {'raw_argv': ['-i', 'other']}):
            with self.subTest(change=change), self.assertRaises(RunnerError):
                validate_parameters(value | change)
