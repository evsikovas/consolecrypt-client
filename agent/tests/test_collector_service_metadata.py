"""Fixed metadata/argv paths only; no actual unit, delegation, or WAL proof."""
from pathlib import Path
import hashlib
import sys
import unittest
import uuid

COLLECTOR = Path(__file__).resolve().parents[1] / "collector"


class ServiceMetadata(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        sys.path.insert(0, str(COLLECTOR))
        import service_binding
        import service_unit_spec
        import disk_unit_spec
        cls.binding = service_binding
        cls.spec = service_unit_spec
        cls.interactive = disk_unit_spec
        sys.path.pop(0)

    def original(self):
        m = hashlib.sha256(b"synthetic exact full manifest").digest()
        address = bytearray(m[:16])
        address[6] = address[6] & 15 | 0x80
        address[8] = address[8] & 63 | 0x80
        base = self.binding.DiskBinding(
            "localProfile", "11111111-1111-4111-8111-111111111111",
            "22222222-2222-4222-8222-222222222222", "a"*64,
            "33333333-3333-4333-8333-333333333333", "b"*64, "c"*64,
            str(uuid.UUID(bytes=bytes(address))), m.hex())
        return self.binding.ServiceDiskBinding(
            base, "44444444-4444-4444-8444-444444444444", "d"*64, 1, 1, 87400)

    def test_service_reuses_same_collector_wal_address_but_not_interactive_binding(self):
        original = self.original()
        self.assertEqual(original.collector_id, original.original.collector_id)
        with self.assertRaises(ValueError):
            original.check_bytes(original.original.bytes())
        with self.assertRaises(ValueError):
            original.original.check_bytes(original.bytes())
        original.check_bytes(original.bytes())

    def test_changed_service_policy_floor_or_expiry_cannot_adopt_original_binding(self):
        original = self.original()
        for field, value in (
                ("delegation_sha256", "e"*64), ("policy_revision", 2),
                ("revocation_epoch", 2), ("expires_unix_seconds", 87401)):
            args = dict(vars(original), **{field: value})
            changed = self.binding.ServiceDiskBinding(**args)
            with self.assertRaises(ValueError):
                changed.check_bytes(original.bytes())

    def test_service_has_fixed_entry_same_kernel_caps_and_original24h_max(self):
        original = self.original()
        session = "55555555555545558555555555555555"
        spec = self.spec.ServiceUnitSpec.from_private_handoff(
            session, original.collector_id, "f"*64, 86400*10**9)
        args = spec.start_argv()
        self.assertIn(self.spec.ENTRY, args)
        self.assertNotIn(self.interactive.ENTRY, args)
        self.assertIn("--property=RuntimeMaxSec=86390s", args)
        self.assertIn("--property=MemoryMax=536870912", args)
        self.assertIn("--property=MemorySwapMax=0", args)
        self.assertIn("--property=Restart=no", args)
        with self.assertRaises(ValueError):
            self.spec.ServiceUnitSpec.from_private_handoff(
                session, original.collector_id, "f"*64, 86400*10**9+1)

    def test_service_time_window_never_widens_existing_interactive_spec(self):
        original = self.original()
        session = "55555555555545558555555555555555"
        with self.assertRaises(ValueError):
            self.interactive.DiskUnitSpec.from_private_handoff(
                session, original.collector_id, "f"*64, 86400*10**9)
        interactive = self.interactive.DiskUnitSpec.from_private_handoff(
            session, original.collector_id, "f"*64, 300*10**9)
        self.assertIn(self.interactive.ENTRY, interactive.start_argv())
        self.assertNotIn(self.spec.ENTRY, interactive.start_argv())


class ServiceWorkerDiagnostics(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        import importlib.util
        spec=importlib.util.spec_from_file_location("owned_service_daemon_diagnostics",COLLECTOR/"service_daemon.py")
        cls.module=importlib.util.module_from_spec(spec);spec.loader.exec_module(cls.module)

    def test_actual_closed_identity_denial_codes_are_classified_without_echo(self):
        import json
        from types import SimpleNamespace
        from contextlib import redirect_stderr
        from io import StringIO
        for code in ("Storage","CapabilityDenied","UncertainCommit","InvalidTime"):
            observed=self.module._WorkerStderr()
            observed.accept(json.dumps({"event":"identityDenied","code":code}).encode()+b"\n")
            self.assertEqual(observed.code(),code)
            sink=StringIO()
            with redirect_stderr(sink):observed.report(SimpleNamespace(returncode=1,pid=4321))
            value=json.loads(sink.getvalue().split(" ",1)[1])
            self.assertEqual(value["errorCode"],code);self.assertEqual(value["workerExit"],1)
            self.assertNotIn("identityDenied",sink.getvalue())

    def test_malformed_duplicate_extra_unknown_and_secret_inputs_never_echo(self):
        from types import SimpleNamespace
        from contextlib import redirect_stderr
        from io import StringIO
        for raw in (b"not JSON secret-sentinel",b'{"event":"identityDenied","code":"Storage","code":"Expired"}',
                    b'{"event":"identityDenied","code":"Storage","password":"secret-sentinel"}',
                    b'{"event":"identityDenied","code":"secret-sentinel"}',b'{"event":"readerDenied","code":"Storage"}',
                    b'{"event":"identityDenied","code":7}',b'\xff'):
            observed=self.module._WorkerStderr();observed.accept(raw)
            self.assertEqual(observed.code(),"unknown");sink=StringIO()
            with redirect_stderr(sink):observed.report(SimpleNamespace(returncode=1,pid=4321))
            self.assertNotIn("secret-sentinel",sink.getvalue());self.assertNotIn("password",sink.getvalue())
            self.assertEqual(len(sink.getvalue().splitlines()),1)

    def test_overflow_discards_raw_and_cannot_borrow_valid_suffix(self):
        observed=self.module._WorkerStderr();observed.accept(b"secret-sentinel"*400)
        observed.accept(b'{"event":"identityDenied","code":"Storage"}\n')
        self.assertTrue(observed.overflow);self.assertEqual(observed.raw,bytearray())
        self.assertEqual(observed.code(),"unknown")

    def test_nonblocking_poll_drain_is_bounded_even_after_overflow(self):
        from types import SimpleNamespace
        from unittest.mock import patch
        observed=self.module._WorkerStderr();stream=SimpleNamespace(fileno=lambda:42)
        with patch.object(self.module.os,"read",return_value=b"s"*4096)as read:
            observed.drain(stream);self.assertEqual(read.call_count,16)
        self.assertTrue(observed.overflow);self.assertLessEqual(len(observed.raw),4096)
        with patch.object(self.module.os,"read",side_effect=BlockingIOError):observed.drain(stream)
        self.assertEqual(observed.code(),"unknown")



class ServiceStartFailureDiagnostics(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        import ast
        import json,os,re,stat
        # Execute only the actual inert helpers; no installed package imports,
        # unit, Actor or worker is constructed by these component tests.
        body=ast.parse((COLLECTOR/'service_unit_entry.py').read_bytes())
        keep=[node for node in body.body if
            isinstance(node,ast.FunctionDef) and node.name in ('_closed_start_failure','_record_start_failure','_entry_start_failure','_failure_no_late_producer') or
            isinstance(node,ast.Assign) and any(isinstance(t,ast.Name) and t.id in ('_START_STAGE','_START_DIAGNOSTIC_ALLOWED','_START_STAGES','_START_CODES','_START_STATES') for t in node.targets)]
        cls.ns=dict(json=json,os=os,re=re,stat=stat,Path=Path,sys=sys)
        exec(compile(ast.Module(body=keep,type_ignores=[]),'actual-closed-start-helpers','exec'),cls.ns)

    def test_first_gate_static_code_errno_and_unit_state_are_retained_without_echo(self):
        closed=self.ns['_closed_start_failure']
        value=closed('entry','binary',ValueError('disk_binary_changed'))
        self.assertEqual((value['source'],value['stage'],value['code']),('entry','binary','disk_binary_changed'))
        self.assertIsNone(value['errno'])
        value=closed('adapter','unit_start',PermissionError(13,'secret-sentinel'),
            {'Result':'exit-code','ExecMainStatus':'226','ControlGroup':'/secret-sentinel','SubState':'secret-sentinel'})
        self.assertEqual(value['errno'],13);self.assertEqual(value['unit'],{'Result':'exit-code','ExecMainStatus':226})
        self.assertNotIn('secret-sentinel',str(value))

    def test_unknown_malformed_and_unbounded_error_text_never_enters_record(self):
        for error in (ValueError('secret-sentinel'),RuntimeError('secret-sentinel'*10000),OSError(99999,'secret-sentinel')):
            value=self.ns['_closed_start_failure']('secret-sentinel','secret-sentinel',error)
            self.assertEqual(value['source'],'unknown');self.assertEqual(value['stage'],'unknown')
            self.assertEqual(value['code'],'unknown');self.assertIsNone(value['errno'])
            self.assertNotIn('secret-sentinel',str(value));self.assertLess(len(str(value)),2048)

    def test_failure_writer_creates_only_existing_leaf_with_exact600_and_never_overwrites(self):
        from types import SimpleNamespace
        from unittest.mock import patch
        import stat,os
        directory=Path('/run/consolecrypt-agent/disk/'+'1'*32)
        uid=os.getuid() if hasattr(os,'getuid') else 10002
        folder=SimpleNamespace(st_mode=stat.S_IFDIR|0o700,st_uid=uid,st_dev=7,st_ino=8)
        leaf=SimpleNamespace(st_mode=stat.S_IFREG|0o600,st_uid=uid,st_dev=7,st_ino=9,st_nlink=1,st_size=0)
        output=bytearray()
        def write(fd,raw):output.extend(raw);leaf.st_size=len(output);return len(raw)
        with patch.object(self.ns['os'],'getuid',return_value=uid,create=True),patch.object(Path,'lstat',return_value=folder), \
             patch.object(self.ns['os'],'open',side_effect=[10,11]) as opened,patch.object(self.ns['os'],'fstat',side_effect=[folder,leaf,leaf]), \
             patch.object(self.ns['os'],'stat',return_value=leaf),patch.object(self.ns['os'],'write',side_effect=write), \
             patch.object(self.ns['os'],'fchmod',create=True) as chmod,patch.object(self.ns['os'],'fsync'),patch.object(self.ns['os'],'close'), \
             patch.object(self.ns['os'],'O_DIRECTORY',0,create=True),patch.object(self.ns['os'],'O_NOFOLLOW',0,create=True),patch.object(self.ns['os'],'O_CLOEXEC',0,create=True):
            self.assertTrue(self.ns['_record_start_failure'](directory,'entry','binary',ValueError('disk_binary_changed')))
            self.assertEqual(opened.call_args_list[1].args[0],'prometheus-exit.json')
            self.assertTrue(opened.call_args_list[1].args[1]&os.O_EXCL);chmod.assert_called_once_with(11,0o600)
            self.assertLessEqual(len(output),2048)
        with patch.object(self.ns['os'],'getuid',return_value=uid,create=True),patch.object(Path,'lstat',return_value=folder), \
             patch.object(self.ns['os'],'open',side_effect=[10,FileExistsError]),patch.object(self.ns['os'],'fstat',return_value=folder), \
             patch.object(self.ns['os'],'write') as write,patch.object(self.ns['os'],'close'), \
             patch.object(self.ns['os'],'O_DIRECTORY',0,create=True),patch.object(self.ns['os'],'O_NOFOLLOW',0,create=True),patch.object(self.ns['os'],'O_CLOEXEC',0,create=True):
            self.assertFalse(self.ns['_record_start_failure'](directory,'entry','binary',ValueError('disk_binary_changed')))
            write.assert_not_called()

    def test_failure_format_cannot_be_accepted_as_real_process_exit(self):
        import ast,json
        from types import SimpleNamespace
        from unittest.mock import patch
        body=ast.parse((COLLECTOR/'disk_unit_adapter.py').read_bytes())
        actual=next(n for c in body.body if isinstance(c,ast.ClassDef) and c.name=='OwnedUnit' for n in c.body if isinstance(n,ast.FunctionDef) and n.name=='observe_entry_exit')
        raw=json.dumps(self.ns['_closed_start_failure']('entry','binary',ValueError('disk_binary_changed'))).encode()
        ns=dict(regular=lambda *a,**k:(99,raw),os=self.ns['os'],json=json,object_pairs=lambda p:dict(p))
        exec(compile(ast.Module(body=[actual],type_ignores=[]),'actual-exit-validator','exec'),ns)
        unit=SimpleNamespace(directory=Path('/unused'),entry_stopped=True)
        with patch.object(self.ns['os'],'getuid',return_value=10002,create=True),patch.object(self.ns['os'],'close'):
            ns['observe_entry_exit'](unit)
        self.assertFalse(unit.entry_stopped)

    def test_owned_close_errors_cannot_replace_first_failure_or_escape_observer(self):
        from types import SimpleNamespace
        from unittest.mock import patch
        import stat,os
        directory=Path('/run/consolecrypt-agent/disk/'+'1'*32)
        uid=os.getuid() if hasattr(os,'getuid') else 10002
        folder=SimpleNamespace(st_mode=stat.S_IFDIR|0o700,st_uid=uid,st_dev=7,st_ino=8)
        leaf=SimpleNamespace(st_mode=stat.S_IFREG|0o600,st_uid=uid,st_dev=7,st_ino=9,st_nlink=1,st_size=0)
        with patch.object(self.ns['os'],'getuid',return_value=uid,create=True),patch.object(Path,'lstat',return_value=folder), \
             patch.object(self.ns['os'],'open',side_effect=[10,11]),patch.object(self.ns['os'],'fstat',side_effect=[folder,leaf]), \
             patch.object(self.ns['os'],'stat',return_value=leaf),patch.object(self.ns['os'],'write',side_effect=OSError(5,'secret-write-error')), \
             patch.object(self.ns['os'],'fchmod',create=True),patch.object(self.ns['os'],'unlink'), \
             patch.object(self.ns['os'],'close',side_effect=OSError(9,'secret-close-error')) as close, \
             patch.object(self.ns['os'],'O_DIRECTORY',0,create=True),patch.object(self.ns['os'],'O_NOFOLLOW',0,create=True),patch.object(self.ns['os'],'O_CLOEXEC',0,create=True):
            self.assertFalse(self.ns['_record_start_failure'](directory,'entry','binary',ValueError('disk_binary_changed')))
            self.assertEqual([call.args[0] for call in close.call_args_list],[11,10])

    def test_failed_publication_removes_only_same_fresh_unpublished_leaf(self):
        from types import SimpleNamespace
        from unittest.mock import patch
        import stat,os
        directory=Path('/run/consolecrypt-agent/disk/'+'1'*32)
        uid=os.getuid() if hasattr(os,'getuid') else 10002
        folder=SimpleNamespace(st_mode=stat.S_IFDIR|0o700,st_uid=uid,st_dev=7,st_ino=8)
        leaf=SimpleNamespace(st_mode=stat.S_IFREG|0o600,st_uid=uid,st_dev=7,st_ino=9,st_nlink=1,st_size=0)
        for replacement in (False,True):
            named=SimpleNamespace(st_dev=7,st_ino=99) if replacement else leaf
            with patch.object(self.ns['os'],'getuid',return_value=uid,create=True),patch.object(Path,'lstat',return_value=folder), \
                 patch.object(self.ns['os'],'open',side_effect=[10,11]),patch.object(self.ns['os'],'fstat',side_effect=[folder,leaf]), \
                 patch.object(self.ns['os'],'stat',return_value=named),patch.object(self.ns['os'],'write',side_effect=OSError(5,'secret-write-error')), \
                 patch.object(self.ns['os'],'fchmod',create=True),patch.object(self.ns['os'],'close'),patch.object(self.ns['os'],'unlink') as unlink, \
                 patch.object(self.ns['os'],'O_DIRECTORY',0,create=True),patch.object(self.ns['os'],'O_NOFOLLOW',0,create=True),patch.object(self.ns['os'],'O_CLOEXEC',0,create=True):
                self.assertFalse(self.ns['_record_start_failure'](directory,'entry','binary',ValueError('disk_binary_changed')))
                if replacement:unlink.assert_not_called()
                else:unlink.assert_called_once_with('prometheus-exit.json',dir_fd=10)

    def test_no_diagnostic_producer_while_original_unit_or_child_may_still_write_exit(self):
        from unittest.mock import patch
        actual=self.ns['_failure_no_late_producer']
        done=dict(unitStopped=True,cgroupEmpty=True,leaderExited=True)
        self.assertTrue(actual(False,{}));self.assertTrue(actual(True,done))
        for key in done:
            self.assertFalse(actual(True,dict(done,**{key:False})))
        self.assertFalse(actual(True,dict(done,unknown=True)))
        self.assertFalse(actual(True,{}))
        previous=self.ns['_START_DIAGNOSTIC_ALLOWED'];self.ns['_START_DIAGNOSTIC_ALLOWED']=False
        try:
            with patch.dict(self.ns,{'_record_start_failure':unittest.mock.Mock()}) as current:
                self.ns['_entry_start_failure'](ValueError,ValueError('disk_binary_changed'),None)
                current['_record_start_failure'].assert_not_called()
        finally:self.ns['_START_DIAGNOSTIC_ALLOWED']=previous

    def test_actual_adapter_preserves_first_error_when_stop_also_fails(self):
        import ast,json,re,stat
        from types import SimpleNamespace
        from unittest.mock import Mock,patch
        source=ast.parse((COLLECTOR/'service_unit_adapter.py').read_bytes())
        run=next(n for n in source.body if isinstance(n,ast.FunctionDef) and n.name=='run')
        session='1'*32;raw=b'{}';digest=hashlib.sha256(raw).hexdigest()
        config=dict(session=session,unit='cc-agent-disk-'+session+'.service',ownerPid=7,ownerStartTicks=8,
            deadlineBoottimeNs=30_000_000_000,createdBoottimeNs=1_000_000_000,expiresUnixNs=40_000_000_000,
            binding={'original':{'collector_id':'unused'}},binarySha256='a'*64,statusPort=19109)
        class Selector:
            def __enter__(self):return self
            def __exit__(self,*args):pass
            def register(self,*args):pass
            def select(self,*args):return []
        class Unit:
            attempted=False;entry_stopped=False;entry_namespace_fd=adapter_namespace_fd=child_pidfd=pidfd=None
            cleanup_state={};retained_public_files={}
            def __init__(self,*args):pass
            def remember_public_file(self,name):self.retained_public_files[name]={'name':name,'dev':1,'ino':2,'mode':384}
            def close(self,end):raise RuntimeError('secret-stop-failure')
        first=ValueError('disk_manager_environment');record=Mock(return_value=True)
        fakeos=SimpleNamespace(fstat=lambda fd:SimpleNamespace(st_mode=stat.S_IFIFO,st_uid=10002,st_dev=1,st_ino=fd),
            getuid=lambda:10002,getpid=lambda:12,getppid=lambda:7,pidfd_open=lambda pid:9,close=lambda fd:None)
        ns=dict(re=re,ROOT=Path('/run/consolecrypt-agent/disk'),identity=lambda m:(1,2,3,4,5,6),
            regular=lambda *a,**k:(10,raw),os=fakeos,hashlib=hashlib,private_config=lambda *a:config,
            need=lambda value,code:None if value else (_ for _ in ()).throw(ValueError(code)),
            stat=stat,proc_start=lambda pid:8,time=SimpleNamespace(CLOCK_BOOTTIME=7,clock_gettime_ns=lambda *a:1_000_000_000,
            time_ns=lambda:0,monotonic=lambda:0),binding_types=lambda:(Mock(),Mock(return_value=Mock())),
            selectors=SimpleNamespace(DefaultSelector=Selector,EVENT_READ=1),ServiceUnitSpec=SimpleNamespace(from_private_handoff=lambda *a:Mock()),
            ServiceOwnedUnit=Unit,atomic_record=Mock(),command=Mock(side_effect=first),
            _record_start_failure=record,_failure_no_late_producer=self.ns['_failure_no_late_producer'])
        exec(compile(ast.Module(body=[run],type_ignores=[]),'actual-adapter-run','exec'),ns)
        with patch.object(Path,'lstat',return_value=Mock()):result=ns['run'](session,digest)
        self.assertEqual(result['errorCode'],'disk_manager_environment')
        self.assertIs(record.call_args.args[3],first)
        self.assertTrue(result['cleanup']['unknown'])
        self.assertNotIn('secret-stop-failure',str(result))

class CollectorManagerProfileAssets(unittest.TestCase):
    @staticmethod
    def profile():
        import ast
        from pathlib import PurePosixPath
        source = (COLLECTOR.parent / 'delivery/collector_provision.py').read_bytes()
        tree = ast.parse(source)
        names = {'HOME', 'XDG', 'ROOT', 'MOUNT', 'PROFILE', 'DISK'}
        nodes = [node for node in tree.body if isinstance(node, ast.Assign) and
                 len(node.targets) == 1 and isinstance(node.targets[0], ast.Name) and node.targets[0].id in names]
        functions = {'need', 'pairs', 'decode', 'encode', 'environment', 'files'}
        nodes += [node for node in tree.body if isinstance(node, ast.FunctionDef) and node.name in functions]
        import json
        scope = dict(Path=PurePosixPath, hashlib=hashlib, json=json)
        exec(compile(ast.Module(body=nodes, type_ignores=[]), 'actual-provision-profile', 'exec'), scope)
        return tree, scope

    def test_owned_manager_excludes_sgx_and_masks_only_private_ssh_socket(self):
        from pathlib import PurePosixPath
        _, scope = self.profile()
        private = PurePosixPath(scope['XDG'])
        mask = private / 'systemd/user/gpg-agent-ssh.socket'
        manager = PurePosixPath('/etc/systemd/system/user@10002.service.d/60-consolecrypt-agent.conf')
        for disk in (False, True):
            scope['DISK'] = disk
            assets = scope['files'](10002, 10002)
            self.assertEqual(set(assets), {manager, private / 'systemd/user.conf', mask,
                             PurePosixPath('/etc/systemd/system') / scope['MOUNT']})
            self.assertEqual(assets[mask], b'')
            self.assertEqual(assets[manager].count(b'UnsetEnvironment=SGX_AESM_ADDR\n'), 1)
            self.assertEqual(assets[manager].count(b'UnsetEnvironment='), 1)
            self.assertNotIn(PurePosixPath('/etc/systemd/user/gpg-agent-ssh.socket'), assets)
            self.assertEqual(set(scope['environment'](10002)), {'HOME','USER','LOGNAME','SHELL','PATH','LANG',
                             'XDG_RUNTIME_DIR','XDG_CONFIG_HOME','SYSTEMD_ENVIRONMENT_GENERATOR_PATH'})

    def test_four_asset_descriptor_preserves_fail_closed_stale_profile(self):
        import ast
        import copy
        from pathlib import PurePosixPath
        tree, scope = self.profile()
        scope.update(uid=10002, gid=10002, assets=scope['files'](10002, 10002))
        provision = next(node for node in tree.body if isinstance(node, ast.FunctionDef) and node.name == 'provision')
        descriptor = next(node for node in provision.body if isinstance(node, ast.Assign) and
                          any(isinstance(target, ast.Name) and target.id == 'descriptor' for target in node.targets))
        owner = next(node for node in provision.body if isinstance(node, ast.If) and node.body and
                     isinstance(node.body[0], ast.Assign) and any(isinstance(target, ast.Name) and target.id == 'record'
                     for target in node.body[0].targets))
        exec(compile(ast.Module(body=[copy.deepcopy(descriptor)], type_ignores=[]), 'actual-profile-descriptor', 'exec'), scope)
        current = scope['descriptor']
        mask = PurePosixPath(scope['XDG']) / 'systemd/user/gpg-agent-ssh.socket'
        self.assertEqual(current['assets'][str(mask)], hashlib.sha256(b'').hexdigest())
        old_assets = {str(path): hashlib.sha256(raw.replace(b'UnsetEnvironment=SGX_AESM_ADDR\n', b'')).hexdigest()
                      for path, raw in scope['assets'].items() if path != mask}
        stale = dict(current, assets=old_assets, state='ready')
        check = compile(ast.Module(body=copy.deepcopy(owner.body[:2]), type_ignores=[]), 'actual-profile-owner-check', 'exec')
        for rejected in (stale, dict(current, state='configured')):
            scope['previous'] = scope['encode'](rejected)
            with self.assertRaisesRegex(ValueError, '^collector_partial_state$'):
                exec(check, scope)
        scope['previous'] = scope['encode'](dict(current, state='ready'))
        exec(check, scope)


if __name__ == "__main__":
    unittest.main()
