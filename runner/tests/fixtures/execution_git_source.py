"""Owned TLS Git source, inside the disposable execution Runner only."""
import base64
import json
from pathlib import Path
import sys
import threading
import traceback

sys.path.insert(0, '/opt/consolecrypt/src')
sys.path.insert(0, '/opt/consolecrypt/tests')
from test_git_import import GitTlsFixture


def main():
    assert len(sys.argv) == 2
    playbook = base64.b64decode(sys.argv[1], validate=True).decode('utf-8')
    assert len(playbook) < 65536 and 'ConsoleCrypt scoped execution' in playbook
    fixture = GitTlsFixture()
    try:
        fixture.setUp()
        # Executing an unedited baseline must not produce the expected marker.
        (fixture.repo/'playbook.yml').write_text(
            playbook.replace('ConsoleCrypt scoped execution', 'ConsoleCrypt Git baseline'), encoding='utf-8')
        fixture.commit()
        commit = fixture.git('-C', str(fixture.repo), 'rev-parse', 'HEAD').decode().strip()
        config = {'url':fixture.url, 'ca_pem':fixture.ca, 'commit':commit}
        path = Path('/home/runner/git-flow-source.json')
        with path.open('x',encoding='utf-8') as file:
            json.dump(config,file)
        # Container cleanup bounds this process too. No private key is exported.
        threading.Event().wait(240)
    finally:
        fixture.doCleanups()


if __name__ == '__main__':
    try:
        main()
    except Exception:
        Path('/home/runner/git-flow-error.txt').write_text(traceback.format_exc(),encoding='utf-8')
        raise
