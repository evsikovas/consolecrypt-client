"""Real source capture/notice reads; no signing, compiler, network or legal claim."""
import copy
import hashlib
import importlib.util
from pathlib import Path
import tempfile
import unittest
from unittest import mock

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location('dependency_notice_candidate', HERE/'after/release_dependency_sources.notices.py')
d = importlib.util.module_from_spec(spec)
spec.loader.exec_module(d)


class DependencyNotices(unittest.TestCase):
    def setUp(self):
        self.owned = tempfile.TemporaryDirectory(prefix='cc-notice-source-')
        self.addCleanup(self.owned.cleanup)
        self.root = Path(self.owned.name)
        self.agent = self.root/'agent'
        (self.agent/'identity').mkdir(parents=True)
        (self.agent/'identity/Cargo.toml').write_text('[dependencies]\ncc-crypto-core={path="../../client/rust/crypto-core"}\ncc-agent-node-wire={path="../../client/rust/agent-node-wire"}\n')
        self.protocol = self.root/'protocol'
        for name in (*d.SELECTED, 'LICENSE'):
            path = self.path(name)
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(b'PUBLIC SYNTHETIC SOURCE\n')
        workspace = '[workspace.package]\nversion="0.1.0"\nlicense="AGPL-3.0-only"\n'
        for name in ('client/rust/Cargo.toml', 'crates/Cargo.toml', 'public-protocol/crates/Cargo.toml'):
            self.path(name).write_text(workspace)
        self.local = {'cc-crypto-core': 'client/rust/crypto-core', 'cc-agent-node-wire': 'client/rust/agent-node-wire', 'cc-models': 'crates/models'}
        for name, root in (*self.local.items(), ('cc-protocol', 'public-protocol/crates/protocol')):
            self.path(root+'/Cargo.toml').write_text('[package]\nname="'+name+'"\nversion.workspace=true\nlicense.workspace=true\n')
        self.metadata = {'packages': [{'name': name, 'source': None, 'manifest_path': str(self.path(root+'/Cargo.toml'))} for name, root in self.local.items()] + [
            {'name': 'cc-protocol', 'version': '0.1.0', 'source': d.GIT_SOURCE, 'manifest_path': str(self.protocol/'crates/protocol/Cargo.toml')}]}
        self.locked = [{'name': name, 'version': '0.1.0'} for name in self.local] + [
            {'name': 'cc-protocol', 'version': '0.1.0', 'source': d.GIT_SOURCE}]
        self.git = mock.patch.object(d, 'git', side_effect=self.tracked_git)
        self.git.start()
        self.addCleanup(self.git.stop)
        frame = d.capture(self.agent, self.metadata, self.read)
        self.descriptor = {'dependencySourceBefore': frame, 'dependencySourceAfter': copy.deepcopy(frame)}

    def path(self, name):
        return self.protocol/name.removeprefix('public-protocol/') if name.startswith('public-protocol/') else self.root/name

    def tracked_git(self, repo, *args):
        if args[0] == 'status': return b''
        if args[0] == 'ls-files': return ('\0'.join(n for n in d.SELECTED if not n.startswith('public-protocol/'))+'\0').encode()
        if args[0] == 'rev-parse': return (d.REV+'\n').encode()
        if args[0] == 'ls-tree':
            return b''.join(('100644 blob '+hashlib.sha1(self.path(n).read_bytes()).hexdigest()+'\t'+n.removeprefix('public-protocol/')+'\0').encode() for n in d.SELECTED if n.startswith('public-protocol/'))
        if args[0] == 'cat-file':
            return next(self.path(n).read_bytes() for n in d.SELECTED if n.startswith('public-protocol/') and hashlib.sha1(self.path(n).read_bytes()).hexdigest() == args[2])
        self.fail('unexpected public Git operation')

    def read(self, path, limit):
        raw = Path(path).read_bytes()
        self.assertLessEqual(len(raw), limit)
        return raw

    def run_notices(self):
        return d.notice_components(self.agent, self.descriptor, self.metadata, self.locked, self.read,
            lambda name, raw: {'original_path': name, 'sha256': hashlib.sha256(raw).hexdigest(), 'bytes': len(raw)})

    def test_full_exact_git_and_three_native_components_from_original_bytes(self):
        values = self.run_notices()
        self.assertEqual({p['name'] for p in values}, set(self.local)|{'cc-protocol'})
        protocol = next(p for p in values if p['name'] == 'cc-protocol')
        self.assertEqual(protocol['source'], d.GIT_SOURCE)
        self.assertEqual(protocol['notices'][0]['sha256'], hashlib.sha256(self.path('public-protocol/LICENSE').read_bytes()).hexdigest())
        self.assertIn('public-protocol/crates/protocol/src/lib.rs', protocol['source_file_sha256'])

    def test_other_git_package_refused_before_notice_emission(self):
        self.locked.append({'name': 'foreign', 'version': '0.1.0', 'source': 'git+https://example.invalid/x#'+d.REV})
        with self.assertRaisesRegex(ValueError, 'unreviewed package source'): self.run_notices()

    def test_revision_prefix_and_wrong_full_revision_are_not_equivalent(self):
        for rev in (d.REV[:7], '0'*40):
            self.locked[-1]['source'] = d.GIT_SOURCE.replace(d.REV, rev)
            with self.assertRaisesRegex(ValueError, 'unreviewed package source'): self.run_notices()

    def test_missing_tracked_license_cannot_borrow_project_license(self):
        self.path('public-protocol/LICENSE').unlink()
        with self.assertRaises(FileNotFoundError): self.run_notices()

    def test_changed_native_source_after_compiled_frame_denied(self):
        self.path('client/rust/crypto-core/src/lib.rs').write_bytes(b'CHANGED PUBLIC SOURCE\n')
        with self.assertRaisesRegex(ValueError, 'compiled dependency notice source changed'): self.run_notices()

    def test_unrecorded_protocol_license_denies(self):
        del self.descriptor['dependencySourceAfter']['files']['public-protocol/LICENSE']
        self.descriptor['dependencySourceBefore'] = copy.deepcopy(self.descriptor['dependencySourceAfter'])
        with self.assertRaisesRegex(ValueError, 'compiled dependency notice source changed'): self.run_notices()

    def test_empty_license_even_in_matching_source_frame_denies(self):
        self.path('public-protocol/LICENSE').write_bytes(b' \n')
        frame = d.capture(self.agent, self.metadata, self.read)
        self.descriptor = {'dependencySourceBefore': frame, 'dependencySourceAfter': copy.deepcopy(frame)}
        with self.assertRaisesRegex(ValueError, 'nonempty full dependency license required'): self.run_notices()

    def test_standalone_does_not_resolve_protocol_or_native_licenses(self):
        (self.agent/'identity/Cargo.toml').write_text('[dependencies]\n')
        with mock.patch.object(d, 'capture', side_effect=AssertionError('no discovery')):
            self.assertEqual(d.notice_components(self.agent, {}, None, [], None, None), [])
            with self.assertRaisesRegex(ValueError, 'compiled protocol dependency frame required'):
                d.notice_components(self.agent, {}, None, self.locked, None, None)


if __name__ == '__main__': unittest.main()
