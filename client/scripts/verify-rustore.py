#!/usr/bin/env python3
"""Verify actual RuStore APK/AAB permissions, release signatures and native layout."""
import argparse
import base64
import hashlib
import importlib.util
import os
from pathlib import Path
import re
import subprocess
import xml.etree.ElementTree as ET
import zipfile

FORBIDDEN_PERMISSIONS = {
    'android.permission.REQUEST_INSTALL_PACKAGES',
    'android.permission.RECORD_AUDIO',
    'android.permission.READ_EXTERNAL_STORAGE',
    'android.permission.WRITE_EXTERNAL_STORAGE',
}


def verify_store_permissions(names):
    unexpected = FORBIDDEN_PERMISSIONS.intersection(names)
    assert not unexpected, f'Unneeded or forbidden store permissions: {sorted(unexpected)}'


def run(program, *args):
    result = subprocess.run([str(program), *map(str, args)], capture_output=True, text=True,
                            encoding='utf-8', errors='replace')
    if result.returncode:
        raise RuntimeError(f'{Path(program).name} verification failed: {result.stdout}\n{result.stderr}')
    return result.stdout + result.stderr


def fingerprint(path):
    pem = Path(path).read_text()
    der = base64.b64decode(''.join(line for line in pem.splitlines() if not line.startswith('---')))
    return hashlib.sha256(der).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--apk', type=Path, required=True)
    parser.add_argument('--aab', type=Path, required=True)
    parser.add_argument('--sdk', type=Path, required=True)
    parser.add_argument('--jdk', type=Path, required=True)
    parser.add_argument('--bundletool', type=Path, required=True)
    parser.add_argument('--app-cert', type=Path, required=True)
    parser.add_argument('--upload-cert', type=Path, required=True)
    parser.add_argument('--build', type=int, required=True)
    args = parser.parse_args()
    # Public identities registered for ConsoleCrypt's first production release.
    # Changing private CI secrets must never silently rotate a store identity.
    assert fingerprint(args.app_cert) == 'dc05b80a3cd71cfe1636f70340ac4aab25ba725158832c53a1ddd5cfdde36a6a', 'Unexpected permanent app signing key'
    assert fingerprint(args.upload_cert) == 'b22e7f6f5f701d937328677dc6ba835f1dcfb41513c28697bb7f6f5630d0d146', 'Unexpected permanent upload signing key'
    windows = os.name == 'nt'
    tools = args.sdk / 'build-tools/36.0.0'
    aapt = tools / ('aapt2.exe' if windows else 'aapt2')
    apk_signer = tools / ('apksigner.bat' if windows else 'apksigner')
    java = args.jdk / 'bin' / ('java.exe' if windows else 'java')
    jarsigner = args.jdk / 'bin' / ('jarsigner.exe' if windows else 'jarsigner')
    keytool = args.jdk / 'bin' / ('keytool.exe' if windows else 'keytool')
    signature = run(apk_signer, 'verify', '--verbose', '--print-certs', args.apk)
    actual = re.search(r'Signer #1 certificate SHA-256 digest:\s*([a-fA-F0-9]+)', signature)
    assert actual and actual[1].lower() == fingerprint(args.app_cert), 'APK signer mismatch'
    assert 'Android Debug' not in signature, 'Debug signing is forbidden for the store'
    permissions = run(aapt, 'dump', 'permissions', args.apk)
    verify_store_permissions(re.findall(r'android\.permission\.[A-Z_]+', permissions))
    badging = run(aapt, 'dump', 'badging', args.apk)
    assert "name='io.consolecrypt.consolecrypt'" in badging, 'Unexpected Android application ID'
    assert f"versionCode='{args.build}'" in badging, 'Incorrect APK versionCode'
    assert 'application-debuggable' not in badging, 'Debuggable APK'
    assert re.search(r"(?:minSdkVersion|sdkVersion):'30'", badging), 'Minimum Android version changed'
    run(java, '-jar', args.bundletool, 'validate', f'--bundle={args.aab}')
    manifest = run(java, '-jar', args.bundletool, 'dump', 'manifest', f'--bundle={args.aab}', '--module=base')
    xml = ET.fromstring(manifest[manifest.index('<manifest'):])
    android = '{http://schemas.android.com/apk/res/android}'
    assert xml.get('package') == 'io.consolecrypt.consolecrypt'
    assert xml.get(android + 'versionCode') == str(args.build)
    verify_store_permissions(node.get(android + 'name') for node in xml.findall('uses-permission'))
    app = xml.find('application')
    assert app is not None and app.get(android + 'debuggable', 'false') == 'false'
    assert all(node.get(android + 'authorities') != 'io.consolecrypt.consolecrypt.updates' for node in app.findall('provider'))
    jar_result = run(jarsigner, '-J-Duser.language=en', '-verify', '-verbose', '-certs', args.aab)
    assert 'jar verified.' in jar_result.lower() and 'unsigned entries' not in jar_result.lower(), 'AAB signature is incomplete'
    jar_cert = run(keytool, '-J-Duser.language=en', '-printcert', '-jarfile', args.aab)
    actual_upload = re.search(r'SHA256:\s*([A-Fa-f0-9:]+)', jar_cert)
    assert actual_upload and actual_upload[1].replace(':', '').lower() == fingerprint(args.upload_cert), 'AAB upload certificate mismatch'
    spec = importlib.util.spec_from_file_location('android_layout', Path(__file__).with_name('verify-android-apk.py'))
    layout = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(layout)
    notice = Path(__file__).resolve().parents[1] / 'rust/rdp-core/THIRD_PARTY_NOTICES.txt'
    layout.verify_apk(args.apk, notice)
    with zipfile.ZipFile(args.aab) as bundle, zipfile.ZipFile(args.apk) as apk:
        assert bundle.testzip() is None
        names = bundle.namelist()
        # These libraries are loaded by the current Flutter/plugin dependency
        # set. A valid signature alone does not prove a complete runnable app.
        required = {'libapp.so', 'libflutter.so', 'libcc_bridge.so',
                    'libsuper_native_extensions.so', 'libirondash_engine_context_native.so',
                    'libimage_processing_util_jni.so', 'libsurface_util_jni.so'}
        packaged = {Path(n).name for n in apk.namelist() if n.startswith('lib/') and n.endswith('.so')}
        assert required <= packaged, f'Missing required Android native libraries: {required - packaged}'
        for library in [n for n in names if n.startswith('base/lib/') and n.endswith('.so')]:
            assert library.startswith('base/lib/arm64-v8a/'), 'Unexpected AAB ABI'
            layout.verify_elf(bundle.read(library), library)
            assert bundle.read(library) == apk.read(library.removeprefix('base/')), 'APK/AAB native payload differs'
        for name in required:
            assert f'base/lib/arm64-v8a/{name}' in names
        assert bundle.read('base/assets/flutter_assets/assets/licenses/RDP-THIRD-PARTY-NOTICES.txt') == notice.read_bytes()
        assert b'GNU AFFERO GENERAL PUBLIC LICENSE' in bundle.read('base/assets/flutter_assets/assets/licenses/AGPL-3.0-only.txt')
        assert b'updates-rustore' in bundle.read('base/lib/arm64-v8a/libapp.so'), 'Store UI missing from compiled Dart'
    run(tools / ('zipalign.exe' if windows else 'zipalign'), '-c', '-P', '16', '4', args.apk)
    for path in (args.apk, args.aab):
        print(f'PASS {path.name}: SHA-256 {hashlib.sha256(path.read_bytes()).hexdigest()}')
    print('PASS: release signatures, store-only permissions, package/version, complete ARM64 payload and 16 KiB layout')


if __name__ == '__main__':
    main()
