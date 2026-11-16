"""Actual validation output is projected before report persistence.
Vault test is a parent-data projection test, not a password/sidecar proof.
"""
from pathlib import Path
import json,tempfile,unittest
from unittest.mock import patch
from consolecrypt_runner import validation
from consolecrypt_runner.snapshot import create_snapshot
from consolecrypt_runner.sandbox import ProcessResult
from consolecrypt_runner.validation_reports import write_report
from consolecrypt_runner.inventory import StaticInventory

class ValidationKnownMaskTests(unittest.TestCase):
    def test_real_parser_known_output_never_reaches_durable_validation_report(self):
        secret='owned_validation_password42';raw=[]
        with tempfile.TemporaryDirectory(prefix='cc-validation-mask-') as folder:
            root=Path(folder);project=root/'source';project.mkdir()
            (project/'playbook.yml').write_text('---\n- hosts: all\n  gather_facts: false\n  tasks:\n    - name: Missing module fixture\n      '+secret+': {}\n')
            snapshot=create_snapshot(project,root/'frozen')
            actual=validation.run_bounded
            def observe(*args,**kwargs):
                result=actual(*args,**kwargs);raw.append(result.output);return result
            with patch.object(validation,'run_bounded',observe):
                report=validation.validate_project(root/'frozen',snapshot,'playbook.yml',parameters={'extra_vars':{'password':secret},'tags':[],'skip_tags':[],'diff':False},inventory=StaticInventory(),targets=[])
            self.assertIn(secret,'\n'.join(raw),'prove actual parser emitted a known value before parent masking')
            self.assertFalse(report.valid)
            revision=root/'revision';revision.mkdir(mode=0o700)
            key=write_report(revision,report.to_json())
            serialized=(revision/'validations'/(key+'.json')).read_text()
            self.assertNotIn(secret,serialized);self.assertIn('[REDACTED]',serialized)
            self.assertTrue(any(check['exit_code']!=0 for check in report.checks))

    def test_vault_diagnostics_withheld_without_requesting_or_exporting_material(self):
        with tempfile.TemporaryDirectory(prefix='cc-validation-vault-mask-') as folder:
            root=Path(folder);source=root/'source';source.mkdir();(source/'playbook.yml').write_text('---\n- hosts: all\n  tasks: []\n')
            snapshot=create_snapshot(source,root/'frozen')
            # No real Vault grant is manufactured: direct parent DATA handling
            # with a preexisting fixed result, no broker/password/material call.
            from contextlib import nullcontext
            result=ProcessResult(1,'arbitrary-vault-material-in-untrusted-diagnostics',False,False,False,1)
            with patch.object(validation,'run_bounded',return_value=result):
                report=validation.validate_project(root/'frozen',snapshot,'playbook.yml',vault_sidecar_factory=lambda _:nullcontext(None))
            self.assertFalse(report.valid)
            self.assertEqual([check['output'] for check in report.checks],['[CENSORED]','[CENSORED]'])
            self.assertEqual([check['exit_code'] for check in report.checks],[1,1])

    def test_partial_output_flags_censor_known_prefix_before_report_write(self):
        secret='owned-known-long-password42'
        for truncated,cancelled,timed_out in ((True,False,False),(False,True,False),(False,False,True)):
            with self.subTest(truncated=truncated,cancelled=cancelled,timed_out=timed_out),tempfile.TemporaryDirectory(prefix='cc-validation-prefix-mask-') as folder:
                root=Path(folder);source=root/'source';source.mkdir();(source/'playbook.yml').write_text('---\n- hosts: all\n  tasks: []\n')
                snapshot=create_snapshot(source,root/'frozen')
                # Existing ProcessResult semantics: capped/cancelled output has
                # no textual marker and may end in a partial known value.
                result=ProcessResult(0,'parser: owned-known-long-pass',truncated,cancelled,timed_out,1)
                with patch.object(validation,'run_bounded',return_value=result):
                    report=validation.validate_project(root/'frozen',snapshot,'playbook.yml',parameters={'extra_vars':{'password':secret},'tags':[],'skip_tags':[],'diff':False},inventory=StaticInventory(),targets=[])
                self.assertFalse(report.valid)
                self.assertTrue(all(check['truncated']==truncated and check['cancelled']==cancelled and check['timed_out']==timed_out and check['exit_code']==0 for check in report.checks))
                revision=root/'revision';revision.mkdir(mode=0o700);key=write_report(revision,report.to_json())
                serialized=(revision/'validations'/(key+'.json')).read_text()
                self.assertNotIn('owned-known-long-pass',serialized)
                self.assertTrue(all(check['output']=='[CENSORED]' for check in report.checks))
