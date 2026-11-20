"""Trusted test bootstrap only. Normal Helper constructor/CLI stay disabled."""
import base64
import hashlib
import json
from pathlib import Path
import sys
import tempfile
import threading
import uuid
from unittest.mock import patch

sys.path.insert(0,'/opt/consolecrypt/src')
from consolecrypt_runner.__main__ import Helper,serve

fixture=json.loads(Path('/fixture/dependency-transfer.json').read_bytes())
raw=b''.join(base64.b64decode(pair['response']['data'],validate=True) for pair in fixture['chunks'])
assert hashlib.sha256(raw).hexdigest()==fixture['selection']['source']['sha256']
disabled=sys.argv[1:]==['--disabled']
assert not sys.argv[1:] or disabled
with tempfile.TemporaryDirectory(prefix='cc-core-transfer-') as temporary:
    root=Path(temporary);root.chmod(0o700)
    helper=Helper(root/'helper',_dependency_preparation=not disabled)
    try:
        if disabled:
            result={'preparation_id':str(uuid.uuid4()),'review_digest':fixture['info']['review_digest']}
        else:
            source=root/'source';source.mkdir(mode=0o700);(source/'play.yml').write_bytes(b'---\n')
            base=helper.revisions.capture(source)
            request={'revision_id':base['revision_id'],'revision_digest':base['digest'],'lock_sha256':None,
                     'selection':fixture['selection'],'strip_components':0,'network_origin':'https://source.example.test'}
            # Fixture-only fixed byte source. RPC cannot supply this override;
            # real TLS worker has its separate non-root proof. No project runs.
            with patch('consolecrypt_runner.dependency_prepare.capture_archive',return_value=raw):
                prepared=helper.request('dependency_prepare',request,threading.Event())
            assert prepared['review_digest']==fixture['info']['review_digest']
            result={key:prepared[key] for key in ('preparation_id','review_digest')}
        sys.stdout.write(json.dumps(result,separators=(',',':'))+'\n');sys.stdout.flush()
        serve(helper,sys.stdin.buffer,sys.stdout.buffer)
    finally:helper.close()
