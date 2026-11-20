"""Real parent channel concurrency, no fabricated Native success/authority."""
from concurrent.futures import ThreadPoolExecutor
import threading,unittest
from consolecrypt_runner.target_channel import TargetChannel
from consolecrypt_runner.diagnostic_mask import KnownJobSecrets,project_event
from consolecrypt_runner.errors import RunnerError
from test_diagnostic_mask import fixture

class DiagnosticAdmissionTests(unittest.TestCase):
    def exercise(self,diagnostic_first):
        verified=fixture().verify();host=verified.value['targets'][0]['host_id'];frames=[];changed=threading.Condition()
        def send(frame):
            with changed:frames.append(frame);changed.notify_all()
        channel=TargetChannel(send);mask=KnownJobSecrets({'extra_vars':{}})
        with ThreadPoolExecutor(max_workers=6) as pool:
            pending=[]
            def exec_call():return channel.call(verified,host,'exec',{},timeout=10)
            def diagnostic():return channel._redact_event(verified,'runner_on_ok',{},timeout=5)
            def wait(count):
                with changed:self.assertTrue(changed.wait_for(lambda:len(frames)>=count,2))
            try:
                for _ in range(4 if diagnostic_first else 5):pending.append(pool.submit(exec_call))
                wait(4 if diagnostic_first else 5)
                pending.append(pool.submit(diagnostic));wait(5 if diagnostic_first else 6)
                if diagnostic_first:pending.append(pool.submit(exec_call));wait(6)
                self.assertEqual(len(channel.pending),6)
                self.assertEqual(sum(p.diagnostic for p in channel.pending.values()),1)
                # Already saturated diagnostic slot immediately censors DATA;
                # neither the five executable requests nor peer are revoked.
                _,safe=project_event(channel,verified,'runner_on_ok',{'res':{'stdout':'sensitive'}},mask,cancel=threading.Event())
                self.assertEqual(safe,{'censored':True,'reason':'privacy_unavailable'})
                self.assertEqual(len(frames),6);self.assertFalse(channel.closed)
                self.assertFalse(any(f['type']=='target_revoke' for f in frames))
            finally:
                channel.close();mask.close()
                for future in pending:
                    with self.assertRaises(RunnerError):future.result(timeout=2)
        self.assertEqual(channel.pending,{})
    def test_four_exec_plus_mask_keeps_fifth_execution_slot(self):self.exercise(True)
    def test_five_exec_keep_separate_diagnostic_capacity(self):self.exercise(False)
