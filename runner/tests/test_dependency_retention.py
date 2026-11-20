"""Owned Linux namespace/disposal tests; no user directories/network/keys."""
from concurrent.futures import ThreadPoolExecutor
import hashlib
import io
import json
import os
from pathlib import Path
import shutil
import select
import subprocess
import sys
import tempfile
import threading
import time
from types import SimpleNamespace
import unittest
from unittest.mock import patch
import uuid

from consolecrypt_runner import dependency_retention as retention
from consolecrypt_runner.dependency_helper import DependencyHelper, operation_path, serve_dependency
from consolecrypt_runner.errors import RunnerError
from consolecrypt_runner.protocol import encode


@unittest.skipUnless(sys.platform == 'linux' and os.getuid() != 0, 'Owned non-root Linux fixture')
class DependencyRetentionTests(unittest.TestCase):
    def setUp(self):
        temporary=tempfile.TemporaryDirectory(prefix='cc-owned-retention-')
        self.addCleanup(temporary.cleanup)
        self.root=Path(temporary.name)
        self.configured=self.root/'jobs';self.configured.mkdir(mode=0o700)
        (self.configured/'user-sentinel').write_bytes(b'unchanged user data')

    def helper(self, configured=None):
        helper=DependencyHelper(configured or self.configured,str(uuid.uuid4()))
        self.addCleanup(helper.close)
        return helper

    def dispose(self, helper, body=None):
        request={'version':1,'request_id':str(uuid.uuid4()),'operation':'dependency_dispose',
                 'body':body if body is not None else {'operation_id':helper.operation_id,'root_family':helper.root_family}}
        output=io.BytesIO()
        implicit=serve_dependency(helper,io.BytesIO(encode(request)+b'\n'),output)
        self.assertFalse(implicit)  # Explicit success/failure never implicit retry.
        return json.loads(output.getvalue())

    def test_four_retained_roots_bound_new_admission_without_touching_any(self):
        roots=[]
        for _ in range(4):
            helper=self.helper();path=operation_path(self.configured,helper.operation_id)
            (path/'data'/'retained-marker').write_bytes(b'crash evidence')
            roots.append(path);helper.close()
        with self.assertRaises(RunnerError) as error:self.helper()
        self.assertEqual(error.exception.code,'dependency_retention_limit')
        for path in roots:self.assertEqual((path/'data'/'retained-marker').read_bytes(),b'crash evidence')
        self.assertEqual((self.configured/'user-sentinel').read_bytes(),b'unchanged user data')

    def test_unknown_first_entry_denies_without_unbounded_search_or_stat(self):
        family=self.root/'family';family.mkdir(mode=0o700)
        fd=os.open(family,os.O_RDONLY|os.O_DIRECTORY)
        self.addCleanup(os.close,fd)
        class Scan:
            def __enter__(self):return self
            def __exit__(self,*args):pass
            def __iter__(self):return self
            def __next__(self):
                if getattr(self,'read',False):raise AssertionError('must not search beyond unknown entry')
                self.read=True
                return SimpleNamespace(name='unknown-junk',stat=lambda **kw:(_ for _ in ()).throw(AssertionError('must not stat unknown')))
        with patch.object(retention.os,'scandir',return_value=Scan()):
            with self.assertRaises(RunnerError) as error:retention.claim(fd,str(uuid.uuid4()))
        self.assertEqual(error.exception.code,'dependency_retention_unknown')

    def test_fourth_observed_entry_stops_before_fourth_stat_or_fifth_read(self):
        family=self.root/'family';family.mkdir(mode=0o700)
        fd=os.open(family,os.O_RDONLY|os.O_DIRECTORY);self.addCleanup(os.close,fd)
        info=os.fstat(fd)
        class Scan:
            reads=stats=0
            def __enter__(self):return self
            def __exit__(self,*args):pass
            def __iter__(self):return self
            def __next__(self):
                self.reads+=1
                if self.reads>4:raise AssertionError('unbounded entry scan')
                def stat(**kw):
                    self.stats+=1
                    if self.stats>3:raise AssertionError('fourth stat is unnecessary')
                    return info
                return SimpleNamespace(name=str(uuid.uuid4()),stat=stat)
        scan=Scan()
        with patch.object(retention.os,'scandir',return_value=scan):
            with self.assertRaises(RunnerError):retention.claim(fd,str(uuid.uuid4()))
        self.assertEqual((scan.reads,scan.stats),(4,3))

    def test_parallel_helpers_cannot_over_admit_four_namespace_roots(self):
        # Each constructor opens a different file description. Nonblocking
        # family flock serializes only count+exclusive creation, no waiter queue.
        def create(_):
            try:return DependencyHelper(self.configured,str(uuid.uuid4()))
            except (RunnerError,FileExistsError):return None
        with ThreadPoolExecutor(max_workers=8) as workers:helpers=list(workers.map(create,range(16)))
        admitted=[helper for helper in helpers if helper is not None]
        try:
            self.assertGreater(len(admitted),0);self.assertLessEqual(len(admitted),4)
            family=operation_path(self.configured,admitted[0].operation_id).parent
            self.assertEqual(len(list(family.iterdir())),len(admitted))
        finally:
            for helper in admitted:helper.close()

    def test_explicit_dispose_removes_current_only_and_releases_namespace_capacity(self):
        older=self.helper();old=operation_path(self.configured,older.operation_id)
        (old/'data'/'sentinel').write_bytes(b'older operation evidence');older.close()
        current=self.helper();own=operation_path(self.configured,current.operation_id)
        response=self.dispose(current)
        self.assertIsNone(response['error']);self.assertEqual(response['result'],{
            'version':1,'operation_id':current.operation_id,'data_disposed':True,'operation_removed':True})
        self.assertFalse(own.exists());self.assertEqual((old/'data'/'sentinel').read_bytes(),b'older operation evidence')
        current.close();self.helper().close()

    def test_wrong_identity_and_extra_path_fields_never_dispose_or_follow(self):
        for body in [{'operation_id':str(uuid.uuid4()),'root_family':'0'*64},
                     {'operation_id':'unused','root_family':'unused','path':'/must-never-follow'}]:
            helper=self.helper();path=operation_path(self.configured,helper.operation_id)
            response=self.dispose(helper,body)
            self.assertIsNotNone(response['error']);helper.close()
            self.assertTrue((path/'data'/'operation.json').is_file())
            self.assertFalse((path/retention.INTENT).exists())

    def test_unsupported_fd_safe_rmtree_fails_without_path_fallback(self):
        helper=self.helper();path=operation_path(self.configured,helper.operation_id)
        with patch.object(retention.shutil.rmtree,'avoids_symlink_attacks',False):
            response=self.dispose(helper)
        self.assertEqual(response['error']['code'],'dependency_cleanup_unsupported')
        self.assertTrue((path/'data'/'operation.json').exists());self.assertFalse((path/retention.INTENT).exists())

    def test_partial_cleanup_keeps_durable_intent_and_never_retries(self):
        helper=self.helper();path=operation_path(self.configured,helper.operation_id)
        with patch.object(retention.shutil,'rmtree',side_effect=OSError('synthetic deletion interruption')) as remove:
            response=self.dispose(helper)
        self.assertIsNotNone(response['error']);self.assertEqual(remove.call_count,1)
        self.assertEqual(json.loads((path/retention.INTENT).read_bytes())['operation_id'],helper.operation_id)
        helper.close(dispose=True)
        self.assertTrue((path/retention.INTENT).is_file());self.assertTrue((path/'data'/'operation.json').is_file())
        with self.assertRaises(FileExistsError):DependencyHelper(self.configured,helper.operation_id)

    def test_parent_rename_does_not_redirect_fd_bound_disposal(self):
        parent=self.root/'parent';parent.mkdir(mode=0o700)
        configured=parent/'jobs';configured.mkdir(mode=0o700)
        helper=self.helper(configured);old=operation_path(configured,helper.operation_id)
        moved=self.root/'moved';parent.rename(moved)
        outside=self.root/'outside';outside.mkdir(mode=0o700);(outside/'sentinel').write_bytes(b'unchanged')
        parent.symlink_to(outside,target_is_directory=True)
        result=self.dispose(helper);self.assertIsNone(result['error'])
        self.assertFalse((moved/old.relative_to(parent)).exists())
        self.assertEqual(list(outside.iterdir()),[outside/'sentinel']);self.assertEqual((outside/'sentinel').read_bytes(),b'unchanged')

    def test_operation_and_data_inode_replacement_fail_closed_without_outside_deletion(self):
        for data in [False,True]:
            helper=self.helper();path=operation_path(self.configured,helper.operation_id)
            target=path/'data' if data else path
            renamed=target.with_name(target.name+'-renamed');target.rename(renamed)
            outside=self.root/('outside-'+str(data));outside.mkdir(mode=0o700);(outside/'sentinel').write_bytes(b'unchanged')
            target.symlink_to(outside,target_is_directory=True)
            response=self.dispose(helper)
            self.assertEqual(response['error']['code'],'dependency_cleanup_identity')
            self.assertEqual((outside/'sentinel').read_bytes(),b'unchanged')
            self.assertEqual(list(outside.iterdir()),[outside/'sentinel'])
            helper.close()
            # Restore this exact synthetic private namespace for next variant.
            target.unlink();renamed.rename(target)

    def test_dispose_cancels_then_joins_worker_before_intent_or_deletion(self):
        helper=self.helper();path=operation_path(self.configured,helper.operation_id)
        entered=threading.Event();release=threading.Event();cancelled=threading.Event()
        original=helper.request
        def request(operation,body,cancel):
            if operation=='files':
                entered.set();self.assertTrue(cancel.wait(5));cancelled.set();self.assertTrue(release.wait(5))
                (path/'data'/'worker-ended').write_bytes(b'joined before delete')
                return {'done':True}
            return original(operation,body,cancel)
        requests=[{'version':1,'request_id':str(uuid.uuid4()),'operation':'files','body':{}},
            {'version':1,'request_id':str(uuid.uuid4()),'operation':'dependency_dispose',
             'body':{'operation_id':helper.operation_id,'root_family':helper.root_family}}]
        output=io.BytesIO();errors=[]
        def serve():
            try:serve_dependency(helper,io.BytesIO(b''.join(encode(r)+b'\n' for r in requests)),output)
            except BaseException as error:errors.append(error)
        with patch.object(helper,'request',side_effect=request):
            worker=threading.Thread(target=serve);worker.start()
            self.assertTrue(entered.wait(5));self.assertTrue(cancelled.wait(5))
            self.assertTrue(path.exists());self.assertFalse((path/retention.INTENT).exists())
            release.set();worker.join(10);self.assertFalse(worker.is_alive())
        self.assertFalse(errors);self.assertFalse(path.exists())
        self.assertEqual(len(output.getvalue().splitlines()),2)

    def child(self, operation):
        source=str(Path(__file__).resolve().parents[1]/'src')
        bootstrap="import sys,runpy;sys.path.insert(0,sys.argv.pop(1));runpy.run_module('consolecrypt_runner',run_name='__main__')"
        return subprocess.Popen([sys.executable,'-I','-B','-c',bootstrap,source,'--dependency-stdio',
            '--root',str(self.configured),'--operation-id',operation],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE)

    def test_actual_cli_normal_eof_disposes_but_killed_process_retains_exact_root(self):
        operation=str(uuid.uuid4());child=self.child(operation)
        output,error=child.communicate(timeout=15)
        self.assertEqual(child.returncode,0,(output,error));self.assertFalse(operation_path(self.configured,operation).exists())
        operation=str(uuid.uuid4());child=self.child(operation)
        try:
            child.stdin.write(encode({'version':1,'request_id':str(uuid.uuid4()),'operation':'probe','body':{}})+b'\n');child.stdin.flush()
            # Child is our fixed local module, bounded by the outer fixture deadline.
            raw=bytearray();deadline=time.monotonic()+10
            while not raw.endswith(b'\n'):
                self.assertTrue(select.select([child.stdout],[],[],max(0,deadline-time.monotonic()))[0])
                chunk=os.read(child.stdout.fileno(),4097-len(raw));self.assertTrue(chunk);raw.extend(chunk);self.assertLessEqual(len(raw),4096)
            response=json.loads(raw);self.assertEqual(response['result']['operation_id'],operation)
            child.kill();child.communicate(timeout=10)
            path=operation_path(self.configured,operation)
            self.assertTrue((path/'data'/'operation.json').is_file());self.assertFalse((path/retention.INTENT).exists())
            with self.assertRaises(FileExistsError):DependencyHelper(self.configured,operation)
        finally:
            if child.poll() is None:child.kill();child.communicate(timeout=10)

    def test_actual_kill_after_durable_intent_preserves_journal_without_reopen(self):
        operation=str(uuid.uuid4());source=str(Path(__file__).resolve().parents[1]/'src')
        entered=self.root/'crash-entered'
        code='''import sys,runpy,time
from pathlib import Path
from unittest.mock import patch
sys.path.insert(0,sys.argv.pop(1))
from consolecrypt_runner import dependency_retention as retention
original=retention.shutil.rmtree
entered=Path(sys.argv.pop(1))
def paused(path,*args,**kwargs):
 if path=='data' and kwargs.get('dir_fd') is not None:
  entered.write_text('durable intent reached',encoding='ascii')
  while True:time.sleep(.05)
 return original(path,*args,**kwargs)
sys.argv=['consolecrypt_runner',*sys.argv[1:]]
with patch.object(retention.shutil,'rmtree',side_effect=paused):
 runpy.run_module('consolecrypt_runner',run_name='__main__')
'''
        child=subprocess.Popen([sys.executable,'-I','-B','-c',code,source,str(entered),
            '--dependency-stdio','--root',str(self.configured),'--operation-id',operation],
            stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE)
        try:
            # EOF is the actual CLI normal-close path; pause only deletion.
            child.stdin.close();child.stdin=None
            deadline=time.monotonic()+10
            while not entered.exists():
                self.assertIsNone(child.poll());self.assertLess(time.monotonic(),deadline);time.sleep(.01)
            path=operation_path(self.configured,operation)
            self.assertEqual(json.loads((path/retention.INTENT).read_bytes())['operation_id'],operation)
            child.kill();child.communicate(timeout=10)
            self.assertTrue((path/'data'/'operation.json').is_file())
            with self.assertRaises(FileExistsError):DependencyHelper(self.configured,operation)
            self.assertTrue((path/retention.INTENT).is_file())
        finally:
            if child.poll() is None:child.kill();child.communicate(timeout=10)

    def test_final_unlink_gap_empty_uuid_still_counts_and_is_never_repaired(self):
        helper=self.helper();path=operation_path(self.configured,helper.operation_id)
        original=retention.os.rmdir
        def fail_final(name,*args,**kwargs):
            if name==helper.operation_id and kwargs.get('dir_fd')==helper._family_fd:
                raise OSError('synthetic final unlink interruption')
            return original(name,*args,**kwargs)
        with patch.object(retention.os,'rmdir',side_effect=fail_final) as fault:
            # Keep the real platform capability while injecting only its final syscall.
            with patch.object(retention.os,'supports_dir_fd',retention.os.supports_dir_fd|{fault}):
                response=self.dispose(helper)
        self.assertIsNotNone(response['error']);self.assertTrue(path.is_dir());self.assertEqual(list(path.iterdir()),[])
        helper.close(dispose=True)
        with self.assertRaises(FileExistsError):DependencyHelper(self.configured,helper.operation_id)
        self.assertEqual(list(path.iterdir()),[])


if __name__=='__main__':unittest.main(verbosity=2)
