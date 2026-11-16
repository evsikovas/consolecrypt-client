"""Pure production delivery projections; not Linux UID/namespace acceptance."""
from pathlib import Path
import ast,hashlib,importlib.util,inspect,json,os,sys,tempfile,types,unittest,stat,zipfile
from unittest.mock import patch
BASE=Path(__file__).resolve().parents[1]

def load(name,leaf):
    path=BASE/'delivery'/leaf
    spec=importlib.util.spec_from_file_location(name,path)
    module=importlib.util.module_from_spec(spec);sys.modules[name]=module
    posix={n:types.SimpleNamespace() for n in ('fcntl','pwd','grp')} if sys.platform=='win32'else{}
    sys.path.insert(0,str(BASE/'delivery'))
    try:
        with patch.dict(sys.modules,posix):spec.loader.exec_module(module)
    finally:sys.path.pop(0)
    return module

class DeliveryProjection(unittest.TestCase):
    def test_rollback_restores_all_service_sources_and_removes_only_new_managed_disk_files(self):
        # Real stage A/B/rollback control flow; notice/signature boundaries are
        # inert substitutes here. No payload, installer or service is executed.
        module=load('cc_service_rollback_stage','stage.py')
        with tempfile.TemporaryDirectory(prefix='owned-service-rollback-')as tmp:
            base=Path(tmp);root=module.initialize_root(base/'private-stage')
            code=root/'opt/consolecrypt-agent'
            first={name:('# first '+name+'\n').encode()for name in module.SERVICE_FILES}
            first.update({'launch.py':b'# first inert launcher\n',
                'systemd/consolecrypt-agent.service':b'# inert unit\n'})
            updated={name:raw.replace(b'first',b'second')for name,raw in first.items()}
            disk={name:b'# inert disk source\n'for name in module.DISK_FILES[:-1]}
            elf=bytearray(64);elf[:6]=b'\x7fELF\x02\x01';elf[18:20]=(62).to_bytes(2,'little')
            disk[module.DISK_FILES[-1]]=bytes(elf)
            with patch.object(module,'validate_notice_inventory'):
                for version,files,executable in [('0.1.0',first,set()),
                        ('0.2.0',{**updated,**disk},{module.DISK_FILES[-1]})]:
                    artifact=base/(version+'.zip');module.build_package(files,artifact,version,executable)
                    module.stage_release(root,artifact,lambda _:True)
            for name in module.SERVICE_FILES:self.assertEqual((code/name).read_bytes(),updated[name])
            foreign=code/'foreign-user-file';foreign.write_bytes(b'preserve unmanaged file')
            identity=root/'var/lib/consolecrypt-agent/identity';identity.mkdir(parents=True)
            (identity/'private').write_bytes(b'preserve private state')
            selected=module.rollback(root)
            self.assertEqual(selected['version'],'0.1.0')
            managed=json.loads((code/'managed.json').read_bytes())
            for name in module.SERVICE_FILES:
                self.assertEqual((code/name).read_bytes(),first[name])
                self.assertEqual(managed['opt/consolecrypt-agent/'+name],hashlib.sha256(first[name]).hexdigest())
            for name in module.DISK_FILES:self.assertFalse((code/name).exists())
            self.assertEqual(foreign.read_bytes(),b'preserve unmanaged file')
            self.assertEqual((identity/'private').read_bytes(),b'preserve private state')
            self.assertEqual(json.loads((code/'current.json').read_bytes()),selected)

    def test_ram_runtime_policy_matches_original_producer_byte_goldens(self):
        module=load('cc_current_ram_policy','installed_runtime_policy.py')
        module.service_ids=lambda:(10001,10001)
        profile={'format':'cc-agent-installed-collector-profile/v1','uid':10001,'gid':10001}
        for name,value in [('ram-none.conf',None),('ram-egress.conf',{'format':'cc-agent-reviewed-egress/v1','relayIps':['192.0.2.1'],'dnsResolverIps':[]})]:
            self.assertEqual(module.render(value,profile),(BASE/'tests/fixtures/disk-profile'/name).read_bytes())
    def test_disk_policy_keeps_fixed_mount_and_original_egress_append_semantics(self):
        module=load('cc_current_disk_policy','installed_runtime_policy.py');module.service_ids=lambda:(10001,10001)
        profile={'format':'cc-agent-installed-disk-collector-profile/v1','uid':10001,'gid':10001}
        raw=module.render(None,profile)
        self.assertIn(b'Requires=run-consolecrypt\\x2dagent-disk.mount user@10001.service',raw)
        self.assertNotIn(b'volatile.mount',raw);self.assertNotIn(b'IPAddressDeny=',raw)
        for bad in (dict(profile,budget=72000),dict(profile,uid=True)):
            with self.assertRaises(ValueError):module.collector_profile(bad)
    def test_profile_refuses_caller_paths_budgets_or_unknown_modes(self):
        for extra in ({'path':'/tmp/adopt'},{'budget':1}):
            module=load('cc_current_disk_provision_extra','collector_provision.py')
            with self.assertRaises(ValueError):module.select_profile({'format':'cc-agent-installed-disk-collector-profile/v1','uid':10001,'gid':10001,**extra})
        module=load('cc_current_disk_provision_unknown','collector_provision.py')
        with self.assertRaises(ValueError):module.select_profile({'format':'unknown','uid':10001,'gid':10001})
    def test_profile_pins_probe_and_only_ephemeral_control_mount(self):
        module=load('cc_current_disk_provision','collector_provision.py')
        module.select_profile({'format':'cc-agent-installed-disk-collector-profile/v1','uid':10001,'gid':10001})
        self.assertEqual(module.ROOT.as_posix(),'/run/consolecrypt-agent/disk')
        self.assertEqual(module.PROBE_SHA256,hashlib.sha256((BASE/'delivery/collector_disk_namespace_probe.py').read_bytes()).hexdigest())
        mount=next(body for path,body in module.files(10001,10001).items()if str(path).endswith('.mount'))
        self.assertIn(b'Where=/run/consolecrypt-agent/disk\n',mount)
        self.assertIn(b'noswap,size=33554432',mount);self.assertNotIn(b'metrics-disk',mount)
    def test_signed_disk_binary_requires_all_sources_and_exact_modes(self):
        module=load('cc_current_disk_stage','stage.py')
        files={n:b'# fixed script\n' for n in module.DISK_FILES[:-1]};self.assertFalse(module._disk_files(files))
        binary=bytearray(64);binary[:6]=b'\x7fELF\x02\x01';binary[18:20]=(62).to_bytes(2,'little')
        files[module.DISK_FILES[-1]]=bytes(binary)
        modes={n:('0755'if n==module.DISK_FILES[-1]else'0644')for n in files}
        self.assertTrue(module._disk_files(files,modes))
        with self.assertRaises(module.StageError):module._disk_files({n:v for n,v in files.items()if n!=module.DISK_FILES[0]},modes)
        with self.assertRaises(module.StageError):module._disk_files(files,dict(modes,**{module.DISK_FILES[-1]:'0644'}))

class CollectorDirectoryOwnership(unittest.TestCase):
    @unittest.skipUnless(sys.platform=='linux'and hasattr(os,'geteuid')and os.geteuid()==0,
                         'requires real Linux root filesystem modes, not a projection mock')
    def test_umask077_new_directory_is0755_and_existing_wrong_mode_is_not_repaired(self):
        module=load('cc_actual_collector_directory','collector_provision.py')
        previous=os.umask(0o077)
        try:
            # Only a fresh disposable child; no existing directory is changed.
            with tempfile.TemporaryDirectory(prefix='cc-provision-dir-',dir='/run')as tmp:
                base=Path(tmp);fresh=base/'new'
                module.directory(fresh)
                info=fresh.lstat()
                self.assertEqual((info.st_uid,stat.S_IMODE(info.st_mode)),(0,0o755))
                module.directory(fresh)
                self.assertEqual(module.ident(fresh.lstat()),module.ident(info))
                wrong=base/'existing-private';wrong.mkdir(mode=0o700)
                original=module.ident(wrong.lstat())
                with self.assertRaises(ValueError):module.directory(wrong)
                self.assertEqual(module.ident(wrong.lstat()),original)
                self.assertEqual(stat.S_IMODE(wrong.stat().st_mode),0o700)
                # Execute the actual provision runtime-parent block, rather
                # than a mirrored mkdir. No service or namespace is started.
                tree=ast.parse(inspect.getsource(module.provision)).body[0].body
                begin=next(i for i,n in enumerate(tree)if isinstance(n,ast.Assign)and any(isinstance(v,ast.Name)and v.id=='parent'for v in n.targets)
                           and isinstance(n.value,ast.Attribute)and isinstance(n.value.value,ast.Name)and n.value.value.id=='ROOT'and n.value.attr=='parent')
                end=next(i for i,n in enumerate(tree)if i>begin and isinstance(n,ast.If)and isinstance(n.test,ast.UnaryOp)
                         and isinstance(n.test.operand,ast.Call)and isinstance(n.test.operand.func,ast.Attribute)
                         and isinstance(n.test.operand.func.value,ast.Name)and n.test.operand.func.value.id=='ROOT'and n.test.operand.func.attr=='exists')
                code=compile(ast.Module(body=tree[begin:end],type_ignores=[]),'actual-provision-runtime-parent','exec')
                environment=dict(module.__dict__,ROOT=base/'runtime-public/disk',uid=10002)
                exec(code,environment)
                runtime=environment['ROOT'].parent;info=runtime.lstat()
                print('CC_COLLECTOR_RUNTIME_PARENT_MODE '+json.dumps({'uid':info.st_uid,'mode':format(stat.S_IMODE(info.st_mode),'04o')}),flush=True)
                self.assertEqual((info.st_uid,stat.S_IMODE(info.st_mode)),(0,0o755))
                self.assertTrue(info.st_mode&stat.S_IXOTH)
                private=base/'runtime-existing';private.mkdir(mode=0o700)
                original=module.ident(private.lstat())
                environment=dict(module.__dict__,ROOT=private/'disk',uid=10002)
                with self.assertRaisesRegex(ValueError,'collector_runtime_parent'):exec(code,environment)
                self.assertEqual(module.ident(private.lstat()),original)
        finally:os.umask(previous)

class MeasuredPackageCaps(unittest.TestCase):
    def test_metadata_large_package_stages_without_dropping_files(self):
        module=load('cc_measured_package_stage','stage.py')
        with tempfile.TemporaryDirectory(prefix='owned-measured-package-')as tmp:
            base=Path(tmp);root=module.initialize_root(base/'private')
            files={'notices/text/'+hashlib.sha256(str(i).encode()).hexdigest()+'.txt':b'full inert test text'for i in range(200)}
            artifact=base/'package.zip';module.build_package(files,artifact)
            with zipfile.ZipFile(artifact)as archive:self.assertGreater(archive.getinfo('package.json').file_size,32768)
            result=module.stage_release(root,artifact,lambda _:True)
            record=json.loads((root/'opt/consolecrypt-agent/releases/0.1.0/installed.json').read_bytes())
            self.assertEqual(set(record['files']),set(files)|{'package.json'})
            self.assertEqual(len(record['files']),201)
            self.assertEqual(result['version'],'0.1.0')
            with self.assertRaises(module.StageError):module.build_package({str(i):b'x'for i in range(513)},base/'too-many.zip')

    def test_oversized_metadata_is_rejected_before_archive_member_read(self):
        module=load('cc_metadata_early_reject','stage.py')
        with tempfile.TemporaryDirectory(prefix='owned-metadata-bound-')as tmp:
            base=Path(tmp);root=module.initialize_root(base/'private');artifact=base/'oversized.zip'
            with zipfile.ZipFile(artifact,'w',compression=zipfile.ZIP_DEFLATED)as archive:
                info=zipfile.ZipInfo('package.json');info.external_attr=(stat.S_IFREG|0o644)<<16
                archive.writestr(info,b'x'*(module.MAX_METADATA+1))
            with patch.object(module.zipfile.ZipFile,'read',side_effect=AssertionError('must not allocate oversized metadata')):
                with self.assertRaisesRegex(module.StageError,'manifest budget'):
                    module.stage_release(root,artifact,lambda _:True)

    def test_disk_size_above_measured_bound_denies_before_binary_allocation(self):
        package=load('cc_disk_measured_bound','prototype_package.py')
        with tempfile.TemporaryDirectory(prefix='owned-disk-size-')as tmp:
            base=Path(tmp);binary=base/'prometheus';report=base/'report.json'
            with binary.open('wb')as stream:
                stream.write(b'\x7fELF\x02\x01'+b'\x00'*12+(62).to_bytes(2,'little'))
                stream.truncate(64*1024**2+1)
            with binary.open('rb')as stream:digest=hashlib.file_digest(stream,'sha256').hexdigest()
            value=dict(passed=True,normalProductBuild=True,sourceUnchanged=True,noDaemonStarted=True,
                       sourceMapSha256='0'*64,binaryBytes=binary.stat().st_size,binarySha256=digest)
            report.write_text(json.dumps(value))
            self.assertEqual(len(package.disk_binary_bytes(binary,report)),64*1024**2+1)
            with binary.open('r+b')as stream:stream.truncate(192*1024**2+1)
            with self.assertRaisesRegex(ValueError,'exceeds existing release budget'):package.disk_binary_bytes(binary,report)

if __name__=='__main__':unittest.main()
