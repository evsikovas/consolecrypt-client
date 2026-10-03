#!/usr/bin/env python3
"""Read-only stable identity, updater trust and bundled RDP notice preflight."""
import argparse
from pathlib import Path
import re

STABLE_BUNDLE_ID = 'io.consolecrypt.consolecrypt'
STABLE_STORE_SERVICE = 'io.consolecrypt.ConsoleCrypt'
STABLE_UPDATE_KEY = 'aK3R5vXwJ9eeqQ1iYah9fzBAIHOeKNejoRdI8weXUYo='
RDP_NOTICE_ASSET = 'assets/licenses/RDP-THIRD-PARTY-NOTICES.txt'


def require(root, path, expected, forbidden=()):
    text = (root / path).read_text()
    if any(value not in text for value in expected) or any(value in text for value in forbidden):
        raise ValueError('Stable identity policy failed: ' + path)


def verify_notices(root):
    source = root / 'client/rust/rdp-core/THIRD_PARTY_NOTICES.txt'
    asset = root / 'client/flutter' / RDP_NOTICE_ASSET
    if source.is_symlink() or asset.is_symlink() or not source.is_file() or not asset.is_file():
        raise ValueError('RDP licence notice must be a regular source and Flutter asset')
    contents = source.read_bytes()
    if not contents or b'Permission is hereby granted' not in contents or b'Apache License' not in contents:
        raise ValueError('RDP licence notice is incomplete')
    if asset.read_bytes() != contents:
        raise ValueError('RDP licence asset is stale; synchronize it with the source notice')


def verify(root):
    pubspec = (root / 'client/flutter/pubspec.yaml').read_text()
    matches = re.findall(r'^version: ((?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*))\+([1-9][0-9]*)$', pubspec, re.M)
    if len(matches) != 1:
        raise ValueError('A production checkout requires one stable numeric version')
    version, build = matches[0]
    require(root, 'client/flutter/lib/app/app_info.dart',
            ["const kAppName = 'ConsoleCrypt';", f"const kAppVersion = '{version}';", f'const kAppBuildNumber = {build};'],
            ['ConsoleCrypt Dev', 'kProductionUpdatesEnabled = false'])
    require(root, 'client/rust/platform-core/src/dirs.rs',
            ['DATA_DIR_ENV: &str = "CONSOLECRYPT_DATA_DIR"', 'APPLICATION: &str = "ConsoleCrypt"'], ['CONSOLECRYPT_DEV_DATA_DIR'])
    for path in ['client/rust/platform-core/src/os_store.rs', 'client/rust/platform-core/src/linux_secret_service.rs']:
        require(root, path, [f'DEFAULT_SERVICE: &str = "{STABLE_STORE_SERVICE}"'],
                [f'DEFAULT_SERVICE: &str = "{STABLE_STORE_SERVICE}.Dev"'])
    require(root, 'client/flutter/macos/Runner/Configs/AppInfo.xcconfig',
            ['PRODUCT_NAME = ConsoleCrypt', 'PRODUCT_BUNDLE_IDENTIFIER = ' + STABLE_BUNDLE_ID], [STABLE_BUNDLE_ID + '.dev'])
    require(root, 'client/flutter/ios/Runner.xcodeproj/project.pbxproj',
            ['PRODUCT_BUNDLE_IDENTIFIER = ' + STABLE_BUNDLE_ID + ';'], [STABLE_BUNDLE_ID + '.dev'])
    require(root, 'client/flutter/rust_builder/ios/Classes/CcBridgePlugin.swift',
            ['appendingPathComponent("ConsoleCrypt", isDirectory: true)'], ['appendingPathComponent("ConsoleCryptDev"'])
    require(root, 'client/flutter/android/app/build.gradle.kts', ['applicationId = "' + STABLE_BUNDLE_ID + '"'], [STABLE_BUNDLE_ID + '.dev'])
    require(root, 'client/flutter/android/app/src/main/kotlin/io/consolecrypt/consolecrypt/MainActivity.kt',
            ['File(noBackupFilesDir, "consolecrypt")', '"install" ->'], ['updates_disabled', '"consolecrypt-dev"'])
    require(root, 'client/flutter/android/app/src/main/AndroidManifest.xml',
            ['android:label="ConsoleCrypt"', 'android.permission.REQUEST_INSTALL_PACKAGES', '${applicationId}.updates'], ['ConsoleCrypt Dev'])
    require(root, 'client/flutter/windows/CMakeLists.txt', ['set(BINARY_NAME "ConsoleCrypt")'], ['ConsoleCryptDev'])
    require(root, 'client/packaging/windows/ConsoleCrypt.iss',
            ['AppId={{07A40F0D-CB43-48CB-B1E1-EE0F23785475}', 'DefaultDirName={localappdata}\\Programs\\ConsoleCrypt'], ['ConsoleCryptDev'])
    require(root, 'client/flutter/linux/CMakeLists.txt',
            ['set(BINARY_NAME "consolecrypt")', 'set(APPLICATION_ID "' + STABLE_BUNDLE_ID + '")'], [STABLE_BUNDLE_ID + '.dev'])
    require(root, 'client/packaging/linux/consolecrypt.desktop', ['Name=ConsoleCrypt', 'Exec=consolecrypt'], ['consolecrypt-dev'])
    require(root, 'client/flutter/lib/updates/update_service.dart',
            ['https://updates.consolecrypt.evsikov.net/stable.json', STABLE_UPDATE_KEY, 'verifyUpdateFeed(', 'verifyInstaller('], ['updates_disabled'])
    require(root, 'client/flutter/macos/Runner/UpdateBridge.swift', ['saveAndOpen', 'VerifiedUpdateExport.write'], ['updates_disabled'])
    for platform in ['ios', 'macos']:
        require(root, f'client/flutter/rust_builder/{platform}/cc_bridge.podspec', ["s.libraries = 'z'"])
    verify_notices(root)
    return f'{version}+{build}'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', type=Path, default=Path(__file__).resolve().parents[2])
    args = parser.parse_args()
    verify(args.root.resolve())
    print('Stable identity, storage namespace, signed updater trust and RDP notice verified')


if __name__ == '__main__':
    main()
