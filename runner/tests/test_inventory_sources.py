import base64
import hashlib
import json
import os
from pathlib import Path
import tempfile
import threading
import subprocess
import sys
import unittest
from unittest.mock import patch

from consolecrypt_runner.errors import RunnerError
from consolecrypt_runner.inventory import StaticInventory
from consolecrypt_runner.inventory_sources import InventoryReceiver,capture_inventory,inventory_chunk
from consolecrypt_runner.inventory_worker import validate_sources,variable_target
from consolecrypt_runner.inventory_artifacts import InventoryUploads,read_artifact
from consolecrypt_runner.protocol import encode
from consolecrypt_runner.revisions import RevisionRepository
from consolecrypt_runner.sandbox import probe_sandbox,run_bounded


class SourceInputTests(unittest.TestCase):
    def test_variable_targets_are_explicit_and_cannot_escape_the_scope(self):
        self.assertEqual(variable_target('inventory/group_vars/web.yml','group_vars'),'web')
        self.assertEqual(variable_target('inventory/host_vars/service_a/20-defaults.json','host_vars'),'service_a')
        for path,scope in [('inventory/group_vars/web.yml','host_vars'),('../group_vars/web.yml','group_vars'),('inventory/host_vars','host_vars')]:
            with self.assertRaises(RunnerError):variable_target(path,scope)
    def test_large_inventory_artifact_is_chunked_hash_bound_and_never_an_execution_grant(self):
        with tempfile.TemporaryDirectory() as temporary:
            root=Path(temporary);source=root/'source';source.mkdir();(source/'p.yml').write_text('---\n')
            repository=RevisionRepository(root/'revisions');revision=repository.capture(source)
            uploads=InventoryUploads(repository);cancel=threading.Event()
            value={'version':1,'hosts':[{'alias':'web','host_id':None,'address':'192.0.2.1','port':22,'username':None,
                'variables':{'large_value':'x'*1200000,'threshold':1e-7},'sources':['fixture'],'credential_refs':[]}],
                'groups':{},'source_digests':{'fixture':'a'*64}}
            raw=json.dumps(value,separators=(',',':')).encode()
            start=uploads.request('inventory_upload_begin',{'revision_id':revision['revision_id'],'digest':hashlib.sha256(raw).hexdigest(),'bytes':len(raw)},cancel)
            for offset in range(0,len(raw),128*1024):
                result=uploads.request('inventory_upload_chunk',{**start,'offset':offset,'data':base64.b64encode(raw[offset:offset+128*1024]).decode()},cancel)
                self.assertEqual(result['offset'],min(len(raw),offset+128*1024))
            receipt=uploads.request('inventory_upload_commit',start,cancel)
            self.assertGreater(receipt['bytes'],1024*1024)
            inventory,canonical=read_artifact(repository,revision['revision_id'],{k:receipt[k] for k in ('key','digest')})
            self.assertEqual(inventory.to_snapshot(),value);self.assertEqual(hashlib.sha256(canonical).hexdigest(),receipt['digest'])
            self.assertEqual(uploads.pending,{})
            with self.assertRaises(RunnerError):inventory.runtime_inventory({'targets':[{'alias':'web','host_id':str(__import__('uuid').uuid4()),
                'address':'192.0.2.1','port':22,'username':'user','credential_refs':[]}]})
            with self.assertRaises(RunnerError):read_artifact(repository,revision['revision_id'],{'key':receipt['key'],'digest':'0'*64})

    def test_incomplete_cancelled_expired_or_excess_transfers_cannot_publish(self):
        with tempfile.TemporaryDirectory() as temporary:
            root=Path(temporary);source=root/'source';source.mkdir();(source/'p.yml').write_text('---\n')
            repository=RevisionRepository(root/'revisions');revision=repository.capture(source)
            uploads=InventoryUploads(repository);cancel=threading.Event()
            spec={'revision_id':revision['revision_id'],'digest':hashlib.sha256(b'{}').hexdigest(),'bytes':2}
            first=uploads.request('inventory_upload_begin',spec,cancel);second=uploads.request('inventory_upload_begin',spec,cancel)
            with self.assertRaises(RunnerError):uploads.request('inventory_upload_begin',spec,cancel)
            with self.assertRaises(RunnerError):uploads.request('inventory_upload_chunk',{**first,'offset':1,'data':'e30='},cancel)
            with self.assertRaises(RunnerError):uploads.request('inventory_upload_commit',first,cancel)
            cancel.set()
            with self.assertRaises(RunnerError):uploads.request('inventory_upload_chunk',{**second,'offset':0,'data':'e30='},cancel)
            uploads.request('inventory_upload_abort',second,cancel)
            self.assertEqual(uploads.pending,{})
            self.assertFalse((repository.root/revision['revision_id']/'inventory-previews').exists())
            cancel.clear();expired=uploads.request('inventory_upload_begin',spec,cancel)
            uploads.pending[expired['transfer_id']]['deadline']=0
            with self.assertRaises(RunnerError):uploads.request('inventory_upload_commit',expired,cancel)
            uploads.close();self.assertEqual(uploads.pending,{})

    def test_sources_cannot_select_scripts_traversal_duplicates_or_unknown_formats(self):
        for value in [[],[{'path':'../outside','format':'ini'}],[{'path':'script.py','format':'ini'}],
                      [{'path':'a.ini','format':'plugin'}],[{'path':'a.ini','format':'ini','execute':True}],
                      [{'path':'a.ini','format':'ini'},{'path':'a.ini','format':'yaml'}]]:
            with self.subTest(value=value),self.assertRaises(RunnerError):validate_sources(value)

    def test_partial_out_of_order_and_changed_worker_streams_are_rejected(self):
        header={'type':'inventory','bytes':2}
        for frame in [ {'type':'chunk','offset':1,'data':base64.b64encode(b'{}').decode()},
                       {'type':'complete','sha256':'0'*64} ]:
            receiver=InventoryReceiver();receiver.feed(encode(header)+b'\n')
            with self.assertRaises(RunnerError):receiver.feed(encode(frame)+b'\n')
            self.assertFalse(receiver.complete)
        receiver=InventoryReceiver();receiver.feed(encode(header)+b'\n')
        self.assertFalse(receiver.complete)


@unittest.skipUnless(os.name=='posix' and os.getuid()!=0,'Prepared non-root Unix runtime required')
class SourceNamespaceTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        capabilities=probe_sandbox()
        if not all(capabilities.get(key) for key in ('filesystem','pid','network')):
            raise unittest.SkipTest('Prepared filesystem/PID/network namespaces required')

    def setUp(self):
        self.temporary=tempfile.TemporaryDirectory(prefix='cc-static-source-')
        self.addCleanup(self.temporary.cleanup)
        self.root=Path(self.temporary.name)
        self.source=self.root/'source';self.source.mkdir(mode=0o700)
        (self.source/'defaults.ini').write_text('[all:vars]\nansible_user=deployer\nansible_port=2222\n')
        (self.source/'nested').mkdir(mode=0o700)
        (self.source/'nested/hosts.yml').write_bytes(b'\xef\xbb\xbfweb:\n  hosts:\n    service_a: {ansible_host: 192.0.2.10}\n')
        self.repository=RevisionRepository(self.root/'revisions')
        self.captured=self.repository.capture(self.source)
        self.id=self.captured['revision_id']
        self.sources=[{'path':'defaults.ini','format':'ini'},{'path':'nested/hosts.yml','format':'yaml'}]

    def test_real_namespace_applies_directory_vars_and_hashes_every_original_file(self):
        (self.source/'nested/group_vars').mkdir();(self.source/'nested/host_vars').mkdir()
        group=self.source/'nested/group_vars/web.yml';group.write_text('ansible_port: 3333\nenabled: true\n')
        host=self.source/'nested/host_vars/service_a.yml';host.write_text('ansible_user: owner\nthreshold: 0.0000001\n')
        captured=self.repository.capture(self.source);sources=[{'path':'defaults.ini','format':'ini'},
            {'path':'nested/hosts.yml','format':'yaml'},{'path':'nested/group_vars/web.yml','format':'group_vars'},
            {'path':'nested/host_vars/service_a.yml','format':'host_vars'}]
        receipt=capture_inventory(self.repository,captured['revision_id'],sources,threading.Event())
        path=self.repository.root/captured['revision_id']/'inventory-previews'/(receipt['key']+'.json')
        value=json.loads(path.read_bytes());self.assertEqual(value['hosts'][0]['port'],3333);self.assertEqual(value['hosts'][0]['username'],'owner')
        self.assertEqual(value['hosts'][0]['variables']['threshold'],1e-7);self.assertEqual(value['groups']['web']['vars'],{'enabled':True})
        self.assertEqual(value['source_digests']['nested/group_vars/web.yml'],hashlib.sha256(group.read_bytes()).hexdigest())
        self.assertEqual(value['source_digests']['nested/host_vars/service_a.yml'],hashlib.sha256(host.read_bytes()).hexdigest())
        self.assertEqual(len(value['source_digests']),4);self.assertIsNone(value['hosts'][0]['host_id']);self.assertEqual(value['hosts'][0]['credential_refs'],[])
    def test_real_namespace_preview_is_bound_to_original_files_and_transfers_in_chunks(self):
        receipt=capture_inventory(self.repository,self.id,self.sources,threading.Event())
        self.assertEqual(receipt['revision_digest'],self.captured['digest'])
        self.assertEqual(receipt['host_count'],1)
        body=bytearray()
        while True:
            page=inventory_chunk(self.repository,self.id,receipt['key'],len(body),127)
            self.assertEqual(page['revision_digest'],receipt['revision_digest'])
            self.assertEqual(page['offset'],len(body))
            body.extend(base64.b64decode(page['data'],validate=True))
            if page['eof']:break
        self.assertEqual(len(body),receipt['bytes'])
        self.assertEqual(hashlib.sha256(body).hexdigest(),receipt['digest'])
        value=json.loads(body);inventory=StaticInventory.from_snapshot(value)
        self.assertEqual(inventory.hosts['service_a'].username,'deployer')
        self.assertEqual(inventory.hosts['service_a'].port,2222)
        self.assertIsNone(inventory.hosts['service_a'].host_id)
        self.assertEqual(inventory.hosts['service_a'].credential_refs,())
        self.assertEqual(value['source_digests']['nested/hosts.yml'],
                         hashlib.sha256((self.source/'nested/hosts.yml').read_bytes()).hexdigest())
        self.assertEqual(self.repository.read(self.id)[1].digest,self.captured['digest'])
        self.repository.release(self.id,referenced=lambda *_:False)
        with self.assertRaises(RunnerError):inventory_chunk(self.repository,self.id,receipt['key'],0,127)

    def test_a_swapped_parent_link_cannot_read_an_outside_file_or_publish_a_preview(self):
        project,_=self.repository.read(self.id)
        outside=self.root/'outside';outside.mkdir(mode=0o700)
        (outside/'hosts.yml').write_text('private-marker-never-a-source')
        (project/'nested/hosts.yml').unlink();(project/'nested').rmdir()
        (project/'nested').symlink_to(outside,target_is_directory=True)
        with self.assertRaises(RunnerError):capture_inventory(self.repository,self.id,self.sources,threading.Event())
        self.assertFalse((project.parent/'inventory-previews').exists())

    def test_cancelled_completion_and_cache_quota_do_not_publish_partial_artifacts(self):
        cancel=threading.Event()
        def cancel_on_output(argv,**options):
            receive=options['on_chunk']
            def feed(chunk):receive(chunk);cancel.set()
            options['on_chunk']=feed
            return run_bounded(argv,**options)
        with patch('consolecrypt_runner.inventory_sources.run_bounded',side_effect=cancel_on_output),self.assertRaises(RunnerError):
            capture_inventory(self.repository,self.id,self.sources,cancel)
        project,_=self.repository.read(self.id)
        self.assertFalse((project.parent/'inventory-previews').exists())
        with patch('consolecrypt_runner.inventory_sources.MAX_CACHE_BYTES',1),self.assertRaisesRegex(RunnerError,'Release unused'):
            capture_inventory(self.repository,self.id,self.sources,threading.Event())
        self.assertFalse(list((project.parent/'inventory-previews').glob('*.json')))

    def test_executable_sources_are_rejected_before_namespace_execution(self):
        (self.source/'defaults.ini').chmod(0o700)
        captured=self.repository.capture(self.source)
        with patch('consolecrypt_runner.inventory_sources.run_bounded') as run,self.assertRaises(RunnerError):
            capture_inventory(self.repository,captured['revision_id'],self.sources,threading.Event())
        run.assert_not_called()

    @unittest.skipUnless(os.environ.get('CC_TEST_INVENTORY_NATIVE_EXE'),'Native inventory receiver requested separately')
    def test_real_native_windows_receiver_checks_source_hashes_and_cannot_gain_authority(self):
        import pwd
        configuration={'launch':{'kind':'wsl','distribution':'Ubuntu-26.04','user':pwd.getpwuid(os.getuid()).pw_name,
            'python':sys.executable,'module_dir':str(Path(__import__('consolecrypt_runner').__file__).resolve().parent.parent),
            'root':str(self.root/'native-helper')}}
        response=subprocess.run([os.environ['CC_TEST_INVENTORY_NATIVE_EXE']],input=(json.dumps(configuration)+'\n').encode(),
            capture_output=True,timeout=90)
        self.assertEqual(response.returncode,0,response.stderr.decode(errors='replace'))
        result=json.loads(response.stdout)
        self.assertEqual(result['status'],'PASS')
        self.assertTrue(result['native_windows_inventory_preview'] and result['independent_digest'] and result['source_has_no_authority'])

    @unittest.skipUnless(os.environ.get('CC_TEST_INVENTORY_APP_CORE_EXE'),'AppCore inventory lifecycle requested separately')
    def test_actual_appcore_preview_is_bounded_profile_fenced_and_never_connects_targets(self):
        import pwd
        configuration={'launch':{'kind':'wsl','distribution':'Ubuntu-26.04','user':pwd.getpwuid(os.getuid()).pw_name,
            'python':sys.executable,'module_dir':str(Path(__import__('consolecrypt_runner').__file__).resolve().parent.parent),
            'root':str(self.root/'appcore-helper')}}
        response=subprocess.run([os.environ['CC_TEST_INVENTORY_APP_CORE_EXE']],input=(json.dumps(configuration)+'\n').encode(),
            capture_output=True,timeout=100)
        self.assertEqual(response.returncode,0,response.stderr.decode(errors='replace'))
        result=json.loads(response.stdout)
        self.assertEqual(result['status'],'PASS')
        self.assertTrue(result['native_appcore_inventory_preview'] and result['public_pages_only'] and result['lock_fences_old_ids'])


if __name__=='__main__':unittest.main()
