"""Synthetic OWNED delivery causals, real existing Rust release verification.

The package's dummy ELF and fake supervisor are never run. No Linux service,
runtime version, actual join, legal inventory or production release is claimed.
"""
import hashlib,importlib.util,json,os,shutil,stat,subprocess,sys,tempfile,time,unittest
from unittest.mock import patch
from pathlib import Path
HERE=Path(__file__).resolve().parents[1]
BASE=HERE if(HERE/'prototype_package.py').exists()else next(p/'client-automation-monitoring/agent/delivery'for p in HERE.parents if(p/'client-automation-monitoring/agent/delivery/prototype_package.py').exists())
sys.path.insert(0,str(BASE));sys.path.insert(0,str(HERE))
spec=importlib.util.spec_from_file_location('fixed_linux_installer',HERE/'install_linux.py');m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m)
from package_wheel import wheel_file,PACKAGE
SUFFIX='.exe'if os.name=='nt'else''
VERIFIER=Path(os.environ.get('CC_AGENT_TEST_IDENTITY_BINARY',str(BASE.parent/('target/identity/debug/cc-agent-identity'+SUFFIX))))
SIGNER=Path(os.environ.get('CC_AGENT_TEST_RELEASE_SIGNER',str(BASE.parent/('target/identity/debug/examples/synthetic_release_fixture'+SUFFIX))))
VMAGENT_BINARY=os.environ.get('CC_AGENT_TEST_VMAGENT_BINARY')
VMAGENT_SHA='2039bc05b6ad5c96b34b72801908896bf8a6a7bcab8ae32120cbb8e6217ef6f6'
COLLECTOR_FILES=('collector/unit_spec.py','collector/volatile_unit_adapter.py','collector/volatile_unit_entry.py','collector/vmagent-prod')

class Supervisor:
    def __init__(self):self.calls=[];self.fail_stop=False;self.fail_start=0
    def stop_join(self):
        self.calls.append('stop_join')
        if self.fail_stop:raise RuntimeError('owned synthetic join not observed')
        return{'synthetic':True,'localWorkersJoined':True}
    def start(self):
        self.calls.append('start')
        if self.fail_start:self.fail_start-=1;raise RuntimeError('controlled start failure')
        return{'synthetic':True,'runtimeVersion':None}
    def disable(self):self.calls.append('disable')
    def reload(self):self.calls.append('reload')

@unittest.skipUnless(VERIFIER.is_file()and SIGNER.is_file(),'real maintained verifier and test-only signer are required; no crypto fallback')
class InstallTests(unittest.TestCase):
    def setUp(self):
        self.assertTrue(VMAGENT_BINARY,'CC_AGENT_TEST_VMAGENT_BINARY must name the exact maintained vmagent data file; no executable/hash fixture fallback')
        vmagent=Path(VMAGENT_BINARY);before=vmagent.lstat()
        self.assertTrue(stat.S_ISREG(before.st_mode)and not vmagent.is_symlink()and before.st_nlink==1)
        self.assertEqual(before.st_size,23209584)
        self.vmagent_bytes=vmagent.read_bytes();after=vmagent.lstat()
        self.assertEqual((before.st_dev,before.st_ino,before.st_size,before.st_mtime_ns,before.st_ctime_ns),
                         (after.st_dev,after.st_ino,after.st_size,after.st_mtime_ns,after.st_ctime_ns))
        self.assertEqual(hashlib.sha256(self.vmagent_bytes).hexdigest(),VMAGENT_SHA)
        self.tmp=tempfile.TemporaryDirectory(prefix='cc-signed-installer-');self.root=Path(self.tmp.name);self.paths=m.Paths(self.root)
        for p in(self.paths.prefix,self.paths.trust.parent,self.paths.artifact.parent):p.mkdir(parents=True)
        self.paths.unit.parent.mkdir(parents=True);self.paths.unit_template.parent.mkdir(parents=True)
        unit=Path(os.environ.get('CC_AGENT_TEST_INSTALLER_UNIT',str(HERE/'roles/consolecrypt_agent_install/files/consolecrypt-agent-installed.service')))
        shutil.copyfile(unit,self.paths.unit_template)
        self.paths.verifier=VERIFIER.resolve() # Existing immutable PE/ELF only; never copied or executed from package.
        self.supervisor=Supervisor()
        self.private=self.root/'var/lib/consolecrypt-agent/identity';self.private.mkdir(parents=True)
        (self.private/'key').write_bytes(b'owned identity unchanged')
        self.make(1,'0.1.0')
    def tearDown(self):self.tmp.cleanup()
    def make(self,sequence,version,incomplete=False,missing_collector=False,wrong_vmagent=False,wrong_runtime_version=False):
        stage=m.stage
        files={name:(BASE.parent/name).read_bytes()for name in m.SOURCES}
        if not wrong_runtime_version:
            from prototype_package import stamp_runtime_version,RUNTIME_VERSION_FILE
            files[RUNTIME_VERSION_FILE],_=stamp_runtime_version(files[RUNTIME_VERSION_FILE],version)
        files['launch.py']=(BASE/'launch.py').read_bytes();files['systemd/consolecrypt-agent.service']=(BASE/'consolecrypt-agent.service').read_bytes()
        header=bytearray(20);header[:6]=b'\x7fELF\x02\x01';header[18:20]=(62).to_bytes(2,'little')
        files['bin/cc-agent-identity']=bytes(header) # Explicitly inert synthetic metadata fixture.
        files.update(wheel_file(BASE.parent));files[m.INVENTORY]=json.dumps({'format':'cc-agent-notice-inventory/v1','components':[]}).encode()
        for name in COLLECTOR_FILES[:-1]:files[name]=(BASE.parent/name).read_bytes()
        files['collector/vmagent-prod']=self.vmagent_bytes # Actual maintained bytes as DATA, never run.
        if incomplete:files.pop(PACKAGE)
        if wrong_vmagent:files['collector/vmagent-prod']=bytes([self.vmagent_bytes[0]^1])+self.vmagent_bytes[1:]
        if missing_collector:
            for name in COLLECTOR_FILES:files.pop(name)
        executables={'bin/cc-agent-identity'}|({'collector/vmagent-prod'}if not missing_collector else set())
        stage.build_package(files,self.paths.artifact,version,executables)
        signed=self.root/('signature-'+str(sequence));signed.mkdir()
        run=subprocess.run([str(SIGNER.resolve()),'--out',str(signed.resolve()),'--artifact',str(self.paths.artifact.resolve()),'--now',str(int(time.time())),'--version',str(sequence),'--display-version',version,'--platform','linux-x86_64'],capture_output=True,timeout=10,check=True)
        self.assertTrue(json.loads(run.stdout)['testOnly'])
        shutil.copyfile(signed/'release-public-key.json',self.paths.key);shutil.copyfile(signed/'release-manifest.json',self.paths.manifest)
        self.paths.trust.write_text(json.dumps({'version':1,'verifierSha256':stage.digest(VERIFIER.read_bytes()),'publicKeySha256':stage.digest(self.paths.key.read_bytes()),'platform':'linux-x86_64'}))
    def act(self,operation,start=False):return m.transaction(self.paths,operation,start=start,supervisor=self.supervisor,uid=None)
    def preserved(self):self.assertEqual((self.private/'key').read_bytes(),b'owned identity unchanged')
    def test_real_signed_install_is_selection_without_invented_runtime_version(self):
        value=self.act('install');self.assertEqual(value['selectedRelease']['version'],'0.1.0');self.assertIsNone(value['actualRuntimeVersion'])
        self.assertEqual(self.supervisor.calls,['stop_join','reload']);self.preserved()
    def test_signed_runtime_version_mismatch_is_denied_before_original_service_stop(self):
        self.make(2,'0.2.0',wrong_runtime_version=True)
        with self.assertRaises(ValueError):self.act('install')
        self.assertEqual(self.supervisor.calls,[])
        self.assertFalse((self.paths.prefix/m.stage.MARKER).exists())
        self.preserved()
    def test_missing_release_trust_has_no_unsigned_or_synthetic_fallback(self):
        self.paths.trust.unlink()
        with self.assertRaises((ValueError,OSError)):self.act('install')
        self.assertEqual(self.supervisor.calls,[]);self.assertFalse((self.paths.prefix/m.stage.MARKER).exists());self.preserved()
    def test_tamper_is_denied_before_original_service_stop(self):
        with self.paths.artifact.open('ab')as stream:stream.write(b'tamper')
        with self.assertRaises(ValueError):self.act('install')
        self.assertEqual(self.supervisor.calls,[]);self.assertFalse((self.paths.prefix/m.stage.MARKER).exists());self.preserved()
    def test_signed_incomplete_package_is_refused_before_service_stop(self):
        self.make(2,'0.2.0',True)
        with self.assertRaises((ValueError,KeyError)):self.act('install')
        self.assertEqual(self.supervisor.calls,[]);self.assertFalse((self.paths.prefix/m.stage.MARKER).exists())
    def test_signed_package_without_fixed_collector_closure_is_refused_before_service_stop(self):
        self.make(2,'0.2.0',missing_collector=True)
        with self.assertRaises(ValueError):self.act('install')
        self.assertEqual(self.supervisor.calls,[]);self.assertFalse(self.paths.unit.exists());self.preserved()
    def test_signed_wrong_vmagent_bytes_are_refused_before_service_stop(self):
        self.make(2,'0.2.0',wrong_vmagent=True)
        with self.assertRaises(ValueError):self.act('install')
        self.assertEqual(self.supervisor.calls,[]);self.assertFalse(self.paths.unit.exists());self.preserved()
    def test_unjoined_original_owner_blocks_staging_after_real_signature_check(self):
        self.supervisor.fail_stop=True
        with self.assertRaises(RuntimeError):self.act('install')
        self.assertFalse((self.paths.prefix/m.stage.MARKER).exists());self.preserved()
    def test_failed_update_start_rolls_back_known_release_and_preserves_floor_and_state(self):
        self.act('install');self.make(2,'0.2.0');self.supervisor.fail_start=1
        value=self.act('update',True)
        self.assertEqual(value['phase'],'rolledBackAfterStartFailure');self.assertEqual(value['selectedRelease']['version'],'0.1.0')
        self.assertIsNone(value['actualRuntimeVersion']);self.assertEqual(json.loads((self.paths.prefix/'var/lib/consolecrypt-agent/release-anchor.json').read_bytes())['version'],2)
        self.assertEqual(self.supervisor.calls,['stop_join','reload','stop_join','reload','start','stop_join','start']);self.preserved()
    def test_old_release_replay_is_refused_before_stop_after_rollback(self):
        self.act('install');self.make(2,'0.2.0');self.act('update');self.act('rollback')
        before=list(self.supervisor.calls);self.make(3,'0.3.0')
        # Actual signer produces stale sequence one for a NEW artifact, no tuple fabrication.
        signed=self.root/'old-signature';signed.mkdir()
        subprocess.run([str(SIGNER.resolve()),'--out',str(signed.resolve()),'--artifact',str(self.paths.artifact.resolve()),'--now',str(int(time.time())),'--version','1','--display-version','0.3.0','--platform','linux-x86_64'],check=True,capture_output=True,timeout=10)
        shutil.copyfile(signed/'release-manifest.json',self.paths.manifest)
        with self.assertRaises(ValueError):self.act('update')
        self.assertEqual(self.supervisor.calls,before);self.preserved()
    def test_remove_preserves_identity_and_replay_anchor_instead_of_purging_data(self):
        self.act('install');floor=self.paths.prefix/'var/lib/consolecrypt-agent/release-anchor.json';original=floor.read_bytes()
        value=self.act('remove');self.assertEqual(value['phase'],'removed');self.assertTrue(value['identityHistoryAndReplayPreserved'])
        self.assertEqual(floor.read_bytes(),original);self.preserved();self.assertIsNone(self.act('status')['selectedRelease'])
    def test_retained_software_budget_refuses_new_release_before_stop_and_deletes_nothing(self):
        self.act('install');releases=self.paths.prefix/'opt/consolecrypt-agent/releases'
        for n in range(1,8):(releases/('0.1.'+str(n))).mkdir()
        self.make(2,'0.2.0');before=list(self.supervisor.calls)
        with self.assertRaises(ValueError):self.act('update')
        self.assertEqual(self.supervisor.calls,before);self.assertEqual(len(list(releases.iterdir())),8);self.preserved()
    def test_malformed_installed_relative_path_is_denied_before_any_path_stat(self):
        self.act('install');record=self.paths.prefix/'opt/consolecrypt-agent/releases/0.1.0/installed.json'
        value=json.loads(record.read_bytes());value['files']['../../outside']='0'*64;record.write_text(json.dumps(value))
        original=Path.stat
        def guarded(path,*args,**kw):
            if'..'in path.parts:raise AssertionError('unvalidated path was statted')
            return original(path,*args,**kw)
        with patch.object(Path,'stat',new=guarded):
            with self.assertRaises(m.stage.StageError):m.selected(self.paths,uid=None)
    def test_unsafe_lock_parent_refuses_before_open_or_transaction(self):
        self.paths.lock.parent.chmod(0o777)
        with patch.object(m,'Paths',return_value=self.paths),patch.object(m.sys,'platform','linux'),patch.object(m.sys,'argv',['installer','install','stage-only']),patch.object(m.os,'getuid',return_value=0,create=True),patch.object(m.os,'geteuid',return_value=0,create=True),patch.object(m.os,'open')as opened,patch.object(m,'transaction')as transaction:
            with self.assertRaises(ValueError):m.main()
            opened.assert_not_called();transaction.assert_not_called()
        self.assertEqual(self.supervisor.calls,[])
    def test_foreign_existing_unit_is_not_overwritten_or_stopped(self):
        self.paths.unit.write_bytes(b'foreign service preserve')
        with self.assertRaises(ValueError):self.act('install')
        self.assertEqual(self.paths.unit.read_bytes(),b'foreign service preserve');self.assertEqual(self.supervisor.calls,[])
    def test_unsigned_candidate_cannot_create_active_unit(self):
        with self.paths.artifact.open('ab')as stream:stream.write(b'tamper')
        with self.assertRaises(ValueError):self.act('install')
        self.assertFalse(self.paths.unit.exists());self.assertEqual(self.supervisor.calls,[])
    def test_exclusive_unit_publication_preserves_foreign_file_racing_after_preflight(self):
        original=os.link
        def racing(source,target,*args,**kw):
            Path(target).write_bytes(b'foreign racing service')
            return original(source,target,*args,**kw)
        with patch.object(m.os,'link',side_effect=racing):
            with self.assertRaises(ValueError):m.publish_unit(self.paths,uid=None)
        self.assertEqual(self.paths.unit.read_bytes(),b'foreign racing service');self.assertEqual(list(self.paths.unit.parent.glob('.cc-agent-unit-*')),[])
    def test_signed_candidate_swap_while_stop_is_held_does_not_bypass_original_completeness(self):
        self.act('install');self.make(2,'0.2.0')
        original_pointer=(self.paths.prefix/'opt/consolecrypt-agent/current.json').read_bytes()
        original_floor=(self.paths.prefix/'var/lib/consolecrypt-agent/release-anchor.json').read_bytes()
        original_unit=self.paths.unit.read_bytes();stop=self.supervisor.stop_join
        def swap():
            joined=stop();self.make(3,'0.3.0',incomplete=True);return joined
        self.supervisor.stop_join=swap
        with self.assertRaises(m.stage.StageError):self.act('update')
        self.assertEqual((self.paths.prefix/'opt/consolecrypt-agent/current.json').read_bytes(),original_pointer)
        self.assertEqual((self.paths.prefix/'var/lib/consolecrypt-agent/release-anchor.json').read_bytes(),original_floor)
        self.assertFalse((self.paths.prefix/'install-backup.json').exists())
        self.assertEqual(self.paths.unit.read_bytes(),original_unit);self.assertEqual(list(self.paths.artifact.parent.glob('.cc-agent-input-*')),[])
        self.preserved()
    def test_install_swap_while_stop_is_held_cannot_initialize_destination_marker(self):
        stop=self.supervisor.stop_join
        def swap():
            joined=stop();self.make(2,'0.2.0',incomplete=True);return joined
        self.supervisor.stop_join=swap
        with self.assertRaises(m.stage.StageError):self.act('install')
        self.assertFalse((self.paths.prefix/m.stage.MARKER).exists())
        self.assertEqual(list(self.paths.prefix.iterdir()),[]);self.assertFalse(self.paths.unit.exists())
        self.assertEqual(list(self.paths.artifact.parent.glob('.cc-agent-input-*')),[])

if __name__=='__main__':unittest.main()
