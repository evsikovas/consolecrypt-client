#!/usr/bin/env python3
"""Materialize CI signing secrets in a private temporary directory, never log them."""
import base64
import os
from pathlib import Path
import subprocess
import tempfile


def property_value(value):
    encoded = []
    for char in value:
        code = ord(char)
        if code > 0xffff:
            code -= 0x10000
            encoded.append(f'\\u{0xd800 + (code >> 10):04x}\\u{0xdc00 + (code & 1023):04x}')
        elif code > 126 or code < 32:
            encoded.append(f'\\u{code:04x}')
        else:
            encoded.append('\\' + char if char in ('\\', ' ') else char)
    return ''.join(encoded)


def main():
    for role in ('APP', 'UPLOAD'):
        for suffix in ('KEYSTORE_BASE64', 'KEYSTORE_PASSWORD'):
            if not os.environ.get(f'ANDROID_{role}_{suffix}'):
                raise SystemExit(f'Missing Android signing secret: ANDROID_{role}_{suffix}')
    directory = Path(tempfile.mkdtemp(prefix='cc-android-signing-', dir=os.environ['RUNNER_TEMP']))
    directory.chmod(0o700)
    with Path(os.environ['GITHUB_ENV']).open('a', encoding='utf-8') as exports:
        exports.write(f'CC_ANDROID_PRIVATE_DIRECTORY={directory}\n')
        for role in ('APP', 'UPLOAD'):
            alias = os.environ.get(f'ANDROID_{role}_KEY_ALIAS') or f'consolecrypt-{role.lower()}'
            password = os.environ[f'ANDROID_{role}_KEYSTORE_PASSWORD']
            key_password = os.environ.get(f'ANDROID_{role}_KEY_PASSWORD') or password
            keystore = directory / f'{role.lower()}.jks'
            keystore.write_bytes(base64.b64decode(os.environ[f'ANDROID_{role}_KEYSTORE_BASE64'], validate=True))
            keystore.chmod(0o600)
            properties = directory / f'{role.lower()}.properties'
            values = {'storeFile': keystore.as_posix(), 'storePassword': password,
                      'keyAlias': alias, 'keyPassword': key_password}
            properties.write_text(''.join(f'{k}={property_value(v)}\n' for k, v in values.items()), encoding='ascii')
            properties.chmod(0o600)
            certificate = directory / f'{role.lower()}_certificate.pem'
            keytool = Path(os.environ['JAVA_HOME']) / 'bin' / ('keytool.exe' if os.name == 'nt' else 'keytool')
            result = subprocess.run([str(keytool), '-exportcert', '-rfc', '-alias', alias, '-keystore', str(keystore),
                '-file', str(certificate), '-storepass:env', f'ANDROID_{role}_KEYSTORE_PASSWORD'], capture_output=True)
            if result.returncode:
                raise SystemExit(f'Cannot export {role.lower()} certificate; check the supplied signing secrets')
            exports.write(f'CC_ANDROID_{role}_SIGNING_PROPERTIES={properties}\n')
            exports.write(f'CC_ANDROID_{role}_CERT={certificate}\n')
    print('Private Android signing files prepared; no secret values printed')


if __name__ == '__main__':
    main()
