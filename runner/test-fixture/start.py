"""Disposable SSH fixture; host/client user credentials never enter this image."""
import os
from pathlib import Path
import subprocess

authorized_key = os.environ.get('CC_FIXTURE_PUBLIC_KEY', '')
if not authorized_key.startswith('ssh-ed25519 ') or '\n' in authorized_key or len(authorized_key) > 2048:
    raise SystemExit('An isolated fixture public key is required')
root = Path('/run/sshd')
root.mkdir(exist_ok=True)
subprocess.run(['ssh-keygen', '-q', '-t', 'ed25519', '-N', '', '-f', str(root / 'hostkey')], check=True)
(root / 'authorized_keys').write_text(authorized_key + '\n')
(root / 'authorized_keys').chmod(0o644)
(root / 'sshd_config').write_text('''Port 2222
HostKey /run/sshd/hostkey
PidFile /run/sshd/pid
AuthorizedKeysFile /run/sshd/authorized_keys
PasswordAuthentication no
KbdInteractiveAuthentication no
PermitRootLogin no
PermitEmptyPasswords no
UsePAM no
AllowUsers runner
StrictModes yes
Subsystem sftp internal-sftp
LogLevel ERROR
''')
os.execv('/usr/sbin/sshd', ['/usr/sbin/sshd', '-D', '-e', '-f', str(root / 'sshd_config')])
