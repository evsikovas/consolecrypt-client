"""New normal-job Vault seams; run in the existing owned non-root Linux fixture."""
import base64
import hashlib
import json
import os
from pathlib import Path
import socket
import tempfile
import threading
import unittest
import uuid
from consolecrypt_runner.errors import RunnerError
from consolecrypt_runner.parameters import validate_parameters, parameters_digest
from consolecrypt_runner.protocol import encode, decode
from consolecrypt_runner.vault_job_broker import VaultJobBroker, job_capsule
from consolecrypt_runner.vault_validation_channel import VaultValidationChannel
from consolecrypt_runner.vault_sidecar import PinnedVaultSidecar, trusted_client_source
from consolecrypt_runner.dependency_helper import operation_path
from consolecrypt_runner.store import atomic_json


def binding():
    return {'version':1,'scope_id':str(uuid.uuid4()),'operation_id':str(uuid.uuid4()),
            'reference_sha256':'a'*64,'environment_digest':'b'*64,'helper_digest':'c'*64}

def capsule():
    return {'version':1,'purpose':'ansible_vault_password','job_id':str(uuid.uuid4()),
            'manifest_digest':'d'*64,'revision_digest':'e'*64,'reference_sha256':'a'*64}

def exchange(directory,request=None):
    fd=os.open(directory,os.O_RDONLY|os.O_DIRECTORY|os.O_NOFOLLOW)
    try:
        with socket.socket(socket.AF_UNIX) as client:
            client.settimeout(3);client.connect('/proc/self/fd/'+str(fd)+'/broker.sock')
            client.sendall(encode(request or {'version':1,'operation':'ansible_vault_password'})+b'\n')
            return decode(client.makefile('rb').readline(8193))
    finally:os.close(fd)

class Channel:
    def __init__(self,fail=False):self.calls=0;self.revoked=0;self.fail=fail
    def call_job(self,value,*,cancel,timeout):
        self.calls+=1
        if self.fail:raise RunnerError('unavailable','fixed denial')
        return {'password_base64':base64.b64encode(b'synthetic-password42\n').decode()}
    def revoke(self):self.revoked+=1

class VaultJobTests(unittest.TestCase):
    def test_legacy_parameters_are_byte_identical_and_binding_is_closed(self):
        old={'extra_vars':{},'tags':[],'skip_tags':[],'diff':False}
        self.assertEqual(validate_parameters(old),old)
        self.assertEqual(parameters_digest(old),hashlib.sha256(encode(old)).hexdigest())
        good=dict(old,vault=binding());self.assertEqual(validate_parameters(good),good)
        for change in ({'vault':None},{'vault':dict(good['vault'],credential_id=str(uuid.uuid4()))},
                       {'vault':dict(good['vault'],version=True)}, {'vault':dict(good['vault'],operation_id=str(uuid.UUID(int=0)))}, {'diff':True}):
            with self.assertRaises(RunnerError):validate_parameters(dict(good,**change))
    def test_job_channel_cannot_consume_validation_response(self):
        channel=VaultValidationChannel();cancel=threading.Event()
        def send(frame):
            self.assertEqual(frame['type'],'vault_job_request')
            channel.deliver({'version':1,'type':'vault_validation_response','request_id':frame['request_id'],
                             'result':{'password_base64':base64.b64encode(b'synthetic-password42\n').decode()},'error':None})
        channel.attach_sender(send)
        with self.assertRaises(RunnerError):channel.call_job(capsule(),cancel=cancel,timeout=1)
        self.assertEqual(channel.pending,{})
        channel.close()
    def test_one_actual_socket_loader_never_retries_native_after_success_or_failure(self):
        for fail in (False,True):
            with self.subTest(fail=fail),tempfile.TemporaryDirectory() as temp:
                channel=Channel(fail);cancel=threading.Event();directory=Path(temp)/'socket'
                with VaultJobBroker(directory,capsule(),channel,cancel):
                    first=exchange(directory);second=exchange(directory)
                    self.assertEqual(channel.calls,1)
                    self.assertIsNotNone(second['error'])
                    self.assertEqual(first['error'] is not None,fail)
                self.assertFalse(directory.exists())
    def test_fd_pinned_phase_rejects_changed_client_and_named_operation_replacement(self):
        with tempfile.TemporaryDirectory() as temp:
            configured=Path(temp)/'jobs';value=binding();request=capsule()
            operation=operation_path(str(configured),value['operation_id'],_purpose='vault_validation')
            operation.mkdir(parents=True,mode=0o700)
            for parent in (operation.parent.parent,operation.parent):os.chmod(parent,0o700)
            data=operation/'data';data.mkdir(mode=0o700);phase=data/'vault-execution';phase.mkdir(mode=0o700)
            script=trusted_client_source('execution');client=phase/'password-client';client.write_bytes(script);client.chmod(0o500)
            atomic_json(phase/'phase.json',{'version':1,'phase':'execution','binding':value,
                'revision_digest':request['revision_digest'],'manifest_digest':request['manifest_digest'],
                'client_sha256':hashlib.sha256(script).hexdigest()})
            channel=Channel()
            with VaultJobBroker(phase/'socket',request,channel,threading.Event(),keep_owned_directory=True):
                with PinnedVaultSidecar(str(configured),value,'execution',request['revision_digest'],request['manifest_digest']) as pinned:
                    mounts=pinned.mounts();self.assertEqual(tuple(fd for fd,_ in mounts),pinned.inherited_fds());self.assertTrue(all(type(fd) is int for fd,_ in mounts))
                    from consolecrypt_runner.sandbox import bubblewrap_argv,run_bounded
                    import sys
                    identities=[(fd,os.fstat(fd).st_dev,os.fstat(fd).st_ino) for fd in pinned.inherited_fds()]
                    proof=Path(temp)/'fd-proof.py'
                    proof.write_text('import os,stat\nassert os.access("/tool/vault-password-client",os.X_OK)\nassert stat.S_ISSOCK(os.stat("/run/consolecrypt-vault/broker.sock").st_mode)\nfor fd,device,inode in '+repr(identities)+':\n try:s=os.fstat(fd)\n except OSError:continue\n assert (s.st_dev,s.st_ino)!=(device,inode),"original setup FD must not survive in guest"\nprint("fixed client/socket mounts usable; setup descriptors closed")\n')
                    workspace=Path(temp)/'workspace';workspace.mkdir(mode=0o700)
                    result=run_bounded(bubblewrap_argv(workspace,[sys.executable,'/tool/fd-proof.py'],extra_readonly=[(proof,'/tool/fd-proof.py')],vault_channel=pinned),timeout=5,output_limit=4096,grace=1,vault_channel=pinned)
                    self.assertEqual(result.exit_code,0,result.output)
                    self.assertIn('setup descriptors closed',result.output)
                    client.chmod(0o600)
                    with self.assertRaises(RunnerError):pinned.recheck()
                    client.chmod(0o500)
                    moved=operation.with_name(operation.name+'-moved');operation.rename(moved)
                    operation.mkdir(mode=0o700)
                    with self.assertRaises(RunnerError):pinned.recheck()
                    operation.rmdir();moved.rename(operation)
            self.assertEqual(channel.calls,0)

    def test_actual_helper_sidecar_begin_finish_and_material_create_fixed_metadata(self):
        from consolecrypt_runner.vault_validation_helper import ValidationHelper
        with tempfile.TemporaryDirectory() as temp:
            directory=Path(temp);source=directory/'source';source.mkdir(mode=0o700)
            value=binding();selected=str(uuid.uuid4())
            reference=encode({'version':1,'ansible_vault':{'credential_id':selected}})
            (source/'.consolecrypt-secret-inputs.json').write_bytes(reference)
            (source/'playbook.yml').write_text('---\n- hosts: fixture\n  tasks: []\n')
            value['reference_sha256']=hashlib.sha256(reference).hexdigest()
            helper=ValidationHelper(directory/'jobs',value['operation_id'])
            try:
                uploaded=helper.revisions.capture(source);children=[]
                for phase in ('syntax','lint'):
                    for index in (0,1):children.append({'version':1,'purpose':'ansible_vault_validation_password',
                        'scope_id':value['scope_id'],'phase':phase,'call_index':index,'phase_nonce':str(uuid.uuid4()),
                        'revision_digest':uploaded['digest'],**{k:value[k] for k in ('reference_sha256','environment_digest','helper_digest')}})
                result=helper.request('vault_sidecar_begin',{'revision_id':uploaded['revision_id'],'binding':value,'children':children},threading.Event())
                self.assertEqual(result['phases'],['syntax','lint'])
                for phase in ('syntax','lint'):
                    with PinnedVaultSidecar(str(directory/'jobs'),value,phase,uploaded['digest']) as pin:pin.recheck()
                self.assertTrue(helper.request('vault_sidecar_finish_validation',{},threading.Event())['validation_closed'])
                request=capsule();request['revision_digest']=uploaded['digest'];request['reference_sha256']=value['reference_sha256']
                self.assertEqual(helper.request('vault_sidecar_material',{'capsule':request},threading.Event())['phase'],'execution')
                with PinnedVaultSidecar(str(directory/'jobs'),value,'execution',uploaded['digest'],request['manifest_digest']) as pin:pin.recheck()
            finally:helper.close()

if __name__=='__main__':unittest.main()
