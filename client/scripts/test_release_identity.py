"""Stable upgrade identity and licence preflights, without user OS stores."""
import importlib.util
from pathlib import Path
import shutil
import re
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location('release_identity', Path(__file__).with_name('verify-release-identity.py'))
policy = importlib.util.module_from_spec(spec)
spec.loader.exec_module(policy)
FILES = (
    'client/flutter/pubspec.yaml', 'client/flutter/lib/app/app_info.dart',
    'client/rust/platform-core/src/dirs.rs', 'client/rust/platform-core/src/os_store.rs',
    'client/rust/platform-core/src/linux_secret_service.rs',
    'client/flutter/macos/Runner/Configs/AppInfo.xcconfig',
    'client/flutter/ios/Runner.xcodeproj/project.pbxproj',
    'client/flutter/rust_builder/ios/Classes/CcBridgePlugin.swift',
    'client/flutter/android/app/build.gradle.kts',
    'client/flutter/android/app/src/main/AndroidManifest.xml',
    'client/flutter/android/app/src/main/kotlin/io/consolecrypt/consolecrypt/MainActivity.kt',
    'client/flutter/windows/CMakeLists.txt', 'client/packaging/windows/ConsoleCrypt.iss',
    'client/flutter/linux/CMakeLists.txt', 'client/packaging/linux/consolecrypt.desktop',
    'client/flutter/lib/updates/update_service.dart', 'client/flutter/macos/Runner/UpdateBridge.swift',
    'client/flutter/rust_builder/ios/cc_bridge.podspec',
    'client/flutter/rust_builder/macos/cc_bridge.podspec',
    'client/rust/rdp-core/THIRD_PARTY_NOTICES.txt',
    'client/flutter/assets/licenses/RDP-THIRD-PARTY-NOTICES.txt',
)


class ReleaseIdentityTest(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix='cc-stable-policy-')
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        for name in FILES:
            dest = self.root / name
            dest.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(ROOT / name, dest)
        pubspec = self.root / 'client/flutter/pubspec.yaml'
        pubspec.write_text(re.sub(r'^version: .*$', 'version: 0.3.0+1362', pubspec.read_text(), flags=re.M))
        metadata = self.root / 'client/flutter/lib/app/app_info.dart'
        text = re.sub(r"const kAppVersion = '[^']+';", "const kAppVersion = '0.3.0';", metadata.read_text())
        metadata.write_text(re.sub(r'const kAppBuildNumber = [0-9]+;', 'const kAppBuildNumber = 1362;', text))

    def test_stable_source_above_all_025_artifacts_uses_exact_existing_namespaces(self):
        version = policy.verify(self.root)
        self.assertTrue(version.startswith('0.3.0+'))
        self.assertGreater(int(version.split('+')[1]), 1358)

    def test_dev_namespaces_or_install_ids_cannot_silently_open_an_empty_workspace(self):
        changes = (
            ('client/rust/platform-core/src/dirs.rs', '"CONSOLECRYPT_DATA_DIR"', '"CONSOLECRYPT_DEV_DATA_DIR"'),
            ('client/rust/platform-core/src/dirs.rs', 'APPLICATION: &str = "ConsoleCrypt"', 'APPLICATION: &str = "ConsoleCryptDev"'),
            ('client/rust/platform-core/src/os_store.rs', '"io.consolecrypt.ConsoleCrypt"', '"io.consolecrypt.ConsoleCrypt.Dev"'),
            ('client/rust/platform-core/src/linux_secret_service.rs', '"io.consolecrypt.ConsoleCrypt"', '"io.consolecrypt.ConsoleCrypt.Dev"'),
            ('client/flutter/android/app/build.gradle.kts', 'applicationId = "io.consolecrypt.consolecrypt"', 'applicationId = "io.consolecrypt.consolecrypt.dev"'),
            ('client/flutter/ios/Runner.xcodeproj/project.pbxproj', 'io.consolecrypt.consolecrypt;', 'io.consolecrypt.consolecrypt.dev;'),
            ('client/flutter/macos/Runner/Configs/AppInfo.xcconfig', 'PRODUCT_BUNDLE_IDENTIFIER = io.consolecrypt.consolecrypt', 'PRODUCT_BUNDLE_IDENTIFIER = io.consolecrypt.consolecrypt.dev'),
            ('client/packaging/windows/ConsoleCrypt.iss', '07A40F0D-CB43-48CB-B1E1-EE0F23785475', '470E18DC-BE3B-4646-871A-6687304D85D2'),
            ('client/flutter/linux/CMakeLists.txt', 'APPLICATION_ID "io.consolecrypt.consolecrypt"', 'APPLICATION_ID "io.consolecrypt.consolecrypt.dev"'),
        )
        for name, old, new in changes:
            with self.subTest(path=name, field=old):
                path = self.root / name
                original = path.read_text()
                self.assertIn(old, original)
                path.write_text(original.replace(old, new))
                with self.assertRaises(ValueError):
                    policy.verify(self.root)
                path.write_text(original)

    def test_stable_updates_retain_original_signed_anchor_and_native_install_entrypoints(self):
        path = self.root / 'client/flutter/lib/updates/update_service.dart'
        original = path.read_text()
        for removed in (policy.STABLE_UPDATE_KEY, 'https://updates.consolecrypt.evsikov.net/stable.json'):
            path.write_text(original.replace(removed, 'disabled'))
            with self.subTest(field=removed), self.assertRaises(ValueError):
                policy.verify(self.root)
        path.write_text(original)
        path = self.root / 'client/flutter/macos/Runner/UpdateBridge.swift'
        path.write_text(path.read_text() + '\n// updates_disabled\n')
        with self.assertRaises(ValueError):
            policy.verify(self.root)

    def test_notice_missing_stale_or_symlink_rejects_before_build(self):
        asset = self.root / 'client/flutter' / policy.RDP_NOTICE_ASSET
        original = asset.read_bytes()
        asset.write_bytes(original + b' stale copy')
        with self.assertRaisesRegex(ValueError, 'stale'):
            policy.verify(self.root)
        asset.unlink()
        with self.assertRaisesRegex(ValueError, 'regular'):
            policy.verify(self.root)
        try:
            asset.symlink_to(self.root / 'client/rust/rdp-core/THIRD_PARTY_NOTICES.txt')
        except OSError as error:
            if getattr(error, 'winerror', None) == 1314:
                return  # Remaining cases already ran; Windows symlink privilege varies.
            raise
        with self.assertRaisesRegex(ValueError, 'regular'):
            policy.verify(self.root)

    def test_apple_native_rdp_zlib_linkage_cannot_be_lost_by_identity_restore(self):
        for platform in ('ios', 'macos'):
            path = self.root / f'client/flutter/rust_builder/{platform}/cc_bridge.podspec'
            original = path.read_text()
            path.write_text(original.replace("s.libraries = 'z'", ''))
            with self.subTest(platform=platform), self.assertRaises(ValueError):
                policy.verify(self.root)
            path.write_text(original)


if __name__ == '__main__':
    unittest.main()
