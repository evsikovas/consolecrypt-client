import importlib.util
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

spec = importlib.util.spec_from_file_location('export_source', Path(__file__).with_name('export-source.py'))
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class ExportTests(unittest.TestCase):
    def test_linux_build_stages_runner_sources_without_private_state(self):
        script = Path(__file__).with_name('stage-linux-source.py')
        public = (
            'runner/setup/environment.py',
            'runner/src/consolecrypt_runner/probe.py',
            'runner/requirements-linux-x86_64-py314.lock',
            'runner/vendor/pyyaml-6.0.3-py3-none-any.whl',
        )
        private = ('runner/state/private.json', 'runner/deployment/.env', 'runner/src/user.sqlite')
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary) / 'source'
            root.mkdir()
            subprocess.run(['git', 'init', '-q', str(root)], check=True)
            scripts = root / 'client/scripts'
            scripts.mkdir(parents=True)
            shutil.copyfile(Path(__file__).with_name('export-source.py'), scripts / 'export-source.py')
            for name in (*public, *private):
                fixture = root / name
                fixture.parent.mkdir(parents=True, exist_ok=True)
                fixture.write_bytes(b'synthetic build source')
            subprocess.run(['git', '-C', str(root), 'add', '--', *public, *private], check=True)
            output = Path(temporary) / 'staged'
            spec = importlib.util.spec_from_file_location('stage_linux_source', script)
            staging = importlib.util.module_from_spec(spec)
            spec.loader.exec_module(staging)
            self.assertEqual(staging.stage(root, output), len(public))
            for name in public:
                self.assertTrue((output / name).is_file(), name)
            for name in private:
                self.assertFalse((output / name).exists(), name)

    def test_runner_corresponding_source_is_explicit_and_private_state_stays_out(self):
        for name in ('runner/src/consolecrypt_runner/service/service_helper.py', 'runner/setup/environment.py',
                     'runner/deployment/bundle.py', 'runner/deployment/README.md', 'runner/pyproject.toml',
                     'runner/requirements-darwin-arm64-py315.lock', 'runner/vendor/package_pyyaml.py',
                     'runner/vendor/pyyaml-6.0.3-py3-none-any.whl', 'runner/vendor/README.md'):
            self.assertTrue(module.publishable(name), name)
        for name in ('runner/state/service-version.json', 'runner/keys/identity.v1',
                     'runner/deployment/.env', 'runner/src/user.sqlite', 'runner/target/release/cc-service-node'):
            self.assertFalse(module.publishable(name), name)

    def test_build_sources_and_licenses_are_kept(self):
        for path in ('LICENSE', 'PROTOCOL_CHANGELOG.md', 'client/flutter/assets/licenses/AGPL-3.0-only.txt',
                     'client/rust/rdp-core/vendor/PROVENANCE.md',
                     'client/flutter/vendor/security/glib/README.md',
                     'client/rust/platform-core/vendor/zbus/README.md',
                     'client/rust/rdp-core/vendor/ashpd/README.md',
                     'client/rust/ssh-core/vendor/ssh-key/README.md',
                     'client/flutter/assets/licenses/RDP-THIRD-PARTY-NOTICES.txt',
                     'client/flutter/pubspec.lock', 'client/rust/Cargo.lock',
                     'crates/models/src/lib.rs', 'server/migrations/001.sql',
                     'client/flutter/rust_builder/cargokit/LICENSE',
                     'client/packaging/macos/installer-background.png',
                     'server/.env.example', 'server/server.env.example',
                     'server/deploy/native/README.md', 'docs/public/BUILD_WINDOWS.md'):
            with self.subTest(path=path):
                self.assertTrue(module.publishable(path))

    def test_private_notes_builds_credentials_are_excluded(self):
        for path in ('LICENSE-MIT', 'LICENSE-APACHE', 'AGENTS.md', 'CLIENT_PLAN.md', 'docs/adr/design.md',
                     'client/AGENTS.md', 'client/packaging/macos/background-prompts.md',
                     'client/flutter/.dart_tool/cache', 'client/rust/target/release/app',
                     'server/.env', 'server/.env.production', 'server/server.env',
                     'server/server.env.production', 'server/SERVER.ENV', 'server/server.json',
                     'client/config.env', 'client/key.p12',
                     'client/secret.pem', 'client/app.apk', 'server/user.sqlite',
                     'windows_password.txt', 'client/windows_password.txt',
                     'client/docs/RDP_DEV_SPEC.md',
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
