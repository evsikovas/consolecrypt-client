"""Causal stop refusals with synthetic OS observations; real systemd is VM-tested."""
import errno,importlib.util,io,os,stat,sys,time,unittest
from contextlib import ExitStack
from pathlib import Path,PurePosixPath
from types import SimpleNamespace
from unittest.mock import MagicMock,mock_open,patch

DEPLOY=Path(__file__).resolve().parent
sys.path.insert(0,str(DEPLOY.parent/'src'))
from consolecrypt_runner.service import service_quiescence as guard
from consolecrypt_runner.errors import RunnerError
spec=importlib.util.spec_from_file_location('stop_candidate',DEPLOY/'service_control.py')
control=importlib.util.module_from_spec(spec);spec.loader.exec_module(control)
IDENTITY='11111111-1111-4111-8111-111111111111'
UNIT='consolecrypt-service@'+IDENTITY+'.service'

class StopTests(unittest.TestCase):
 def setUp(self):
  self.stack=ExitStack();self.addCleanup(self.stack.close)
  def stub(target,name,*args,**kwargs):return self.stack.enter_context(patch.object(target,name,*args,**kwargs))
  self.values=dict(LoadState='loaded',ActiveState='deactivating',SubState='stop',MainPID='0',ControlPID='321',
   ExecMainPID='123',ExecMainCode='1',ExecMainStatus='0',Result='success',ControlGroup=guard.TEMPLATE_CGROUP+'/'+UNIT,
   Restart='no',KillMode='control-group',Delegate='no',FragmentPath=str(guard.UNIT),DropInPaths='')
  self.show=stub(guard,'_show',side_effect=lambda *_a,**_k:dict(self.values))
  stub(control,'_result_software',return_value=(None,Path('/original-bundle')))
  self.stack.enter_context(patch.object(sys,'path',list(sys.path)))
  for name,value in [('getuid',10001),('geteuid',10001),('getpid',321)]:stub(os,name,return_value=value,create=True)
  for name in ['O_DIRECTORY','O_NOFOLLOW','O_CLOEXEC','O_NONBLOCK']:stub(os,name,getattr(os,name,0),create=True)
  stub(guard,'_cgroup_parent',return_value=10)
  self.open=stub(os,'open',side_effect=[11,12])
  self.closed=stub(os,'close')
  def directory(inode):return SimpleNamespace(st_mode=stat.S_IFDIR|0o755,st_uid=0,st_dev=2,st_ino=inode)
  self.parent=directory(10);self.group=directory(11)
  self.file=SimpleNamespace(st_mode=stat.S_IFREG|0o644,st_uid=0,st_dev=2,st_ino=12)
  stub(os,'fstat',side_effect=lambda fd:{10:self.parent,11:self.group,12:self.file}[fd])
  self.named=stub(os,'stat',return_value=self.group)
  self.read=stub(os,'read',return_value=b'321\n')
  entries=MagicMock();entries.__enter__.return_value=[]
  self.scandir=stub(os,'scandir',return_value=entries)
  self.membership=mock_open(read_data=('0::'+self.values['ControlGroup']+'\n').encode())
  self.stack.enter_context(patch('builtins.open',self.membership))

 def test_completed_original_exit_is_only_success_in_actual_execstop_process(self):
  control._completed_stop(IDENTITY)
  self.assertEqual(self.show.call_count,2)
  self.assertEqual(self.open.call_args_list[0].args[0],UNIT)
  self.assertEqual(self.open.call_args_list[0].kwargs,{'dir_fd':10})
  self.assertEqual(self.open.call_args_list[1].args[0],'cgroup.procs')
  self.assertEqual([c.args[0] for c in self.closed.call_args_list],[10,12,11,10])

 def test_missing_socket_and_forged_environment_do_not_establish_completed_stop(self):
  for key,value in [('ActiveState','failed'),('ActiveState','active'),('SubState','stop-sigterm'),
    ('MainPID','123'),('ControlPID','999'),('ExecMainPID','0'),('ExecMainPID','321'),
    ('ExecMainCode','2'),('ExecMainStatus','65'),('Result','exit-code'),('ControlGroup','/system.slice/'+UNIT)]:
   with self.subTest(field=key,value=value),patch.dict(os.environ,MAINPID='0',SERVICE_RESULT='success',EXIT_CODE='exited',EXIT_STATUS='0'):
    previous=self.values[key];self.values[key]=value
    with self.assertRaises(ValueError):control._completed_stop(IDENTITY)
    self.values[key]=previous
  self.open.assert_not_called()

 def test_remaining_helper_or_descendant_group_denies_success(self):
  self.read.return_value=b'321\n456\n'
  with self.assertRaises(ValueError):control._completed_stop(IDENTITY)

 def test_descendant_group_denies_success(self):
  child=SimpleNamespace(is_dir=lambda **_:True)
  self.scandir.return_value.__enter__.return_value=[child]
  with self.assertRaises(ValueError):control._completed_stop(IDENTITY)
  self.read.assert_not_called()

 def test_other_kernel_membership_denies_success(self):
  self.membership.return_value.read.return_value=b'0::/system.slice/foreign.service\n'
  with self.assertRaises(ValueError):control._completed_stop(IDENTITY)
  self.open.assert_not_called()

 def test_replaced_cgroup_name_denies_success(self):
  self.named.return_value=SimpleNamespace(st_dev=2,st_ino=999)
  with self.assertRaises(ValueError):control._completed_stop(IDENTITY)

 def test_changed_authoritative_second_observation_denies_success(self):
  self.show.side_effect=[dict(self.values),{**self.values,'ExecMainPID':'124'}]
  with self.assertRaises(ValueError):control._completed_stop(IDENTITY)

 def test_only_missing_or_refused_endpoint_in_stop_mode_can_enter_fallback(self):
  for mode,error,allowed in [('--stop',FileNotFoundError(errno.ENOENT,'closed'),True),
    ('--stop',ConnectionRefusedError(errno.ECONNREFUSED,'closed'),True),
    ('--stop',PermissionError(errno.EACCES,'denied'),False),
    ('--stop',ValueError('foreign endpoint'),False),('',FileNotFoundError(errno.ENOENT,'closed'),False)]:
   with self.subTest(mode=mode,error=type(error).__name__),patch.object(control,'Endpoint',side_effect=error),\
     patch.object(control,'_completed_stop')as completed,patch.object(sys,'argv',['service_control.py',*([mode]if mode else[]),IDENTITY]):
    if allowed:control.main();completed.assert_called_once_with(IDENTITY)
    else:
     with self.assertRaises(type(error)):control.main()
     completed.assert_not_called()

class TemplateCgroupTests(unittest.TestCase):
 def test_parent_is_canonical_template_slice_with_existing_mount_and_root_guards(self):
  mounted=b'31 20 0:28 / /sys/fs/cgroup rw - cgroup2 cgroup rw\n'
  with patch.object(guard,'CGROUP',PurePosixPath('/sys/fs/cgroup')),patch('builtins.open',mock_open(read_data=mounted)),patch.object(guard,'_root_directory',return_value=42)as root:
   self.assertEqual(guard._cgroup_parent(),42)
   root.assert_called_once_with(PurePosixPath('/sys/fs/cgroup/system.slice/system-consolecrypt\\x2dservice.slice'))
  with patch.object(guard,'CGROUP',PurePosixPath('/sys/fs/cgroup')),patch('builtins.open',mock_open(read_data=mounted.replace(b'cgroup2',b'tmpfs'))),patch.object(guard,'_root_directory')as root:
   with self.assertRaises(RunnerError):guard._cgroup_parent()
   root.assert_not_called()

 def test_root_show_still_rejects_failed_unit_altering_installed_unit_or_dropins(self):
  values=dict(LoadState='loaded',ActiveState='failed',SubState='failed',MainPID='0',ControlGroup='',Restart='no',
   KillMode='control-group',Delegate='no',FragmentPath=str(guard.UNIT),DropInPaths='')
  result=SimpleNamespace(exit_code=0,truncated=False,cancelled=False,timed_out=False,output='')
  with patch.object(guard,'_root_file',return_value=b'exact original unit'),patch.object(guard,'run_bounded',return_value=result):
   result.output='\n'.join(k+'='+v for k,v in values.items())
   self.assertEqual(guard._show(UNIT,Path('/original-bundle'),time.monotonic()+5)['ActiveState'],'failed')
   values['DropInPaths']='/forged.conf';result.output='\n'.join(k+'='+v for k,v in values.items())
   with self.assertRaises(RunnerError):guard._show(UNIT,Path('/original-bundle'),time.monotonic()+5)

if __name__=='__main__':unittest.main(verbosity=2)
