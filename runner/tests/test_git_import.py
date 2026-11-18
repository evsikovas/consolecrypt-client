"""Real Git over owned localhost TLS + real namespace, never public Git services."""
import base64
from concurrent.futures import ThreadPoolExecutor
from datetime import datetime,timedelta,timezone
import hashlib
import json
from http.server import BaseHTTPRequestHandler,ThreadingHTTPServer
import ipaddress
import os
from pathlib import Path
import shutil
import ssl
import subprocess
import sys
import tempfile
import threading
import unittest
from unittest.mock import patch

from cryptography import x509
from cryptography.hazmat.primitives import hashes,serialization
from cryptography.hazmat.primitives.asymmetric import rsa
from cryptography.x509.oid import NameOID

from consolecrypt_runner.errors import RunnerError
from consolecrypt_runner.git_import import capture_git,validate_source,GitStreamReceiver
from consolecrypt_runner.revisions import RevisionRepository


class GitInputTests(unittest.TestCase):
    def test_import_source_never_embeds_credentials_or_executes_a_transport(self):
        for url in ('file:///tmp/repo','ssh://git@example.test/repo','ext::bad','http://user:secret@example.test/repo',
                    'https://user:secret@example.test/repo','https://example.test/repo?token=secret',
                    'https://example.test/repo#secret','https://example.test:0/repo','https://example.test\\repo'):
            with self.subTest(url=url),self.assertRaises(RunnerError):validate_source(url,'main')
        for reference in ('-c','../main','main~1','main:other','refs//heads','branch.lock'):
            with self.subTest(reference=reference),self.assertRaises(RunnerError):validate_source('https://example.test/repo',reference)
        validate_source('https://example.test/repo.git','release/1.0')
        validate_source('http://example.test/repo.git','release/1.0')

    def test_corrupt_or_truncated_stream_never_becomes_a_completed_snapshot(self):
        with tempfile.TemporaryDirectory() as temporary:
            receiver=GitStreamReceiver(Path(temporary),'https://example.test/repo','main')
            receiver.message({'type':'source','url':receiver.url,'reference':'main','commit':'a'*40,'ca_sha256':None})
            receiver.message({'type':'file','path':'file','mode':0o600,'bytes':3})
            receiver.message({'type':'chunk','offset':0,'data':base64.b64encode(b'abc').decode()})
            try:
                with self.assertRaises(RunnerError):receiver.message({'type':'file_end','sha256':'0'*64})
                self.assertFalse(receiver.complete)
            finally:receiver.close()


@unittest.skipUnless(os.name=='posix' and os.getuid()!=0 and shutil.which('git') and shutil.which('bwrap'),
                     'Prepared non-root Linux Git namespace required')
class GitTlsFixture(unittest.TestCase):
    def setUp(self):
        temporary=tempfile.TemporaryDirectory(prefix='cc-git-tls-');self.addCleanup(temporary.cleanup)
        self.root=Path(temporary.name)
        self.home=self.root/'git-home';self.home.mkdir(mode=0o700)
        self.env={'PATH':'/usr/bin:/bin','HOME':str(self.home),'LANG':'C.UTF-8','LC_ALL':'C.UTF-8',
                  'GIT_CONFIG_GLOBAL':'/dev/null','GIT_CONFIG_NOSYSTEM':'1','GIT_TERMINAL_PROMPT':'0'}
        self.repo=self.root/'repo.git'
        self.git('init','-b','main',str(self.repo))
        (self.repo/'playbook.yml').write_text('---\n- name: Git fixture\n  hosts: all\n  gather_facts: false\n  tasks: []\n')
        (self.repo/'binary.dat').write_bytes(bytes(range(256))*1024)
        (self.repo/'script.sh').write_text('#!/bin/sh\nexit 0\n');(self.repo/'script.sh').chmod(0o700)
        (self.repo/'.gitattributes').write_text('*.dat filter=do_not_run\n')
        tasks=self.repo/'roles'/'fixture'/'tasks';tasks.mkdir(parents=True)
        (tasks/'main.yml').write_bytes(b'---\r\n- name: Nested fixture\r\n  ansible.builtin.debug:\r\n    msg: fixture\r\n')
        (self.repo/'файл с пробелом.yml').write_bytes(b'---\r\n# UTF-8 path, unchanged CRLF\r\n')
        self.commit()
        self.expected=self.git('-C',str(self.repo),'rev-parse','HEAD').strip().decode()
        key=rsa.generate_private_key(public_exponent=65537,key_size=2048)
        subject=x509.Name([x509.NameAttribute(NameOID.COMMON_NAME,'Disposable ConsoleCrypt Git fixture')])
        now=datetime.now(timezone.utc)
        certificate=(x509.CertificateBuilder().subject_name(subject).issuer_name(subject).public_key(key.public_key())
            .serial_number(x509.random_serial_number()).not_valid_before(now-timedelta(minutes=5)).not_valid_after(now+timedelta(days=1))
            .add_extension(x509.SubjectAlternativeName([x509.IPAddress(ipaddress.ip_address('127.0.0.1'))]),False)
            .add_extension(x509.BasicConstraints(ca=True,path_length=None),True).sign(key,hashes.SHA256()))
        self.ca=certificate.public_bytes(serialization.Encoding.PEM).decode()
        cert_file=self.root/'server.pem';cert_file.write_text(self.ca)
        key_file=self.root/'server.key';key_file.write_bytes(key.private_bytes(serialization.Encoding.PEM,
            serialization.PrivateFormat.PKCS8,serialization.NoEncryption()));key_file.chmod(0o600)
        owner=self
        self.received=threading.Event();self.pause=None
        class Handler(BaseHTTPRequestHandler):
            def handle(self):
                try:super().handle()
                except (ConnectionResetError,BrokenPipeError,ssl.SSLError):pass
            def do_GET(self):self.handle_git()
            def do_POST(self):self.handle_git()
            def log_message(self,*args):pass
            def handle_git(self):
                owner.received.set()
                if owner.pause is not None:owner.pause.wait(10)
                path,_,query=self.path.partition('?')
                if not path.startswith('/repo.git/') or '..' in path:
                    self.send_error(404);return
                length=int(self.headers.get('Content-Length','0'))
                if length>1024*1024:self.send_error(413);return
                env=dict(owner.env,GIT_PROJECT_ROOT=str(owner.root),GIT_HTTP_EXPORT_ALL='1',
                    REQUEST_METHOD=self.command,PATH_INFO=path,QUERY_STRING=query,
                    CONTENT_TYPE=self.headers.get('Content-Type',''),CONTENT_LENGTH=str(length),
                    SERVER_PROTOCOL='HTTP/1.1',REMOTE_ADDR='127.0.0.1')
                result=subprocess.run(['/usr/bin/git','http-backend'],input=self.rfile.read(length),env=env,
                                      stdout=subprocess.PIPE,stderr=subprocess.DEVNULL,timeout=10,check=True)
                headers,_,body=result.stdout.partition(b'\r\n\r\n')
                fields=[line.decode().split(':',1) for line in headers.split(b'\r\n') if b':' in line]
                status=next((int(value.strip().split()[0]) for name,value in fields if name.lower()=='status'),200)
                self.send_response(status)
                for name,value in fields:
                    if name.lower() not in ('status','content-length','connection'):self.send_header(name,value.strip())
                self.send_header('Content-Length',str(len(body)));self.end_headers()
                try:self.wfile.write(body)
                except (BrokenPipeError,ssl.SSLError):pass
        server=ThreadingHTTPServer(('127.0.0.1',0),Handler);server.daemon_threads=True
        tls=ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER);tls.load_cert_chain(cert_file,key_file)
        server.socket=tls.wrap_socket(server.socket,server_side=True)
        thread=threading.Thread(target=server.serve_forever,daemon=True);thread.start()
        self.addCleanup(lambda:(server.shutdown(),server.server_close(),thread.join(timeout=3)))
        self.url=f'https://127.0.0.1:{server.server_port}/repo.git'
        self.handler=Handler
        self.store=RevisionRepository(self.root/'revisions')
    def git(self,*args):
        return subprocess.run(['/usr/bin/git','-c','core.hooksPath=/nonexistent','-c','commit.gpgsign=false',*args],
            env=self.env,stdin=subprocess.DEVNULL,stdout=subprocess.PIPE,stderr=subprocess.PIPE,timeout=10,check=True).stdout
    def commit(self):
        self.git('-C',str(self.repo),'add','--all')
        self.git('-C',str(self.repo),'-c','user.name=Fixture','-c','user.email=fixture@example.invalid','commit','-m','Owned fixture')
    def test_real_anonymous_http_import_records_a_plain_origin(self):
        server=ThreadingHTTPServer(('127.0.0.1',0),self.handler)
        thread=threading.Thread(target=server.serve_forever,daemon=True)
        thread.start()
        self.addCleanup(lambda:(server.shutdown(),server.server_close(),thread.join(timeout=3)))
        url=f'http://127.0.0.1:{server.server_port}/repo.git'
        captured=capture_git(self.store,url,'main',threading.Event())
        root,_=self.store.read(captured['revision_id'])
        self.assertEqual(captured['source']['url'],url)
        self.assertEqual(captured['source']['commit'],self.expected)
        self.assertIsNone(captured['source']['ca_sha256'])
        self.assertTrue((root/'playbook.yml').is_file())
        self.assertFalse((root/'.git').exists())
    def test_real_https_import_preserves_binary_modes_and_ignores_user_configuration(self):
        marker=self.root/'must-not-execute'
        config=self.root/'user.gitconfig'
        config.write_text(f'[filter "do_not_run"]\n\tclean = touch {marker}\n')
        with patch.dict(os.environ,{'GIT_CONFIG_GLOBAL':str(config),'GIT_CONFIG_COUNT':'1',
                'GIT_CONFIG_KEY_0':'http.sslVerify','GIT_CONFIG_VALUE_0':'false'}):
            captured=capture_git(self.store,self.url,'main',threading.Event(),ca_pem=self.ca)
        root,snapshot=self.store.read(captured['revision_id'])
        self.assertEqual(captured['source']['commit'],self.expected)
        self.assertEqual(captured['source']['ca_sha256'],hashlib.sha256(self.ca.encode()).hexdigest())
        self.assertEqual((root/'binary.dat').read_bytes(),bytes(range(256))*1024)
        self.assertEqual(next(entry['mode'] for entry in snapshot.files if entry['path']=='script.sh'),0o700)
        self.assertFalse(marker.exists());self.assertFalse((root/'.git').exists())
        self.assertEqual(list(self.root.glob('git-import-*')),[])
        from consolecrypt_runner.__main__ import Helper
        helper=Helper(self.root)
        try:
            content=bytearray()
            while len(content)<256*1024:
                chunk=helper.request('read_chunk',{'revision_id':captured['revision_id'],'path':'binary.dat',
                    'offset':len(content),'limit':128*1024},threading.Event())
                self.assertEqual(chunk['revision_digest'],captured['digest'])
                content.extend(base64.b64decode(chunk['data'],validate=True))
            self.assertEqual(bytes(content),bytes(range(256))*1024)
            with self.assertRaises(RunnerError):
                helper.request('read_chunk',{'revision_id':captured['revision_id'],'path':'../server.key','offset':0,'limit':1},threading.Event())
        finally:helper.close()

    @unittest.skipUnless(os.environ.get('CC_TEST_GIT_NATIVE_EXE'),'Native roundtrip requested separately')
    def test_real_native_windows_download_and_upload_keep_the_same_tree_and_git_commit(self):
        import pwd
        config={'url':self.url,'ca_pem':self.ca,'commit':self.expected,
            'launch':{'kind':'wsl','distribution':'Ubuntu-26.04','user':pwd.getpwuid(os.getuid()).pw_name,
                'python':sys.executable,'module_dir':str(Path(__import__('consolecrypt_runner').__file__).resolve().parent.parent),
                'root':str(self.root/'native-helper')}}
        result=subprocess.run([os.environ['CC_TEST_GIT_NATIVE_EXE']],input=(json.dumps(config)+'\n').encode(),
            stdout=subprocess.PIPE,stderr=subprocess.PIPE,timeout=90)
        self.assertEqual(result.returncode,0,result.stderr.decode(errors='replace'))
        report=json.loads(result.stdout)
        self.assertEqual(report['status'],'PASS')
        self.assertTrue(report['native_windows_download'] and report['independent_sha256'] and report['portable_modes'])
    @unittest.skipUnless(os.environ.get('CC_TEST_GIT_APP_CORE_EXE'),'AppCore source import requested separately')
    def test_actual_app_core_import_is_durable_and_keeps_edits_separate_from_execution(self):
        import pwd
        config={'url':self.url,'ca_pem':self.ca,'commit':self.expected,
            'launch':{'kind':'wsl','distribution':'Ubuntu-26.04','user':pwd.getpwuid(os.getuid()).pw_name,
                'python':sys.executable,'module_dir':str(Path(__import__('consolecrypt_runner').__file__).resolve().parent.parent),
                'root':str(self.root/'appcore-helper')}}
        result=subprocess.run([os.environ['CC_TEST_GIT_APP_CORE_EXE']],input=(json.dumps(config)+'\n').encode(),
            stdout=subprocess.PIPE,stderr=subprocess.PIPE,timeout=100)
        self.assertEqual(result.returncode,0,result.stderr.decode(errors='replace'))
        report=json.loads(result.stdout)
        self.assertEqual(report['status'],'PASS')
        self.assertTrue(report['durable_import'] and report['manual_edits_preserved'] and report['source_metadata_protected'])
        self.assertTrue(report['original_tree_diff_verified'] and report['stale_diff_denied'])
        print(json.dumps({'native_appcore_git_diff':True,'original_tree_diff_verified':report['original_tree_diff_verified'],
                          'stale_diff_denied':report['stale_diff_denied']}),flush=True)
    def test_unknown_tls_root_and_symlink_fail_without_publishing_a_revision(self):
        with self.assertRaises(RunnerError):capture_git(self.store,self.url,'main',threading.Event())
        self.assertEqual(self.store.list(),[])
        (self.repo/'outside').symlink_to('/usr/local/consolecrypt-synthetic-private')
        self.commit()
        with self.assertRaises(RunnerError):capture_git(self.store,self.url,'main',threading.Event(),ca_pem=self.ca)
        self.assertEqual(self.store.list(),[])
    def test_cancel_while_tls_response_is_pending_never_publishes_partial_files(self):
        self.pause=threading.Event()
        cancel=threading.Event()
        with ThreadPoolExecutor(1) as executor:
            future=executor.submit(capture_git,self.store,self.url,'main',cancel,ca_pem=self.ca)
            try:
                self.assertTrue(self.received.wait(10));cancel.set()
                with self.assertRaises(RunnerError):future.result(timeout=20)
            finally:cancel.set();self.pause.set()
        self.assertEqual(self.store.list(),[])
        self.assertEqual(list(self.root.glob('git-import-*')),[])
