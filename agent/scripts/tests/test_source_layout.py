import importlib.util
from pathlib import Path
import unittest

SCRIPT = Path(__file__).resolve().parents[1] / 'check_source.py'
spec = importlib.util.spec_from_file_location('agent_source_check', SCRIPT)
checker = importlib.util.module_from_spec(spec)
spec.loader.exec_module(checker)


class SourceBoundaryTests(unittest.TestCase):
    def test_build_and_license_sources(self):
        for name in ('identity/src/bin/cc-agent-identity.rs', 'identity/tests/vectors/CANONICAL_VECTORS.json',
                     'identity/vendor/libsqlite3-sys/sqlcipher/LICENSE',
                     'third_party/prometheus-agent/patches/0001-native-agent.patch', checker.WHEEL):
            with self.subTest(name=name):
                self.assertTrue(checker.source_name(name))

    def test_private_state_artifacts_and_noncanonical_paths(self):
        for name in ('target/debug/app', '.cache/source.tar', 'private/data.json',
                     'state/node.json', 'evidence/report.json', 'examples/fixture.rs',
                     '.env.prod', 'node.key', 'helper.elf', 'helper.test',
                     'third_party/unpinned.whl', '../outside', '/outside',
                     'runtime\\state.json', 'runtime//service.py', 'name:stream'):
            with self.subTest(name=name):
                self.assertFalse(checker.source_name(name))


if __name__ == '__main__':
    unittest.main()
