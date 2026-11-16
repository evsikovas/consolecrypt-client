"""Owned live proof: actual CLI/dispatcher and actual bounded TLS worker.

Only CA trust is test-injected. No fake archive capture, RPC response, target,
job or credential resolver; role/plugin bytes are never imported/executed.
"""
import argparse
import base64
import hashlib
import json
from pathlib import Path
import sys
from unittest.mock import patch
import uuid

sys.path[:0]=['/opt/consolecrypt/src','/opt/consolecrypt/tests']
from consolecrypt_runner.__main__ import main
from consolecrypt_runner.dependency_prepare import capture_archive
from test_dependency_fetch import DependencyFetchTests

parser=argparse.ArgumentParser();parser.add_argument('--operation-id',required=True)
args=parser.parse_args();operation=str(uuid.UUID(args.operation_id))
assert operation==args.operation_id and uuid.UUID(operation).int
fixture=json.loads(Path('/fixture/dependency-preview.json').read_bytes())
raw=b''.join(base64.b64decode(chunk['response']['data'],validate=True) for chunk in fixture['chunks'])
assert hashlib.sha256(raw).hexdigest()==fixture['selection']['source']['sha256']
configured='/tmp/cc-live-'+operation+'/jobs'
Path(configured).parent.mkdir(mode=0o700)  # Exact owned new fixture only.
tls=DependencyFetchTests();tls.setUp();tls.body=raw
tls.headers=[('Content-Length',str(len(raw)))]
try:
    sys.stdout.write(json.dumps({'version':1,'operation_id':operation,'configured_root':configured,
        'url':tls.url,'sha256':hashlib.sha256(raw).hexdigest(),'archive_bytes':len(raw)},separators=(',',':'))+'\n')
    sys.stdout.flush()
    # Trust only this synthetic private CA, outside RPC, just like existing
    # trusted-worker tests. Real HTTPS, hostname and SHA checks still execute.
    def trusted_test_ca(parent,record,cancel):
        assert record['source']['url']==tls.url
        return capture_archive(parent,record,cancel,_ca_pem=tls.ca_pem)
    sys.argv=['consolecrypt-runner','--dependency-stdio','--root',configured,'--operation-id',operation]
    with patch('consolecrypt_runner.dependency_prepare.capture_archive',side_effect=trusted_test_ca):
        main()
    assert len(tls.requests)==1 and tls.requests[0][0]=='/artifact.tar.gz'
    headers={key.lower():value for key,value in tls.requests[0][1].items()}
    assert not {'authorization','proxy-authorization','cookie'}.intersection(headers)
    assert headers['accept-encoding']=='identity'
    assert not Path(configured).exists()  # The normal jobs root was untouched.
finally:
    assert tls.doCleanups(), 'Owned TLS fixture cleanup failed'
