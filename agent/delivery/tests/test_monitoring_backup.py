"""Real fixed-mapping reads/refusals, plus optional maintained promtool check."""
import hashlib,importlib.util,json,os,sys,tempfile,unittest
from pathlib import Path
from unittest.mock import patch
HERE=Path(__file__).resolve().parents[1]
spec=importlib.util.spec_from_file_location('installed_config_backup',HERE/'monitoring_backup.py')
b=importlib.util.module_from_spec(spec);spec.loader.exec_module(b)
CANONICAL=HERE if(HERE/'monitoring_plan.py').is_file()else next(p/'client-automation-monitoring/agent/delivery'for p in HERE.parents if(p/'client-automation-monitoring/agent/delivery/monitoring_plan.py').is_file())
sys.path.insert(0,str(CANONICAL))
from monitoring_plan import MonitoringPlan

class BackupTests(unittest.TestCase):
    def setUp(self):
        self.tmp=tempfile.TemporaryDirectory(prefix='cc-installed-monitoring-backup-')
        self.root=Path(self.tmp.name);self.backup=self.root/'config-backup';self.backup.mkdir()
        self.config=MonitoringPlan().config()
        self.files={'prometheus.yml':json.dumps(self.config,indent=2).encode(),
                    'rules.yml':(CANONICAL/'rules.yml').read_bytes(),
                    'prometheus.service':(HERE/'prometheus.service.j2').read_bytes()}
        self.expected={'version':1,'config':self.config,'rulesSha256':b.digest(self.files['rules.yml']),'unitSha256':b.digest(self.files['prometheus.service'])}
        self.write_backup()
        self.live=self.root/'live';self.live.mkdir();(self.live/'prometheus.yml').write_bytes(b'ORIGINAL LIVE CONFIG')
        self.data=self.root/'data';self.data.mkdir();(self.data/'WAL').write_bytes(b'ORIGINAL METRIC WAL SENTINEL')
    def tearDown(self):self.tmp.cleanup()
    def write_backup(self):
        for name,raw in self.files.items():(self.backup/name).write_bytes(raw)
        (self.backup/'manifest.json').write_text(json.dumps({'version':1,'files':{n:b.digest(v)for n,v in self.files.items()}}))
    def preserved(self):
        self.assertEqual((self.live/'prometheus.yml').read_bytes(),b'ORIGINAL LIVE CONFIG')
        self.assertEqual((self.data/'WAL').read_bytes(),b'ORIGINAL METRIC WAL SENTINEL')
    def check(self):return b.verify(self.backup,self.expected,uid=None)
    def test_matching_reviewed_backup_validates_without_reading_or_mutating_metrics(self):
        original=b.read;seen=[]
        def observe(path,*args):seen.append(Path(path));return original(path,*args)
        with patch.object(b,'read',side_effect=observe),patch.object(b,'validate')as validated:
            result=self.check();validated.assert_called_once()
        self.assertTrue(result['backupVerified']);self.assertFalse(result['metricsReadOrChanged'])
        self.assertTrue(all(p.parent==self.backup for p in seen));self.preserved()
    def test_restore_refuses_corrupt_payload_before_promtool_and_live_writes(self):
        (self.backup/'rules.yml').write_bytes(b'operator changed backup')
        with patch.object(b,'validate')as validated:
            with self.assertRaises(ValueError):self.check()
            validated.assert_not_called()
        self.preserved()
    def test_saved_receiver_unit_cannot_override_explicit_reviewed_receiver_off(self):
        self.files['prometheus.service']+=b' --web.enable-remote-write-receiver\n';self.write_backup()
        with patch.object(b,'validate')as validated:
            with self.assertRaises(ValueError):self.check()
            validated.assert_not_called()
        self.preserved()
    def test_configuration_mismatch_even_with_valid_manifest_denies_before_tools(self):
        other=MonitoringPlan(alertmanager_enabled=True).config();self.files['prometheus.yml']=json.dumps(other).encode();self.write_backup()
        with patch.object(b,'validate')as validated:
            with self.assertRaises(ValueError):self.check()
            validated.assert_not_called()
        self.preserved()
    def test_missing_managed_configuration_can_be_checked_without_creating_data(self):
        (self.live/'prometheus.yml').unlink();self.data.rename(self.root/'original-data')
        with patch.object(b,'validate'):self.assertTrue(self.check()['backupVerified'])
        self.assertFalse((self.live/'prometheus.yml').exists());self.assertFalse(self.data.exists())
        self.assertEqual((self.root/'original-data/WAL').read_bytes(),b'ORIGINAL METRIC WAL SENTINEL')
    def test_incomplete_or_duplicate_manifest_is_refused_without_tool_calls(self):
        for raw in(b'{"version":1,"version":1,"files":{}}',b'{"version":1,"files":{}}'):
            (self.backup/'manifest.json').write_bytes(raw)
            with patch.object(b,'validate')as validated:
                with self.assertRaises(ValueError):self.check()
                validated.assert_not_called()
            self.preserved()
    def test_hardlinked_backup_cannot_alias_live_or_unrelated_file(self):
        original=self.backup/'rules.yml';original.unlink();outside=self.root/'outside';outside.write_bytes(self.files['rules.yml'])
        os.link(outside,original)
        with patch.object(b,'validate')as validated:
            with self.assertRaises(ValueError):self.check()
            validated.assert_not_called()
        self.assertEqual(outside.read_bytes(),self.files['rules.yml']);self.preserved()
    def test_real_promtool_validates_saved_rules_instead_of_missing_live_rules(self):
        platform='windows-amd64'if os.name=='nt'else'linux-amd64'
        assets=Path(os.environ.get('CC_AGENT_TEST_PROMTOOL_DIR',str(CANONICAL.parent/('collector/.cache/prometheus-'+platform))))
        tool=assets/('promtool.exe'if os.name=='nt'else'promtool')
        if not tool.is_file():self.skipTest('maintained pinned promtool not cached')
        import tarfile
        archive=assets/('prometheus-3.16.0.'+platform+'.tar.gz')
        required='9e46350ae75dfa766e64d4abbc00f45a327bb4198915b3fe4d74baa809a83223'if os.name=='nt'else'68727009472a2c02e1ff7f062fd5b3e637285bffc8fc2d340c76fb449d6e0f34'
        with archive.open('rb')as stream:self.assertEqual(hashlib.file_digest(stream,'sha256').hexdigest(),required)
        with tarfile.open(archive,'r:gz')as package:
            with package.extractfile('prometheus-3.16.0.'+platform+'/'+tool.name)as stream:expected=hashlib.file_digest(stream,'sha256').hexdigest()
        with tool.open('rb')as stream:self.assertEqual(hashlib.file_digest(stream,'sha256').hexdigest(),expected)
        self.assertTrue(b.verify(self.backup,self.expected,uid=None,tool=tool)['promtoolValidated']);self.preserved()
        self.files['rules.yml']=b'groups: wrong schema\n';self.expected['rulesSha256']=b.digest(self.files['rules.yml']);self.write_backup()
        with self.assertRaises(ValueError):b.verify(self.backup,self.expected,uid=None,tool=tool)
        self.preserved()


@unittest.skipUnless(os.name=='posix','requires actual Linux directory-FD publication')
class ConfigurationPublicationTests(unittest.TestCase):
    setUp=BackupTests.setUp
    tearDown=BackupTests.tearDown
    write_backup=BackupTests.write_backup
    preserved=BackupTests.preserved
    def live_mapping(self):
        return{name:self.live/name for name in b.NAMES}
    def fill_live(self):
        for name,raw in self.files.items():(self.live/name).write_bytes(raw)
    def test_create_then_restore_exact_configuration_and_private_backup_without_metric_io(self):
        self.fill_live();live=self.live_mapping()
        with patch.object(b,'validate'):
            result=b.create_backup(self.backup,self.expected,live=live,uid=None)
            self.assertTrue(result['backupCreated'])
            for name in b.NAMES:
                self.assertEqual((self.backup/name).stat().st_mode&0o777,0o600)
                (self.live/name).unlink()
            self.assertTrue(b.restore_backup(self.backup,self.expected,live=live,uid=None)['configurationRestored'])
        for name,raw in self.files.items():
            self.assertEqual((self.live/name).read_bytes(),raw)
            self.assertEqual((self.live/name).stat().st_mode&0o777,0o644)
        self.assertEqual((self.data/'WAL').read_bytes(),b'ORIGINAL METRIC WAL SENTINEL')
        self.assertEqual(sorted(p.name for p in self.backup.iterdir()),sorted([*b.NAMES,'manifest.json']))
    def test_create_wrong_review_denies_before_any_backup_write(self):
        self.fill_live();(self.live/'rules.yml').write_bytes(b'different live rule')
        original={p.name:p.read_bytes()for p in self.backup.iterdir()}
        with patch.object(b,'validate')as validated:
            with self.assertRaises(ValueError):b.create_backup(self.backup,self.expected,live=self.live_mapping(),uid=None)
            validated.assert_not_called()
        self.assertEqual(original,{p.name:p.read_bytes()for p in self.backup.iterdir()})
    def test_source_changed_by_validation_denies_before_backup_writes(self):
        self.fill_live();original={p.name:p.read_bytes()for p in self.backup.iterdir()}
        with patch.object(b,'validate',side_effect=lambda *args:(self.live/'rules.yml').write_bytes(b'changed during check')):
            with self.assertRaises(ValueError):b.create_backup(self.backup,self.expected,live=self.live_mapping(),uid=None)
        self.assertEqual(original,{p.name:p.read_bytes()for p in self.backup.iterdir()})
    def test_failed_promtool_leaves_all_previous_backup_bytes_intact(self):
        self.fill_live();original={p.name:p.read_bytes()for p in self.backup.iterdir()}
        with patch.object(b,'validate',side_effect=ValueError('invalid actual config')):
            with self.assertRaises(ValueError):b.create_backup(self.backup,self.expected,live=self.live_mapping(),uid=None)
        self.assertEqual(original,{p.name:p.read_bytes()for p in self.backup.iterdir()})
    def test_oversize_live_payload_denies_before_backup_write(self):
        self.fill_live();(self.live/'rules.yml').write_bytes(b'x'*(b.MAX_FILE+1))
        original={p.name:p.read_bytes()for p in self.backup.iterdir()}
        with patch.object(b,'validate')as validated:
            with self.assertRaises(ValueError):b.create_backup(self.backup,self.expected,live=self.live_mapping(),uid=None)
            validated.assert_not_called()
        self.assertEqual(original,{p.name:p.read_bytes()for p in self.backup.iterdir()})
    def test_destination_symlink_denies_before_any_restore_write(self):
        (self.live/'rules.yml').symlink_to(self.data/'WAL')
        with patch.object(b,'validate'):
            with self.assertRaises(ValueError):b.restore_backup(self.backup,self.expected,live=self.live_mapping(),uid=None)
        self.preserved();self.assertFalse((self.live/'prometheus.service').exists())
    def test_destination_hardlink_denies_before_any_restore_write(self):
        os.link(self.data/'WAL',self.live/'rules.yml')
        with patch.object(b,'validate'):
            with self.assertRaises(ValueError):b.restore_backup(self.backup,self.expected,live=self.live_mapping(),uid=None)
        self.preserved()
    def test_corrupt_backup_does_not_recreate_missing_live_configuration(self):
        (self.live/'prometheus.yml').unlink();(self.backup/'rules.yml').write_bytes(b'corrupt')
        with patch.object(b,'validate'):
            with self.assertRaises(ValueError):b.restore_backup(self.backup,self.expected,live=self.live_mapping(),uid=None)
        self.assertEqual(list(self.live.iterdir()),[])
    def test_existing_backup_symlink_cannot_overwrite_foreign_file(self):
        self.fill_live();(self.backup/'rules.yml').unlink();(self.backup/'rules.yml').symlink_to(self.data/'WAL')
        original=(self.backup/'prometheus.yml').read_bytes()
        with patch.object(b,'validate'):
            with self.assertRaises(ValueError):b.create_backup(self.backup,self.expected,live=self.live_mapping(),uid=None)
        self.assertEqual((self.backup/'prometheus.yml').read_bytes(),original)
        self.assertEqual((self.data/'WAL').read_bytes(),b'ORIGINAL METRIC WAL SENTINEL')
    def test_actual_promtool_create_and_restore_retain_original_bytes(self):
        assets=Path(os.environ.get('CC_AGENT_TEST_PROMTOOL_DIR','/missing-test-tool'))
        tool=assets/'promtool';archive=assets/'prometheus-3.16.0.linux-amd64.tar.gz'
        if not tool.is_file():self.skipTest('maintained pinned promtool not cached')
        import tarfile
        with archive.open('rb')as stream:self.assertEqual(hashlib.file_digest(stream,'sha256').hexdigest(),'68727009472a2c02e1ff7f062fd5b3e637285bffc8fc2d340c76fb449d6e0f34')
        with tarfile.open(archive,'r:gz')as package:
            with package.extractfile('prometheus-3.16.0.linux-amd64/promtool')as stream:expected=hashlib.file_digest(stream,'sha256').hexdigest()
        with tool.open('rb')as stream:self.assertEqual(hashlib.file_digest(stream,'sha256').hexdigest(),expected)
        self.fill_live()
        self.assertTrue(b.create_backup(self.backup,self.expected,live=self.live_mapping(),uid=None,tool=tool)['backupCreated'])
        (self.live/'prometheus.yml').write_bytes(b'corrupt live config')
        self.assertTrue(b.restore_backup(self.backup,self.expected,live=self.live_mapping(),uid=None,tool=tool)['configurationRestored'])
        for name,raw in self.files.items():self.assertEqual((self.live/name).read_bytes(),raw)
        self.assertEqual((self.data/'WAL').read_bytes(),b'ORIGINAL METRIC WAL SENTINEL')

if __name__=='__main__':unittest.main()
