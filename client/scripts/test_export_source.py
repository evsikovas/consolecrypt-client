import importlib.util
from pathlib import Path
import subprocess
import tempfile
import unittest

spec = importlib.util.spec_from_file_location('export_source', Path(__file__).with_name('export-source.py'))
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class ExportTests(unittest.TestCase):
    def test_agent_corresponding_source_is_kept(self):
        for path in ('agent/README.md', 'agent/BUILDING.md', 'agent/THIRD_PARTY.md',
                     'agent/LICENSE', 'agent/DEVICE_BUILD_MANIFEST.json',
                     'agent/identity/src/bin/cc-agent-identity.rs', 'agent/identity/Cargo.lock',
                     'agent/identity/vendor/libsqlite3-sys/sqlite3/sqlite3.c',
                     'agent/identity/vendor/libsqlite3-sys/CONSOLECRYPT_BACKPORT.md',
                     'agent/identity/vendor/libsqlite3-sys/sqlcipher/LICENSE',
                     'agent/third_party/prometheus-agent/upstream/NOTICE',
                     'agent/third_party/prometheus-agent/patches/0001-native-agent.patch',
                     module.AGENT_WHEEL):
            with self.subTest(path=path):
                self.assertTrue(module.publishable(path))

    def test_agent_runtime_state_and_build_products_are_excluded(self):
        for path in ('agent/target/debug/cc-agent-identity', 'agent/.cache/archive.tar',
                     'agent/evidence/result.json', 'agent/state/node.json',
                     'agent/private/public-frame.json', 'agent/examples/fixture.rs',
                     'agent/key.pem', 'agent/package.elf', 'agent/package.test',
                     'agent/third_party/other.whl', 'agent/INTERNAL_NOTES.md',
                     'agent/runtime\\state.json', 'agent/runtime/:stream'):
            with self.subTest(path=path):
                self.assertFalse(module.publishable(path))

    def test_agent_executable_without_extension_and_changed_pinned_wheel_deny_export(self):
        for name, payload in (('agent/helper', b'\x7fELFcompiled fixture'),
                              (module.AGENT_WHEEL, b'not the pinned wheel')):
            with self.subTest(name=name), tempfile.TemporaryDirectory() as temp:
                root = Path(temp) / 'source'
                root.mkdir()
                subprocess.run(['git', 'init', '-q', str(root)], check=True)
                (root / '.gitignore').write_text('')
                file = root / name
                file.parent.mkdir(parents=True)
                file.write_bytes(payload)
                out = Path(temp) / 'export'
                with self.assertRaises(ValueError):
                    module.export(root, out)
                self.assertFalse(out.exists())

    def test_runner_corresponding_source_is_explicit_and_private_state_stays_out(self):
        for name in ('runner/src/consolecrypt_runner/service/service_helper.py', 'runner/setup/environment.py',
                     'runner/deployment/bundle.py', 'runner/deployment/README.md', 'runner/pyproject.toml'):
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
