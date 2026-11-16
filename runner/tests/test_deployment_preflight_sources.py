"""Actual verified source courier/parser; no service or authority operation."""
from pathlib import Path
import base64,hashlib,importlib.util,io,json,os,stat,sys,tempfile,unittest,zipfile
from unittest.mock import patch
DEPLOY=Path(__file__).resolve().parents[1]/'deployment'
def load(name,path):
    spec=importlib.util.spec_from_file_location(name,path);module=importlib.util.module_from_spec(spec)
    sys.modules[name]=module;spec.loader.exec_module(module);return module
def body(value):return json.dumps(value,sort_keys=True,separators=(',',':')).encode()
def sha(raw):return hashlib.sha256(raw).hexdigest()

class SourceCourier(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.bundle=load('cc_actual_deployment_bundle',DEPLOY/'bundle.py')
        cls.preflight=load('cc_actual_deployment_preflight',DEPLOY/'preflight.py')
    def setUp(self):
        self.temp=tempfile.TemporaryDirectory(prefix='cc-source-courier-test-');self.addCleanup(self.temp.cleanup)
        self.root=Path(self.temp.name)
        files={n:b'fixed public test metadata\n' for n in self.bundle.FIXED}
        files.update({'runner/consolecrypt_runner/__init__.py':b'', 'runner/consolecrypt_runner/probe.py':b'# public source only\n',
            'runner/consolecrypt_runner/sub/data.json':b'{"schema":1}\n'})
        for name,raw in files.items():
            p=self.root/name;p.parent.mkdir(parents=True,exist_ok=True);p.write_bytes(raw)
        self.records={n:{'bytes':len(raw),'sha256':sha(raw)}for n,raw in files.items()}
        self.metadata={'version':1,'kind':self.bundle.KIND,'nativeVersion':'0.1.0','sourceRevision':'a'*40,
            'cargoLockSha256':'b'*64,'runtimePinSha256':'c'*64,
            'target':{'system':'Linux','arch':'x86_64','libc':'glibc','minGlibc':'2.34','distribution':'ubuntu','distributionVersion':'24.04','minSystemd':255},
            'runtime':{'prefix':'/opt/cc-runtime','python':'3.12.3','environmentDigest':'d'*64,'helperDigest':'e'*64,
                'dependencyFilesDigest':'f'*64,'dependencyLock':'1'*64},'files':self.records}
        raw=body(self.metadata);(self.root/'bundle.json').write_bytes(raw);self.manifest=sha(raw)
    def payload(self):return self.bundle.preflight_sources(self.root,self.manifest)
    def test_failure_diagnostic_is_closed_and_never_discloses_exception_data(self):
        for phase,expected in [(12,'12'),(True,'00'),('private-untrusted-value','00'),(21,'00'),(1000,'00')]:
            stderr=io.StringIO()
            with patch.object(self.preflight,'_phase',phase),patch.object(self.preflight,'main',side_effect=OSError('SYNTHETIC_SECRET /private/path')),patch.object(sys,'stderr',stderr):
                self.assertEqual(self.preflight.entrypoint(),78)
            self.assertEqual(stderr.getvalue(),'service_host_or_runtime_unsupported:'+expected+'\n')
    def test_success_entrypoint_does_not_emit_failure_or_change_result(self):
        stderr=io.StringIO()
        with patch.object(self.preflight,'main',return_value=None),patch.object(sys,'stderr',stderr):
            self.assertEqual(self.preflight.entrypoint(),0)
        self.assertEqual(stderr.getvalue(),'')
    def check(self,value):
        raw=body(value);return self.preflight.checked_sources(raw,sha(raw),self.manifest)
    def test_actual_verified_producer_is_deterministic_and_preserves_every_source_byte(self):
        first=self.payload();self.assertEqual(first,self.payload())
        files=self.check(first)
        expected={n.removeprefix('runner/'):(self.root/n).read_bytes()for n in self.records if n.startswith('runner/')}
        self.assertEqual(files,expected);self.assertEqual(set(files),set(first['files']))
        self.assertEqual(first['manifestSha256'],self.manifest)
        self.assertFalse(any('cc-service-node'in name or 'key'in name for name in files))
    def test_changed_source_is_denied_by_original_real_bundle_verifier(self):
        (self.root/'runner/consolecrypt_runner/probe.py').write_bytes(b'changed')
        with self.assertRaises(ValueError):self.payload()
    def test_wrong_manifest_hash_is_denied_before_any_source_courier(self):
        with self.assertRaises(ValueError):self.bundle.preflight_sources(self.root,'0'*64)
    def test_transport_hash_and_original_manifest_are_both_required(self):
        value=self.payload();raw=body(value)
        for h,m in [('0'*64,self.manifest),(sha(raw),'0'*64)]:
            with self.assertRaises(ValueError):self.preflight.checked_sources(raw,h,m)
    def malicious(self,name,mode=stat.S_IFREG|0o444,compression=zipfile.ZIP_STORED,duplicate=False):
        value=self.payload();source=io.BytesIO();raw=b'public'
        with zipfile.ZipFile(source,'w',compression=compression)as archive:
            for member,contents in [('consolecrypt_runner/__init__.py',b''),('consolecrypt_runner/probe.py',b'# probe'),(name,raw)]:
                entry=zipfile.ZipInfo(member);entry.create_system=3;entry.external_attr=mode<<16;entry.compress_type=compression
                archive.writestr(entry,contents)
            if duplicate:archive.writestr(name,raw)
        blob=source.getvalue();value['archiveSha256']=sha(blob);value['archiveBase64']=base64.b64encode(blob).decode()
        value['files']={n:{'bytes':len(r),'sha256':sha(r)} for n,r in [('consolecrypt_runner/__init__.py',b''),('consolecrypt_runner/probe.py',b'# probe'),(name,raw)]}
        return value
    def test_traversal_absolute_or_foreign_package_never_materializes(self):
        for name in ['../escape.py','/consolecrypt_runner/evil.py','other/evil.py','consolecrypt_runner/../evil.py','consolecrypt_runner/x\\evil.py']:
            before=set(self.root.rglob('*'))
            with self.assertRaises(ValueError):self.check(self.malicious(name))
            self.assertEqual(before,set(self.root.rglob('*')))
    def test_symlink_modes_compression_or_duplicate_members_are_refusals(self):
        for mode,compression,duplicate in [(stat.S_IFLNK|0o444,zipfile.ZIP_STORED,False),
            (stat.S_IFREG|0o444,zipfile.ZIP_DEFLATED,False),(stat.S_IFREG|0o444,zipfile.ZIP_STORED,True)]:
            with self.assertRaises(ValueError):self.check(self.malicious('consolecrypt_runner/data.json',mode,compression,duplicate))
    def test_declared_unknown_or_changed_file_hash_cannot_pass_envelope_check(self):
        value=self.payload();value['files']['consolecrypt_runner/probe.py']['sha256']='0'*64
        with self.assertRaises(ValueError):self.check(value)
        value=self.payload();value['version']=True
        with self.assertRaises(ValueError):self.check(value)
    def full_archive(self,omit=None,corrupt=None,duplicate=False):
        output=io.BytesIO()
        with zipfile.ZipFile(output,'w',compression=zipfile.ZIP_STORED)as archive:
            for path in sorted(self.root.rglob('*')):
                if not path.is_file():continue
                name=path.relative_to(self.root).as_posix()
                if name==omit:continue
                entry=zipfile.ZipInfo(name);entry.create_system=3;entry.external_attr=(stat.S_IFREG|0o444)<<16
                archive.writestr(entry,b'corrupt'if name==corrupt else path.read_bytes())
            if duplicate:archive.writestr('bundle.json',(self.root/'bundle.json').read_bytes())
        return output.getvalue()
    def test_complete_courier_checks_original_manifest_and_all_native_source_bytes(self):
        with zipfile.ZipFile(io.BytesIO(self.full_archive()))as archive:
            self.assertEqual(self.bundle.check_courier(archive,self.manifest),self.metadata)
    def test_complete_courier_refuses_partial_changed_or_duplicate_members(self):
        for raw in [self.full_archive(omit='cc-service-node'),self.full_archive(corrupt='cc-service-node'),self.full_archive(duplicate=True)]:
            with zipfile.ZipFile(io.BytesIO(raw))as archive:
                with self.assertRaises(ValueError):self.bundle.check_courier(archive,self.manifest)
    def test_complete_courier_cannot_replace_original_manifest_digest(self):
        with zipfile.ZipFile(io.BytesIO(self.full_archive()))as archive:
            with self.assertRaises(ValueError):self.bundle.check_courier(archive,'0'*64)
    @unittest.skipUnless(sys.platform=='linux','actual held POSIX source FDs required')
    def test_complete_producer_streams_same_deterministic_verified_courier(self):
        first=io.BytesIO();second=io.BytesIO()
        self.bundle.write_courier(self.root,self.manifest,first);self.bundle.write_courier(self.root,self.manifest,second)
        self.assertEqual(first.getvalue(),second.getvalue())
        with zipfile.ZipFile(io.BytesIO(first.getvalue()))as archive:
            self.assertEqual(self.bundle.check_courier(archive,self.manifest),self.metadata)
    @unittest.skipUnless(sys.platform=='linux','actual POSIX readonly modes required')
    def test_materialization_uses_new_private_subtree_and_never_overwrites_it(self):
        destination=self.root/'owned';destination.mkdir(mode=0o700)
        files=self.check(self.payload());self.preflight.materialize_sources(destination,files)
        for name,raw in files.items():
            p=destination/'runner'/name;self.assertEqual(p.read_bytes(),raw);self.assertEqual(stat.S_IMODE(p.stat().st_mode),0o444)
        with self.assertRaises(FileExistsError):self.preflight.materialize_sources(destination,files)

if __name__=='__main__':unittest.main()
