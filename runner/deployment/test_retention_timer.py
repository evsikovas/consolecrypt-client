"""Automatic-path refusals/real fd lifetime, not simulated OS-positive proof."""
import copy,hashlib,io,json,os,runpy,tempfile,time,unittest,uuid
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch
import yaml
from consolecrypt_runner.errors import RunnerError
from consolecrypt_runner.protocol import encode,decode
from consolecrypt_runner.service import service_quiescence as guard
import test_results

DEPLOY=Path(__file__).resolve().parent

class TimerGuardTests(unittest.TestCase):
 def test_active_or_failed_unit_never_attaches_or_stops_in_automatic_producer(self):
  for state,sub in [('active','running'),('failed','failed'),('deactivating','stop')]:
   fd=os.open('/sys/fs/cgroup',os.O_RDONLY|os.O_DIRECTORY|os.O_CLOEXEC)
   values={'ActiveState':state,'SubState':sub,'MainPID':'123','ControlGroup':'/system.slice/original'}
   endpoint=lambda _:(_ for _ in ()).throw(AssertionError('automatic path must never attach'))
   stop=lambda _:(_ for _ in ()).throw(AssertionError('automatic path must never stop'))
   with patch.object(guard,'_cgroup_parent',return_value=fd),patch.object(guard,'_show',return_value=values):
    with self.assertRaises(RunnerError):guard._ObservedQuiescence(None,str(uuid.uuid4()),None,SimpleNamespace(public_identity=lambda:(1,2)),None,endpoint,stop,inactive_only=True)
   with self.assertRaises(OSError):os.fstat(fd)
 def test_inactive_property_alone_cannot_replace_actual_absent_cgroup(self):
  identity=str(uuid.uuid4());name='consolecrypt-service@'+identity+'.service'
  with tempfile.TemporaryDirectory()as path:
   (Path(path)/name).mkdir()
   fd=os.open(path,os.O_RDONLY|os.O_DIRECTORY|os.O_CLOEXEC)
   values={'ActiveState':'inactive','SubState':'dead','MainPID':'0','ControlGroup':''}
   with patch.object(guard,'_cgroup_parent',return_value=fd),patch.object(guard,'_show',return_value=values):
    with self.assertRaises(RunnerError):guard.require_inactive(identity,None)
   with self.assertRaises(OSError):os.fstat(fd)
 def test_fixed_per_instance_timer_and_hardened_service_cannot_start_node(self):
  timer=(DEPLOY/'consolecrypt-service-retention@.timer').read_text();unit=(DEPLOY/'consolecrypt-service-retention@.service').read_text()
  self.assertIn('OnCalendar=hourly\n',timer);self.assertIn('Persistent=true\n',timer)
  self.assertIn('Unit=consolecrypt-service-retention@%i.service\n',timer)
  self.assertIn('ExecStart=/opt/cc-runtime/bin/python -I -B /opt/consolecrypt/deployment/service_control.py --retire-raw-inactive %i\n',unit)
  for fixed in ['User=consolecrypt-service','Group=consolecrypt-service','NoNewPrivileges=yes','PrivateNetwork=yes','ProtectSystem=strict','ProtectControlGroups=yes','ProtectClock=yes','RestrictAddressFamilies=AF_UNIX','ReadWritePaths=/var/lib/consolecrypt-service/%i','CapabilityBoundingSet=','AmbientCapabilities=','TimeoutStartSec=240','KillMode=control-group']:
   self.assertIn(fixed+'\n',unit)
  for absent in ['cc-service-node','--identity-create','--identity-existing','ExecStop=','StateDirectory=','SuccessExitStatus=65']:self.assertNotIn(absent,unit)
  role=yaml.safe_load((DEPLOY/'service.yml').read_text())[0]
  block=next(t['block']for t in role['tasks']if t['name']=='Explicit operator maintenance')
  retain=next(t for t in block if t['name']=='Enable bounded inactive-only raw retention for this installed instance')
  self.assertTrue(retain['ansible.builtin.systemd_service']['enabled']);self.assertIn("'remove'",retain['when'])
  node=next(t for t in block if t['name']=='Start existing identity only on explicit start')
  self.assertEqual(node['when'],"service_operation == 'start'")

class AutomaticRegistrationTests(test_results.ResultsTests):
 # Do not discover/repeat the unchanged six parent tests under this subclass.
 def automatic_registration(self):
  path=self.keydir/'enrollment.v1';original=path.read_bytes();size=int.from_bytes(original[8:12],'little');record=decode(original[12:12+size])
  record['bootstrap']['owner'].update(owner_id=self.f.pin.owner_id,tenant=self.f.tenant)
  record['bootstrap']['origin']={'service_id':self.f.service_id}
  raw=encode(record);prefix=b'CCSE\0\0\0\1'+len(raw).to_bytes(4,'little')+raw
  path.write_bytes(prefix+hashlib.sha256(b'ConsoleCrypt Original Service Enrollment Commit v1\0'+prefix).digest())
 def test_automatic_uses_original_bootstrap_context_and_never_reads_caller_input(self):
  self.automatic_registration();before=self.snapshot();control=test_results.control
  class NoInput:
   def readline(self,*_):raise AssertionError('automatic path cannot read caller authority JSON')
  captured=[]
  def proof_required(*args,**kwargs):
   captured.append((args,kwargs));raise RunnerError('actual_os_proof_required','component is not OS proof')
  with (patch.object(control,'_RESULT_STATE',self.base),patch.object(control,'_result_software',return_value=(test_results.daemon,test_results.RUNNER/'src')),
       patch.object(control.sys,'stdin',SimpleNamespace(buffer=NoInput())),patch.object(guard,'require_inactive'),patch.object(guard,'retire_fixed',side_effect=proof_required)):
   with self.assertRaises(RunnerError):control.results(self.identity,retire_raw=True,inactive_only=True)
  self.assertEqual(len(captured),1);self.assertTrue(captured[0][1]['inactive_only']);self.assertEqual(before,self.snapshot())
 def test_missing_original_bootstrap_cannot_fall_back_to_policy_self_owner(self):
  before=self.snapshot();control=test_results.control
  with (patch.object(control,'_RESULT_STATE',self.base),patch.object(control,'_result_software',return_value=(test_results.daemon,test_results.RUNNER/'src')),
       patch.object(guard,'require_inactive'),patch.object(guard,'retire_fixed',side_effect=AssertionError('cannot reach raw proof'))):
   with self.assertRaises((KeyError,ValueError)):control.results(self.identity,retire_raw=True,inactive_only=True)
  self.assertEqual(before,self.snapshot())

# Parent class supplies the genuine signature/store setup only; keep ordinary
# ResultsTests in its own regression group, without duplicate inherited tests.
for _name in list(test_results.ResultsTests.__dict__):
 if _name.startswith('test_'):setattr(AutomaticRegistrationTests,_name,None)

if __name__=='__main__':unittest.main(verbosity=2)
