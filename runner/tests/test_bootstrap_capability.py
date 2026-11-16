"""Compatibility only: a probe bit never supplies target/material authority."""
import hashlib,json,os,pwd,runpy,tempfile,unittest
from types import SimpleNamespace
from pathlib import Path
from unittest.mock import patch
from consolecrypt_runner.probe import probe
from consolecrypt_runner.protocol import encode

class BootstrapCapabilityTests(unittest.TestCase):
    def test_setup_selects_only_existing_platform_locks_from_actual_observers(self):
        setup=runpy.run_path(str(Path(__file__).resolve().parents[1]/'setup/environment.py'))
        key=setup['_environment_key']
        for system,machine,minor,expected in [('linux','x86_64',12,'linux-x86_64-py312'),
                ('linux','x86_64',14,'linux-x86_64-py314'),('linux','aarch64',12,'linux-aarch64-py312'),
                ('linux','aarch64',14,None),('linux','arm64',12,None),('darwin','arm64',12,None)]:
            with self.subTest(system=system,machine=machine,minor=minor),patch.dict(key.__globals__,
                    sys=SimpleNamespace(platform=system,version_info=SimpleNamespace(major=3,minor=minor)),
                    platform=SimpleNamespace(machine=lambda:machine)):
                self.assertEqual(key(),expected)
    def test_fixed_probe_explicitly_reports_compatibility_not_execution(self):
        report=probe()
        self.assertIs(report['capabilities']['trusted_worker_bootstrap_v1'],True)
        self.assertIs(report['capabilities']['execute'],False)
        self.assertEqual(report['protocol'],1)
        from consolecrypt_runner import VERSION
        self.assertEqual(VERSION,'0.1.0')
    def test_existing_scan_marks_old_package_outdated_without_install_or_version_bump(self):
        setup=runpy.run_path(str(Path(__file__).resolve().parents[1]/'setup/environment.py'))
        scan=setup['scan']
        home=Path(pwd.getpwuid(os.getuid()).pw_dir)
        with tempfile.TemporaryDirectory(prefix='owned-bootstrap-receipt-',dir=home) as raw:
            root=Path(raw);root.chmod(0o700);directory=root/'legacy';directory.mkdir(mode=0o700)
            paths={'python':str(directory/'venv/bin/python'),'module_dir':str(directory/'src'),'root':str(directory/'state')}
            for key,value in paths.items():
                p=Path(value)
                if key=='python':p.parent.mkdir(parents=True,mode=0o700);p.write_text('not executed\n')
                else:p.mkdir(mode=0o700)
            source=Path(__file__).resolve().parents[1]/'src/consolecrypt_runner/probe.py'
            fixed=source.read_bytes();self.assertIn(b'"trusted_worker_bootstrap_v1": True,',fixed)
            old=fixed.replace(b'"trusted_worker_bootstrap_v1": True,',b'')
            previous=hashlib.sha256(encode({'src/consolecrypt_runner/probe.py':old.decode()})).hexdigest()
            current=hashlib.sha256(encode({'src/consolecrypt_runner/probe.py':fixed.decode()})).hexdigest()
            receipt={'version':1,'paths':paths,'package_fingerprint':previous}
            (directory/'installation.json').write_text(json.dumps(receipt))
            # Existing scan code, own new receipt only; never execute fake python.
            with patch.dict(scan.__globals__,managed_root=lambda:root):
                report=scan(current)
                self.assertEqual(report['outdated_candidates'],[paths['python']])
                self.assertEqual(report['candidates'],[paths])
                self.assertEqual(scan(previous)['outdated_candidates'],[])
            self.assertEqual(json.loads((directory/'installation.json').read_text()),receipt)
