import base64,io,json,os,socket,sys,tempfile,threading,time,unittest,uuid
from pathlib import Path
from unittest.mock import patch
from consolecrypt_runner.vault_validation_broker import VaultValidationBroker
from consolecrypt_runner import vault_password_client as client
from consolecrypt_runner.errors import RunnerError

class SyntheticNative:
    """Unit-side stub only; actual Native authority/pipeline is NOT claimed."""
    def __init__(self,values=None):self.calls=[];self.revoked=False;self.values=values or [b'synthetic-password\n']*2
    def call(self,capsule,*,cancel,timeout):
        self.calls.append(dict(capsule));value=self.values[len(self.calls)-1]
        if isinstance(value,Exception):raise value
        return {'password_base64':base64.b64encode(value).decode()}
    def revoke(self):self.revoked=True

def capsules(phase='syntax'):
    base={'version':1,'purpose':'ansible_vault_validation_password','scope_id':str(uuid.uuid4()),'phase':phase,'environment_digest':'a'*64,'helper_digest':'b'*64,'revision_digest':'c'*64,'reference_sha256':'d'*64}
    return [dict(base,call_index=i,phase_nonce=str(uuid.uuid4())) for i in range(2)]
def request(directory,value=None):
    value=value or {'version':1,'operation':'ansible_vault_password'}
    with socket.socket(socket.AF_UNIX,socket.SOCK_STREAM) as sock:
        sock.settimeout(6);sock.connect(str(directory/'broker.sock'));sock.sendall((json.dumps(value)+'\n').encode());return json.loads(sock.makefile('rb').readline(8193))

class VaultBrokerTests(unittest.TestCase):
    def test_root_or_nonlinux_denied_before_socket_or_channel(self):
        import consolecrypt_runner.vault_validation_broker as module
        with tempfile.TemporaryDirectory() as temporary:
            directory=Path(temporary)/'phase';channel=SyntheticNative()
            for target,value in [('platform','win32'),('platform','darwin')]:
                with patch.object(module.sys,target,value),self.assertRaises(RunnerError):VaultValidationBroker(directory,capsules(),channel,threading.Event())
                self.assertFalse(directory.exists())
            with patch.object(module.os,'getuid',return_value=0),self.assertRaises(RunnerError):VaultValidationBroker(directory,capsules(),channel,threading.Event())
            self.assertFalse(directory.exists());self.assertEqual(channel.calls,[])
    def test_sequential_reply_then_delayed_handler_cleanup_hands_off_without_reset(self):
        import socketserver
        reached=threading.Event();release=threading.Event();accepted=threading.Event()
        channel=SyntheticNative();response=[]
        with tempfile.TemporaryDirectory() as temporary:
            directory=Path(temporary)/'phase'
            with VaultValidationBroker(directory,capsules(),channel,threading.Event()) as broker:
                original_finish=socketserver.StreamRequestHandler.finish
                def delayed_finish(handler):
                    if handler.server is broker.server and not reached.is_set():
                        reached.set();self.assertTrue(release.wait(2))
                    original_finish(handler)
                original_accept=broker.server.process_request
                count=[0]
                def observe_accept(*args):
                    count[0]+=1
                    if count[0]==2:accepted.set()
                    return original_accept(*args)
                with patch.object(socketserver.StreamRequestHandler,'finish',delayed_finish),patch.object(broker.server,'process_request',observe_accept):
                    self.assertIsNone(request(directory)['error']);self.assertTrue(reached.wait(1))
                    def second():
                        try:response.append(request(directory))
                        except Exception as error:response.append(type(error).__name__)
                    worker=threading.Thread(target=second);worker.start();self.assertTrue(accepted.wait(1));self.assertEqual(len(channel.calls),1)
                    release.set();worker.join(2);self.assertFalse(worker.is_alive());self.assertIsInstance(response[0],dict);self.assertIsNone(response[0]['error'])
                self.assertEqual([r['call_index'] for r in channel.calls],[0,1])
    def test_two_distinct_children_third_denied_without_native_call(self):
        channel=SyntheticNative();children=capsules();cancel=threading.Event()
        with tempfile.TemporaryDirectory() as temporary:
            directory=Path(temporary)/'phase'
            with VaultValidationBroker(directory,children,channel,cancel):
                for _ in range(2):self.assertIsNone(request(directory)['error'])
                self.assertIsNotNone(request(directory)['error']);self.assertEqual(len(channel.calls),2)
                self.assertEqual([r['call_index'] for r in channel.calls],[0,1])
                self.assertNotEqual(channel.calls[0]['phase_nonce'],channel.calls[1]['phase_nonce'])
            self.assertFalse(directory.exists());self.assertTrue(channel.revoked)
    def test_client_cannot_select_credential_host_nonce_or_other_operation(self):
        channel=SyntheticNative()
        with tempfile.TemporaryDirectory() as temporary:
            directory=Path(temporary)/'phase'
            with VaultValidationBroker(directory,capsules(),channel,threading.Event()):
                for payload in [dict(version=1,operation='exec'),dict(version=1,operation='ansible_vault_password',credential_id=str(uuid.uuid4())),dict(version=1,operation='ansible_vault_password',call_index=1),dict(version=True,operation='ansible_vault_password')]:
                    self.assertIsNotNone(request(directory,payload)['error'])
                self.assertEqual(channel.calls,[]);self.assertIsNone(request(directory)['error'])
    def test_forged_or_duplicate_child_capsules_fail_before_socket_creation(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory=Path(temporary)/'phase'
            for field,value in [('phase','lint'),('scope_id',str(uuid.uuid4())),('environment_digest','e'*64),('call_index',0),('phase_nonce',None),('approved',True)]:
                children=capsules();children[1][field]=children[0]['phase_nonce'] if value is None else value
                with self.assertRaises(RunnerError):VaultValidationBroker(directory,children,SyntheticNative(),threading.Event())
                self.assertFalse(directory.exists())
    def test_invalid_native_password_line_consumes_child_without_echo(self):
        for value in [b'one\ntwo\n',b'one\rtwo\n',b'nul\x00value\n',b' leading\n',b'\xff\n',b'x'*4097+b'\n']:
            channel=SyntheticNative([value,b'valid-second-child\n'])
            with tempfile.TemporaryDirectory() as temporary:
                directory=Path(temporary)/'phase'
                with VaultValidationBroker(directory,capsules(),channel,threading.Event()):
                    failed=request(directory);self.assertIsNotNone(failed['error']);self.assertIsNone(failed['result'])
                    self.assertNotIn('leading',json.dumps(failed));self.assertIsNone(request(directory)['error'])
                    self.assertEqual([r['call_index'] for r in channel.calls],[0,1])
    def test_native_failure_does_not_echo_diagnostic_or_replay_child(self):
        marker='SYNTHETIC_DECRYPTED_DIAGNOSTIC_917';channel=SyntheticNative([RunnerError('vault_unavailable',marker),b'valid\n'])
        with tempfile.TemporaryDirectory() as temporary:
            directory=Path(temporary)/'phase'
            with VaultValidationBroker(directory,capsules(),channel,threading.Event()):
                response=request(directory);self.assertNotIn(marker,json.dumps(response));self.assertIsNotNone(response['error'])
                self.assertIsNone(request(directory)['error']);self.assertEqual(len(channel.calls),2)
    def test_cancelled_phase_denies_without_lookup(self):
        cancel=threading.Event();cancel.set();channel=SyntheticNative()
        with tempfile.TemporaryDirectory() as temporary:
            directory=Path(temporary)/'phase'
            with VaultValidationBroker(directory,capsules(),channel,cancel):self.assertIsNotNone(request(directory)['error'])
            self.assertEqual(channel.calls,[])
    def test_close_cancels_and_joins_actual_pending_handler_before_removing_socket(self):
        started=threading.Event();finished=threading.Event()
        class Waiting(SyntheticNative):
            def call(self,capsule,*,cancel,timeout):
                self.calls.append(capsule);started.set()
                if not cancel.wait(timeout):raise AssertionError('bounded cancellation missing')
                finished.set();raise RunnerError('cancelled','private raw diagnostics withheld')
        with tempfile.TemporaryDirectory() as temporary:
            directory=Path(temporary)/'phase';channel=Waiting();broker=VaultValidationBroker(directory,capsules(),channel,threading.Event())
            worker=threading.Thread(target=lambda:request(directory));worker.start();self.assertTrue(started.wait(2))
            broker.close();worker.join(2);self.assertFalse(worker.is_alive());self.assertTrue(finished.is_set());self.assertFalse(directory.exists())

class ClientTests(unittest.TestCase):
    def run_client(self,response,args=None):
        with tempfile.TemporaryDirectory() as temporary:
            path=Path(temporary)/'private.sock';server=socket.socket(socket.AF_UNIX,socket.SOCK_STREAM);server.bind(str(path));server.listen(1);server.settimeout(1)
            received=[]
            def serve():
                try:
                    connection,_=server.accept()
                    with connection:received.append(connection.recv(1024));connection.sendall(response)
                except (socket.timeout,OSError):pass
            worker=threading.Thread(target=serve);worker.start();output=type('PrivatePipe',(),{'buffer':io.BytesIO()})()
            with patch.object(client,'SOCKET',str(path)),patch.object(sys,'argv',args or ['fixed-client']),patch.object(sys,'stdout',output):code=client.main()
            worker.join(2);server.close();self.assertFalse(worker.is_alive())
            return code,output.buffer.getvalue(),received
    def test_exact_utf8_line_only_to_private_stdout(self):
        password='РїР°СЂРѕР»СЊрџ”’42\n'.encode();response=json.dumps({'version':1,'result':{'password_base64':base64.b64encode(password).decode()},'error':None}).encode()+b'\n'
        code,output,requests=self.run_client(response);self.assertEqual(code,0);self.assertEqual(output,password)
        self.assertEqual(json.loads(requests[0]),{'version':1,'operation':'ansible_vault_password'})
    def test_bad_frames_and_passwords_return_no_plaintext_or_exception_text(self):
        for raw in [b'{"version":1,"version":1,"result":{},"error":null}\n',b'X'*8193+b'\n',b'{"version":1,"result":null,"error":{"message":"DECRYPTED_917"}}\n']:
            code,output,_=self.run_client(raw);self.assertEqual(code,65);self.assertEqual(output,b'')
        for value in [b'a\nb\n',b'a\0b\n',b'a\rb\n',b'\xff\n',b' leading\n',b'\n']:
            raw=json.dumps({'version':1,'result':{'password_base64':base64.b64encode(value).decode()},'error':None}).encode()+b'\n'
            code,output,_=self.run_client(raw);self.assertEqual(code,65);self.assertEqual(output,b'')
    def test_arbitrary_cli_arguments_never_connect_or_select_scope(self):
        code,output,received=self.run_client(b'{}\n',['fixed-client','--vault-id','other']);self.assertEqual(code,65);self.assertEqual(output,b'');self.assertEqual(received,[])
if __name__=='__main__':unittest.main()
