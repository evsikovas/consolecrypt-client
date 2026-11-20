from datetime import datetime, timedelta, timezone
import hashlib
from http.server import BaseHTTPRequestHandler, HTTPServer
import os
from pathlib import Path
import ssl
import tempfile
import tarfile
import threading
import unittest
from unittest.mock import patch

from cryptography import x509
from cryptography.hazmat.primitives import hashes, serialization
from cryptography.hazmat.primitives.asymmetric import rsa
from cryptography.x509.oid import NameOID
import ipaddress

from consolecrypt_runner.dependency_fetch import fetch_pinned_archive
from consolecrypt_runner.dependency_archive import staged_dependency
from consolecrypt_runner.errors import RunnerError
from consolecrypt_runner.snapshot import create_snapshot
from test_dependency_archive import archive


@unittest.skipUnless(os.name == 'posix', 'Offline Unix TLS fixture')
class DependencyFetchTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix='cc-dependency-tls-')
        self.addCleanup(temporary.cleanup)
        root = Path(temporary.name)
        key = rsa.generate_private_key(public_exponent=65537, key_size=2048)
        subject = x509.Name([x509.NameAttribute(NameOID.COMMON_NAME, 'Synthetic ConsoleCrypt TLS fixture')])
        now = datetime.now(timezone.utc)
        cert = (x509.CertificateBuilder().subject_name(subject).issuer_name(subject).public_key(key.public_key())
            .serial_number(x509.random_serial_number()).not_valid_before(now-timedelta(minutes=1))
            .not_valid_after(now+timedelta(hours=1))
            .add_extension(x509.SubjectAlternativeName([x509.IPAddress(ipaddress.ip_address('127.0.0.1'))]),critical=False)
            .add_extension(x509.BasicConstraints(ca=True,path_length=None),critical=True)
            .sign(key,hashes.SHA256()))
        key_path = root/'key.pem'
        key_path.write_bytes(key.private_bytes(serialization.Encoding.PEM, serialization.PrivateFormat.PKCS8,
                                               serialization.NoEncryption()))
        cert_path = root/'cert.pem'
        cert_path.write_bytes(cert.public_bytes(serialization.Encoding.PEM))
        self.ca_pem = cert_path.read_text(encoding='ascii')
        self.context = ssl.create_default_context(cafile=str(cert_path))
        self.body = b'synthetic-pinned-archive-bytes'
        self.status, self.headers, self.requests, self.request_hook = 200, [], [], None
        owner = self

        class Handler(BaseHTTPRequestHandler):
            def do_GET(self):
                owner.requests.append((self.path, dict(self.headers)))
                if owner.request_hook is not None:
                    owner.request_hook()
                self.send_response(owner.status)
                for key, value in owner.headers:
                    self.send_header(key, value)
                self.end_headers()
                try:
                    self.wfile.write(owner.body)
                except (BrokenPipeError, ssl.SSLError):
                    pass

            def log_message(self, *args):
                pass  # Never log source paths, payloads or private test keys.

        server = HTTPServer(('127.0.0.1', 0), Handler)
        tls = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
        tls.load_cert_chain(str(cert_path),str(key_path))
        server.socket = tls.wrap_socket(server.socket, server_side=True)
        self.addCleanup(server.server_close)
        thread = threading.Thread(target=server.serve_forever, kwargs={'poll_interval': .01})
        thread.start()
        self.addCleanup(thread.join, 5)
        self.addCleanup(server.shutdown)
        self.url = 'https://127.0.0.1:'+str(server.server_address[1])+'/artifact.tar.gz'

    def record(self):
        return {'kind':'role','name':'demo','version':'1.2.3','tree_digest':'b'*64,
                'source':{'url':self.url,'sha256':hashlib.sha256(self.body).hexdigest()}}

    def denied(self, record=None, **kwargs):
        with self.assertRaises(RunnerError):
            fetch_pinned_archive(record or self.record(), _context=self.context, **kwargs)

    def test_verified_tls_exact_url_pin_and_no_environment_proxy_auth_or_cookie(self):
        self.headers = [('Content-Length', str(len(self.body)))]
        with patch.dict(os.environ, {'HTTPS_PROXY':'http://127.0.0.1:1',
                                     'https_proxy':'http://127.0.0.1:1'}):
            self.assertEqual(fetch_pinned_archive(self.record(),_context=self.context),self.body)
        self.assertEqual(len(self.requests),1)
        path, headers = self.requests[0]
        self.assertEqual(path,'/artifact.tar.gz')
        self.assertNotIn('Authorization',headers)
        self.assertNotIn('Proxy-Authorization',headers)
        self.assertNotIn('Cookie',headers)
        self.assertEqual(headers['Accept-Encoding'],'identity')

    def test_default_trust_rejects_self_signed_server_and_hostname_check_cannot_be_disabled(self):
        with self.assertRaises(RunnerError):
            fetch_pinned_archive(self.record())
        self.assertEqual(self.requests,[])
        insecure = ssl._create_unverified_context()
        with self.assertRaises(RunnerError):
            fetch_pinned_archive(self.record(),_context=insecure)
        wrong_host = self.record()
        wrong_host['source']['url']=self.url.replace('127.0.0.1','localhost')
        self.denied(wrong_host)
        self.assertEqual(self.requests,[])

    def test_redirect_never_contacts_its_destination_or_sends_another_request(self):
        self.status=302
        self.headers=[('Location','https://127.0.0.1:1/never-follow?token=synthetic')]
        self.denied()
        self.assertEqual(len(self.requests),1)

    def test_non_200_http_compression_ambiguous_and_oversized_framing_are_denied(self):
        cases=[(404,[]),(200,[('Content-Encoding','gzip')]),
            (200,[('Content-Length','3'),('Content-Length','3')]),
            (200,[('Content-Length','3'),('Transfer-Encoding','chunked')]),
            (200,[('Transfer-Encoding','gzip')]),(200,[('Content-Length','-1')]),
            (200,[('Content-Length','999999999')]),(200,[('Content-Length','0')]),
            (200,[('Content-Length',str(len(self.body)+3))])]
        for status, headers in cases:
            with self.subTest(status=status,headers=headers):
                self.status,self.headers=status,headers
                self.denied()

    def test_stream_budget_and_hash_are_checked_even_without_content_length(self):
        self.headers=[]
        with patch('consolecrypt_runner.dependency_fetch.MAX_ARCHIVE_BYTES',4):
            self.denied()
        wrong=self.record()
        wrong['source']['sha256']='0'*64
        self.denied(wrong)
        self.body=b''
        self.denied()

    def test_caller_cannot_replace_sha_pin_during_download(self):
        record = self.record()

        def mutate():
            self.body = b'changed-after-source-approval'
            record['source']['sha256'] = hashlib.sha256(self.body).hexdigest()

        self.request_hook = mutate
        self.denied(record)

    def test_real_tls_download_stages_the_same_pinned_tree_without_executing_role_content(self):
        with tempfile.TemporaryDirectory(prefix='cc-fetch-stage-') as temporary:
            root = Path(temporary)
            project = root/'source'
            (project/'tasks').mkdir(parents=True)
            # The loader must preserve these bytes as data, never invoke them.
            payload = b'---\n- ansible.builtin.command: touch /must-never-execute\n'
            (project/'tasks/main.yml').write_bytes(payload)
            pinned = create_snapshot(project, root/'pinned')
            self.body = archive([('tasks/main.yml',tarfile.REGTYPE,payload)])
            self.headers = [('Content-Length',str(len(self.body)))]
            record = self.record()
            record['tree_digest'] = pinned.digest
            staging = root/'staging'
            staging.mkdir(mode=0o700)
            downloaded = fetch_pinned_archive(record,_context=self.context)
            with staged_dependency(downloaded,record,staging) as (tree,snapshot,receipt):
                self.assertEqual(snapshot.digest,pinned.digest)
                self.assertEqual((tree/'tasks/main.yml').read_bytes(),payload)
                self.assertEqual(receipt['source'],record['source'])
                self.assertFalse(Path('/must-never-execute').exists())
            self.assertEqual(list(staging.iterdir()),[])

    def test_source_and_timeout_validation_precede_any_network_request(self):
        for url in ('http://127.0.0.1/role','https://user:pass@127.0.0.1/role',
                    self.url+'?token=synthetic',self.url+'#fragment'):
            value=self.record();value['source']['url']=url
            self.denied(value)
        for timeout in (True,0,61,1.5):
            self.denied(timeout=timeout)
        self.assertEqual(self.requests,[])
