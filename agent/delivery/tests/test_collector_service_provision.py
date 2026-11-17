"""Focused real-FS terminal-state checks; no systemd/worker/authority claim."""
import importlib.util,os,stat,tempfile,unittest
from pathlib import Path
from unittest.mock import patch

@unittest.skipUnless(os.environ.get('CC_SERVICE_DISABLE_CANDIDATE')and os.name=='posix'and os.geteuid()==0,
    'Explicit owned Root-filesystem fixture required; no host/service fallback')
class CollectorDisabledTerminal(unittest.TestCase):
    def setUp(self):
        self.tmp=tempfile.TemporaryDirectory(prefix='cc-service-disable-',dir='/work')
        self.root=Path(self.tmp.name)
        path=Path(os.environ.get('CC_SERVICE_DISABLE_CANDIDATE',str(Path(__file__).resolve().parents[1]/'collector_service_provision.py')))
        spec=importlib.util.spec_from_file_location('actual_disable_candidate',path)
        self.m=importlib.util.module_from_spec(spec);spec.loader.exec_module(self.m)
        self.m.UNIT=self.root/'consolecrypt-agent-collector.service'
        self.m.CATALOG=self.root/'catalog.json'
        self.m.UNIT.write_bytes(b'original-unit');self.m.UNIT.chmod(0o644)
        self.m.CATALOG.write_bytes(b'original-catalog');self.m.CATALOG.chmod(0o644)
        self.cg=self.root/'cgroup';self.cg.mkdir(mode=0o755)
        self.slice=self.cg/'system.slice';self.slice.mkdir(mode=0o755)
        self.unit=self.slice/'consolecrypt-agent-collector.service';self.unit.mkdir(mode=0o755)
        self.procs=self.unit/'cgroup.procs';self.procs.write_bytes(b'');self.procs.chmod(0o644)
        self.events=self.unit/'cgroup.events';self.events.write_bytes(b'populated 0\nfrozen 0\n');self.events.chmod(0o444)
        self.row=dict(ActiveState='failed',SubState='failed',MainPID='0',FragmentPath=str(self.m.UNIT),DropInPaths='',UnitFileState='disabled')
        self.calls=[]
        class Base:
            def show(inner,name,fields):
                self.calls.append((name,fields));return dict(self.row)
        self.base=Base()
        original=Path
        def fixed_path(value):
            return self.cg if value=='/sys/fs/cgroup'else original(value)
        self.paths=patch.object(self.m,'Path',fixed_path);self.paths.start()
    def tearDown(self):
        self.paths.stop();self.tmp.cleanup()
    def check(self):
        return self.m.disabled_terminal_state(self.base,b'original-unit',b'original-catalog')
    def test_inactive_dead_disabled_original_unit_accepted(self):
        self.row.update(ActiveState='inactive',SubState='dead')
        self.assertEqual(self.check(),'inactive');self.assertEqual(len(self.calls),1)
    def test_failed_original_empty_cgroup_accepted_without_state_reset(self):
        before=dict(self.row);self.assertEqual(self.check(),'failed')
        self.assertEqual(self.row,before);self.assertEqual(len(self.calls),2)
    def test_failed_original_absent_unit_cgroup_accepted(self):
        self.procs.unlink();self.events.unlink();self.unit.rmdir();self.assertEqual(self.check(),'failed')
    def test_failed_live_pid_enabled_dropin_foreign_or_unknown_state_denied(self):
        original=dict(self.row)
        for change in ({'MainPID':'2'},{'UnitFileState':'enabled'},{'UnitFileState':'unknown'},
                       {'FragmentPath':'/foreign.service'},{'DropInPaths':'/foreign.conf'},
                       {'ActiveState':'active','SubState':'running'},{'ActiveState':'failed','SubState':'dead'},
                       {'extra':'unknown'}):
            with self.subTest(change=change):
                self.row={**original,**change}
                with self.assertRaises(ValueError):self.check()
        self.row=original
    def test_nonempty_or_oversize_cgroup_procs_denied(self):
        for raw in (b'123\n',b'0\n',b' '*4097):
            with self.subTest(size=len(raw)):
                self.procs.write_bytes(raw)
                with self.assertRaises(ValueError):self.check()
    def test_linked_cgroup_procs_or_unit_directory_denied(self):
        self.procs.unlink();self.procs.symlink_to(self.m.CATALOG)
        with self.assertRaises((ValueError,OSError)):self.check()
        self.procs.unlink();self.events.unlink();self.unit.rmdir();self.unit.symlink_to(self.root,target_is_directory=True)
        with self.assertRaises((ValueError,OSError)):self.check()
    def test_group_writable_cgroup_or_changed_unit_catalog_denied(self):
        self.unit.chmod(0o775)
        with self.assertRaises(ValueError):self.check()
        self.unit.chmod(0o755);self.m.UNIT.write_bytes(b'foreign-unit')
        with self.assertRaises(ValueError):self.check()
        self.m.UNIT.write_bytes(b'original-unit');self.m.CATALOG.write_bytes(b'foreign-catalog')
        with self.assertRaises(ValueError):self.check()
    def test_empty_root_with_live_descendant_or_unknown_events_denied(self):
        for raw in (b'populated 1\nfrozen 0\n',b'populated 0\npopulated 0\n',b'populated 0\nfrozen 2\n',b'populated 0\nfrozen 0\nextra 0\n',b'x'*4097):
            with self.subTest(size=len(raw)):
                self.events.chmod(0o644);self.events.write_bytes(raw);self.events.chmod(0o444)
                with self.assertRaises(ValueError):self.check()
    def test_foreign_uid_fixed_image_directory_denied(self):
        foreign=Path('/home/runner');self.assertEqual(foreign.stat().st_uid,10001)
        self.cg=foreign
        with self.assertRaises((ValueError,PermissionError)):self.check()
    def test_failed_state_changed_during_quiescence_denied(self):
        first=dict(self.row);second={**first,'MainPID':'19'};count=[0]
        def show(name,fields):
            count[0]+=1;return dict(first if count[0]==1 else second)
        self.base.show=show
        with self.assertRaises(ValueError):self.check()

if __name__=='__main__':unittest.main(verbosity=2)
