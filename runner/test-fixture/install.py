from pathlib import Path
from importlib import metadata
import hashlib
import json
import re
import subprocess
import sys

root = Path('/opt/consolecrypt')
manifest = json.loads((root / 'environment.json').read_text())
subprocess.run([sys.executable, '-m', 'pip', 'install', '--only-binary=:all:', '--disable-pip-version-check',
                '--report', str(root / 'install-report.json'), '--require-hashes',
                '-r', str(root / 'requirements-linux-x86_64-py312.lock')], check=True)
report = json.loads((root / 'install-report.json').read_text())
lines = sorted([item['metadata']['name'] + '==' + item['metadata']['version'] +
    ' --hash=sha256:' + item['download_info']['archive_info']['hashes']['sha256'] for item in report['install']], key=str.lower)
lock = ('\n'.join(lines) + '\n').encode()
(root / 'requirements-linux-x86_64-py312.lock').write_bytes(lock)
environment = {'schema_version': 1, 'python': '3.12', 'system': 'Linux', 'machine': 'x86_64',
    'packages': {re.sub(r'[-_.]+', '-', item.metadata['Name']).lower(): item.version for item in metadata.distributions()},
    'lock_sha256': hashlib.sha256(lock).hexdigest()}
(root / 'linux-x86_64-py312.json').write_text(json.dumps(environment, indent=2) + '\n')
subprocess.run(['dpkg-query', '-W'], stdout=(root / 'os-packages.txt').open('w'), check=True)
