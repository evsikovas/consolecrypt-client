"""Actual same-connection worker/transaction/close regressions, no service tasks."""
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path
import tempfile,threading,time,unittest,uuid
from consolecrypt_runner.service.scheduler import Store,Definition,Clock,SchedulerError

class AuditedLock:
    def __init__(self,original,attempted):self.original=original;self.attempted=attempted
    def __enter__(self):
        if threading.current_thread().name.startswith('second'):self.attempted.set()
        return self.original.__enter__()
    def __exit__(self,*args):return self.original.__exit__(*args)

class WorkerCustodyTests(unittest.TestCase):
    def test_actual_owner_created_connection_works_on_worker_and_preserves_cas_across_restart(self):
        with tempfile.TemporaryDirectory(prefix='owned-scheduler-worker-') as own:
            root=Path(own)/'scheduler';store=Store(root);identity=str(uuid.uuid4());epoch=str(uuid.uuid4())
            now=int(time.time());clock=Clock(now,time.monotonic_ns(),epoch);definition=Definition('Etc/UTC',12,0)
            try:
                self.assertEqual(store.schedules(),())
                with ThreadPoolExecutor(1) as worker:
                    self.assertEqual(worker.submit(store.save,identity,0,definition,clock).result(5),1)
                    self.assertEqual(worker.submit(store.schedules).result(5)[0].revision,1)
                    with self.assertRaises(SchedulerError):worker.submit(store.save,identity,0,definition,clock).result(5)
                self.assertEqual(store.schedules()[0].revision,1)
            finally:store.close()
            restarted=Store(root)
            try:
                with ThreadPoolExecutor(1) as worker:
                    values=worker.submit(restarted.schedules).result(5)
                    self.assertEqual(len(values),1);self.assertEqual(values[0].definition,definition);self.assertEqual(values[0].revision,1)
            finally:restarted.close()

    def test_actual_second_worker_waits_outside_complete_existing_transaction(self):
        with tempfile.TemporaryDirectory(prefix='owned-scheduler-serial-') as own:
            store=Store(Path(own)/'scheduler');entered=threading.Event();attempted=threading.Event();release=threading.Event();second_inside=threading.Event()
            store._mutex=AuditedLock(store._mutex,attempted)
            def first():
                with store.transaction():
                    self.assertTrue(store.db.in_transaction);entered.set()
                    self.assertTrue(release.wait(5));self.assertTrue(store.db.in_transaction)
                    self.assertEqual(store.db.execute('PRAGMA user_version').fetchone()[0],1)
            def second():
                with store.transaction():second_inside.set();self.assertTrue(store.db.in_transaction)
            try:
                with ThreadPoolExecutor(1,thread_name_prefix='first') as one,ThreadPoolExecutor(1,thread_name_prefix='second') as two:
                    a=one.submit(first)
                    try:
                        self.assertTrue(entered.wait(5));b=two.submit(second);self.assertTrue(attempted.wait(5))
                        self.assertFalse(second_inside.is_set())
                    finally:release.set()
                    a.result(5);b.result(5)
                self.assertTrue(second_inside.is_set());self.assertFalse(store.db.in_transaction)
            finally:release.set();store.close()

    def test_actual_close_waits_for_worker_commit_then_rejects_new_reads(self):
        with tempfile.TemporaryDirectory(prefix='owned-scheduler-close-') as own:
            store=Store(Path(own)/'scheduler');entered=threading.Event();attempted=threading.Event();release=threading.Event()
            store._mutex=AuditedLock(store._mutex,attempted)
            def first():
                with store.transaction():entered.set();self.assertTrue(release.wait(5));self.assertTrue(store.db.in_transaction)
            try:
                with ThreadPoolExecutor(1,thread_name_prefix='first') as one,ThreadPoolExecutor(1,thread_name_prefix='second') as two:
                    a=one.submit(first)
                    try:
                        self.assertTrue(entered.wait(5));b=two.submit(store.close);self.assertTrue(attempted.wait(5));self.assertFalse(b.done())
                    finally:release.set()
                    a.result(5);b.result(5)
                self.assertIsNone(store.db)
                with self.assertRaises(SchedulerError):store.schedules()
            finally:release.set();store.close()

if __name__=='__main__':unittest.main()
