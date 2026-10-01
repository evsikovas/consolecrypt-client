import importlib.util
from pathlib import Path
import subprocess
import tempfile
import unittest

spec = importlib.util.spec_from_file_location('export_source', Path(__file__).with_name('export-source.py'))
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class ExportTests(unittest.TestCase):
    def test_build_sources_and_licenses_are_kept(self):
        for path in ('LICENSE', 'client/flutter/assets/licenses/AGPL-3.0-only.txt',
                     'client/flutter/pubspec.lock', 'client/rust/Cargo.lock',
                     'crates/models/src/lib.rs', 'server/migrations/001.sql',
                     'client/flutter/rust_builder/cargokit/LICENSE',
                     'client/packaging/macos/installer-background.png',
                     'server/.env.example', 'docs/public/BUILD_WINDOWS.md'):
            with self.subTest(path=path):
                self.assertTrue(module.publishable(path))

    def test_private_notes_builds_credentials_are_excluded(self):
        for path in ('LICENSE-MIT', 'LICENSE-APACHE', 'AGENTS.md', 'CLIENT_PLAN.md', 'docs/adr/design.md',
                     'client/AGENTS.md', 'client/packaging/macos/background-prompts.md',
                     'client/flutter/.dart_tool/cache', 'client/rust/target/release/app',
                     'server/.env', 'server/.env.production', 'client/key.p12',
                     'client/secret.pem', 'client/app.apk', 'server/user.sqlite',
                     'client/flutter/android/local.properties', '/etc/passwd',
                     'client/../private/file', 'client/.git/config',
                     'server/deploy/evsikov.values.yaml', 'server/web/index.html',
                     'server/.local/credentials.json'):
            with self.subTest(path=path):
                self.assertFalse(module.publishable(path))

    def test_export_does_not_copy_history_or_ignored_data(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp) / 'source'
            root.mkdir()
            subprocess.run(['git', 'init', '-q', str(root)], check=True)
            (root / '.gitignore').write_text('client/ignored.txt\n')
            (root / 'client').mkdir()
            (root / 'client/source.rs').write_text('fn main() {}')
            (root / 'client/ignored.txt').write_text('private fixture')
            (root / 'AGENTS.md').write_text('internal notes')
            out = Path(temp) / 'export'
            selected = module.export(root, out)
            self.assertIn('client/source.rs', selected)
            self.assertFalse((out / '.git').exists())
            self.assertFalse((out / 'AGENTS.md').exists())
            self.assertFalse((out / 'client/ignored.txt').exists())
            with self.assertRaises(ValueError):
                module.export(root, out)

    def test_symlink_outside_tree_is_rejected(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp) / 'source'
            root.mkdir()
            subprocess.run(['git', 'init', '-q', str(root)], check=True)
            (root / 'client').mkdir()
            outside = Path(temp) / 'outside.txt'
            outside.write_text('outside the exported tree')
            try:
                (root / 'client/leak.txt').symlink_to(outside)
            except OSError as error:
                if getattr(error, 'winerror', None) == 1314:
                    self.skipTest('Windows requires Developer Mode or symlink privilege')
                raise
            with self.assertRaises(ValueError):
                module.export(root, Path(temp) / 'export')


if __name__ == '__main__':
    unittest.main()
