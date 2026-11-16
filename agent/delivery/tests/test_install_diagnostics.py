"""Stable closed installer failure metadata; no VM/service/crypto substitute or host IO."""
import ast
import contextlib
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import stat
import sys
import tempfile
import types
import unittest
from unittest import mock

DELIVERY=Path(__file__).resolve().parents[1]
# A sparse-stage host gate provides the exact current Main dependency root.
# Applied tests use the normal existing delivery package directly.
import os
DEPENDENCIES=Path(os.environ.get('CC_TEST_INSTALL_DELIVERY_ROOT',str(DELIVERY)))
sys.path.insert(0,str(DEPENDENCIES))
spec=importlib.util.spec_from_file_location('closed_install_diagnostics',DELIVERY/'install_linux.py')
install=importlib.util.module_from_spec(spec);spec.loader.exec_module(install)


class ClosedDiagnostics(unittest.TestCase):
    def setUp(self):
        self.temporary=tempfile.TemporaryDirectory(prefix='cc-closed-install-error-');self.addCleanup(self.temporary.cleanup)
        self.root=Path(self.temporary.name).resolve();self.paths=install.Paths(self.root)
        install._diagnostic('bootstrap')

    def report(self,callback):
        try:callback()
        except Exception as error:
            result=install._closed_failure(error)
            self.assertEqual(set(result),{'stage','errorClass'})
            self.assertIn(result['stage'],install._DIAGNOSTIC_STAGES)
            return result,error
        self.fail('original guard unexpectedly succeeded')

    def test_missing_bootstrap_retains_original_failure_and_reports_only_closed_stage(self):
        report,error=self.report(lambda:install.verifier(self.paths,uid=None))
        self.assertIsInstance(error,FileNotFoundError)
        self.assertEqual(report['stage'],'bootstrap')
        self.assertEqual(report['errorClass'],'missing_input')
        self.assertNotIn(str(self.root),json.dumps(report))

    def test_wrong_owner_and_link_metadata_still_deny_before_reading_or_process_use(self):
        path=self.root/'owned-public-input';path.write_bytes(b'SYNTHETIC_ONLY')
        report,error=self.report(lambda:install.read(path,100,uid=987654321))
        self.assertIsInstance(error,ValueError)
        self.assertEqual(report['stage'],'bootstrap')
        with mock.patch.object(Path,'lstat',return_value=types.SimpleNamespace(st_mode=stat.S_IFLNK,st_file_attributes=0)),mock.patch.object(install.os,'open')as opened:
            report,error=self.report(lambda:install.read(path,100,uid=None))
            self.assertIsInstance(error,install.stage.StageError)
            self.assertEqual(report['stage'],'bootstrap');opened.assert_not_called()

    def test_failed_existing_verifier_remains_denied_no_staging_and_closed_crypto_context_code(self):
        # Real ReleaseVerifier Python adapter; only its child runner is synthetic.
        # This checks negative propagation, not a Rust signature verification.
        for path,payload in((self.paths.verifier,b'SYNTHETIC NONEXECUTABLE'),
            (self.paths.key,b'{"format":1,"publicKey":"synthetic-public-only"}'),
            (self.paths.manifest,b'{"synthetic":"not-a-signature"}'),(self.paths.artifact,b'SYNTHETIC NOT A ZIP')):
            path.parent.mkdir(parents=True,exist_ok=True);path.write_bytes(payload)
        self.paths.trust.parent.mkdir(parents=True,exist_ok=True)
        self.paths.trust.write_text(json.dumps(dict(version=1,verifierSha256=hashlib.sha256(self.paths.verifier.read_bytes()).hexdigest(),
            publicKeySha256=hashlib.sha256(self.paths.key.read_bytes()).hexdigest(),platform='linux-x86_64')),encoding='utf-8')
        actual_class=install.ReleaseVerifier;calls=[]
        def make(*args,**kwargs):
            def run(argv,**options):calls.append((argv,options));return types.SimpleNamespace(returncode=65,stdout=b'',stderr=b'SYNTHETIC PRIVATE DETAIL MUST NOT ESCAPE')
            return actual_class(*args,**kwargs,run=run)
        with mock.patch.object(install,'ReleaseVerifier',side_effect=make):
            report,error=self.report(lambda:install.verifier(self.paths,uid=None))
        self.assertEqual(len(calls),1);self.assertIsInstance(error,ValueError)
        self.assertEqual(report['stage'],'signature_verify')
        self.assertEqual(report['errorClass'],'policy_denied')
        self.assertFalse(self.paths.prefix.exists())
        self.assertFalse(list(self.paths.artifact.parent.glob('.cc-agent-input-*')))
        self.assertNotIn('PRIVATE',json.dumps(report))

    def test_foreign_unit_intent_and_failed_original_join_are_distinct_without_relaxation(self):
        for path,payload in((self.paths.unit_template,b'KNOWN PUBLIC UNIT'),(self.paths.unit,b'FOREIGN SYNTHETIC UNIT')):
            path.parent.mkdir(parents=True,exist_ok=True);path.write_bytes(payload)
        before=self.paths.unit.read_bytes()
        report,error=self.report(lambda:install.unit_preflight(self.paths,uid=None))
        self.assertIsInstance(error,ValueError);self.assertEqual(report['stage'],'unit_identity')
        self.assertEqual(self.paths.unit.read_bytes(),before)
        # Synthetic already-checked manager identity reaches the original join
        # I/O boundary. No systemctl/cgroup access is made on the test host.
        manager=install.Systemd(paths=self.paths,configuration=types.SimpleNamespace(unchanged=lambda:None))
        with mock.patch.object(manager,'known',return_value=None),mock.patch.object(manager,'show',return_value={'LoadState':'not-found','MainPID':'0'}),mock.patch.object(install.os,'O_DIRECTORY',0,create=True),mock.patch.object(install.os,'O_NOFOLLOW',0,create=True),mock.patch.object(install.os,'O_CLOEXEC',0,create=True),mock.patch.object(install.os,'open',side_effect=OSError('SYNTHETIC UNKNOWN JOIN')):
            report,error=self.report(manager.stop_join)
        self.assertIsInstance(error,OSError);self.assertEqual(report['stage'],'stop_join')
        self.assertEqual(report['errorClass'],'io_unavailable')

    def test_actual_cli_handler_keeps_unknown_rc65_no_retry_and_never_serializes_error_values(self):
        tree=ast.parse((DELIVERY/'install_linux.py').read_text(encoding='utf-8'))
        main=next(n for n in tree.body if isinstance(n,ast.If)and isinstance(n.test,ast.Compare)and isinstance(n.test.left,ast.Name)and n.test.left.id=='__name__')
        handler=main.body[0].handlers[0]
        function=ast.FunctionDef(name='emit',args=ast.arguments(posonlyargs=[],args=[ast.arg(arg='error')],kwonlyargs=[],kw_defaults=[],defaults=[]),body=handler.body,decorator_list=[])
        class ExitProbe(Exception):pass
        def exit(code):self.assertEqual(code,65);raise ExitProbe()
        scope=dict(json=json,sys=types.SimpleNamespace(exit=exit),_closed_failure=install._closed_failure)
        exec(compile(ast.fix_missing_locations(ast.Module(body=[function],type_ignores=[])),'<actual-cli-handler>','exec'),scope)
        stream=io.StringIO()
        with contextlib.redirect_stdout(stream),self.assertRaises(ExitProbe):scope['emit'](ValueError('PRIVATE /path --argv secret never print'))
        value=json.loads(stream.getvalue())
        self.assertEqual(value['phase'],'unknown');self.assertIsNone(value['actualRuntimeVersion']);self.assertIs(value['automaticRetryAllowed'],False)
        self.assertEqual(value['reason'],'release_trust_or_transaction_unavailable')
        self.assertNotIn('PRIVATE',stream.getvalue());self.assertNotIn('/path',stream.getvalue());self.assertNotIn('--argv',stream.getvalue())


if __name__=='__main__':unittest.main()
