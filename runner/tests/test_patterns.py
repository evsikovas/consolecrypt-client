import unittest

from consolecrypt_runner.errors import RunnerError
from consolecrypt_runner.inventory import parse_yaml_inventory
from consolecrypt_runner.patterns import resolve_pattern


class PatternTests(unittest.TestCase):
    def setUp(self):
        self.inventory = parse_yaml_inventory('''all:
  hosts:
    lonely: {}
  children:
    web:
      hosts:
        web_a: {}
        web_b: {}
    production:
      hosts:
        web_a: {}
        db_a: {}
    maintenance:
      hosts:
        web_b: {}
''', 'fixture')

    def selected(self, pattern, limit=None):
        return [host.alias for host in resolve_pattern(self.inventory, pattern, limit=limit)]

    def test_exact_targets_are_expanded_before_approval(self):
        self.assertEqual(self.selected('web:&production:!maintenance'), ['web_a'])
        self.assertEqual(self.selected('web_*,db_*', 'production'), ['db_a', 'web_a'])
        self.assertEqual(self.selected('ungrouped'), ['lonely'])
        self.assertEqual(self.selected('!production'), ['lonely', 'web_b'])
        self.assertEqual(self.selected('not_found'), [])
        self.assertEqual(self.selected('all', '!all'), [])

    def test_unsupported_regex_slice_empty_terms_and_file_limits_are_not_guessed(self):
        for pattern in ('web[0]', '~(.*)+', 'web::all', '@outside', '', 'web &production'):
            with self.subTest(pattern=pattern), self.assertRaises(RunnerError):
                self.selected(pattern)
