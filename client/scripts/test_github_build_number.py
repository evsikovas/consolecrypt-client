"""Cross-platform and retry identities must never reuse published build numbers."""
import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location('github_build', Path(__file__).with_name('github-build-number.py'))
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class GitHubBuildNumberTests(unittest.TestCase):
    def test_unique_platforms_and_retries_increase_across_workflow_runs(self):
        first = [module.build_number('1', str(attempt), platform)
                 for attempt in range(1, 100) for platform in module.PLATFORMS]
        self.assertEqual(len(first), len(set(first)))
        self.assertGreater(min(first), 1393)
        self.assertLess(max(first), module.build_number('2', '1', 'macos'))

    def test_invalid_inputs_and_android_overflow_fail_closed(self):
        for args in [('0', '1', 'macos'), ('١', '1', 'macos'), ('1', '100', 'macos'),
                     ('1', '0', 'macos'), ('1', '1', 'other'), ('2100000', '1', 'android')]:
            with self.subTest(args=args), self.assertRaises(ValueError):
                module.build_number(*args)


if __name__ == '__main__':
    unittest.main()
