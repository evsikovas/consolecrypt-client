"""Dirty/omitted/foreign public-source denials and actual archive byte custody."""
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import sys
import tarfile
import tempfile
import unittest
from unittest import mock

HERE=Path(__file__).resolve().parent
STAGED=(HERE/'after/release_pipeline.provenance.py').is_file()
DELIVERY=HERE/'after' if STAGED else HERE.parent
PIPELINE=DELIVERY/('release_pipeline.provenance.py'if STAGED else'release_pipeline.py')
sys.path.insert(0,str(DELIVERY.resolve()))
if STAGED:sys.path.append(str((HERE.parents[1]/'client-automation-monitoring/agent/delivery').resolve()))
import release_dependency_sources as d
spec=importlib.util.spec_from_file_location('coherent_optional_and_provenance',PIPELINE)
p=importlib.util.module_from_spec(spec);spec.loader.exec_module(p)


class DependencyProvenance(unittest.TestCase):
    def agent(self,root,native=True):
        agent=root/'agent';(agent/'identity').mkdir(parents=True)
        text='[dependencies]\n'
        if native:text+='cc-crypto-core = { path = "../../client/rust/crypto-core" }\ncc-agent-node-wire = { path = "../../client/rust/agent-node-wire" }\n'
        (agent/'identity/Cargo.toml').write_text(text)
        return agent

    def test_actual_source_map_requests_native_dirty_scope_before_any_capture(self):
        with tempfile.TemporaryDirectory(prefix='cc-native-dirty-')as temporary:
            agent=self.agent(Path(temporary));seen=[]
            def status(argv):
                seen.append(argv)
                return b' M client/rust/crypto-core/src/lib.rs\n' if 'client/rust/crypto-core' in argv else b''
            with mock.patch.object(p,'AGENT',agent),mock.patch.object(p.subprocess,'check_output',side_effect=status):
                with self.assertRaisesRegex(ValueError,'committed exact Agent source'):
                    p.source_map()
            self.assertEqual(len(seen),1)
            self.assertIn('crates/models',seen[0]);self.assertIn('client/rust/Cargo.lock',seen[0])

    def test_omitted_native_file_denies_before_metadata_or_file_reads(self):
        with tempfile.TemporaryDirectory(prefix='cc-native-omitted-')as temporary:
            agent=self.agent(Path(temporary));names=[n for n in d.SELECTED if not n.startswith('public-protocol/')]
            def git(repo,*args):
                return b'' if args[0]=='status' else ('\0'.join(names[:-1])+'\0').encode()
            read=mock.Mock(side_effect=AssertionError('must refuse omitted set before reads'))
            with mock.patch.object(d,'git',side_effect=git):
                with self.assertRaisesRegex(ValueError,'omitted or unreviewed'):
                    d.capture(agent,{},read)
            read.assert_not_called()

    def test_foreign_actual_protocol_revision_is_not_relabelled(self):
        with tempfile.TemporaryDirectory(prefix='cc-protocol-foreign-')as temporary:
            root=Path(temporary);agent=self.agent(root);names=[n for n in d.SELECTED if not n.startswith('public-protocol/')]
            metadata={'packages':[{'name':name,'source':None,'manifest_path':str(root/path)}for name,path in (
                ('cc-crypto-core','client/rust/crypto-core/Cargo.toml'),('cc-agent-node-wire','client/rust/agent-node-wire/Cargo.toml'),('cc-models','crates/models/Cargo.toml'))]+
                [{'name':'cc-protocol','version':'0.1.0','source':d.GIT_SOURCE.replace(d.REV,'0'*40),'manifest_path':'/unused'}]}
            with mock.patch.object(d,'git',side_effect=lambda repo,*a:b'' if a[0]=='status'else('\0'.join(names)+'\0').encode()):
                with self.assertRaisesRegex(ValueError,'original exact protocol revision'):
                    d.capture(agent,metadata,lambda *a:(_ for _ in()).throw(AssertionError('no body read')))

    def test_legacy_standalone_does_not_borrow_uncompiled_native_source(self):
        with tempfile.TemporaryDirectory(prefix='cc-legacy-source-')as temporary:
            agent=self.agent(Path(temporary),False)
            with mock.patch.object(d,'git',side_effect=AssertionError('no dependency discovery')):
                self.assertEqual(d.capture(agent,None,None)['files'],{})
                self.assertEqual(d.dirty_paths(agent),())

    def test_partial_or_redirected_native_dependency_is_not_accepted(self):
        with tempfile.TemporaryDirectory(prefix='cc-native-path-')as temporary:
            agent=self.agent(Path(temporary));path=agent/'identity/Cargo.toml';raw=path.read_text()
            path.write_text(raw.replace('../../client/rust/crypto-core','../../other'))
            with self.assertRaisesRegex(ValueError,'fixed Native dependency path'):d.enabled(agent)
            path.write_text('[dependencies]\ncc-crypto-core = {path="../../client/rust/crypto-core"}\n')
            with self.assertRaisesRegex(ValueError,'both original Native'):d.enabled(agent)

    def test_real_tar_keeps_agent_and_binds_native_protocol_license_bytes(self):
        # Capture/actual-Cargo binding is tested independently above. This test
        # exercises the REAL append operation, preventing dropped dependency
        # bodies or a read-from-append-mode regression without any Git repo.
        with tempfile.TemporaryDirectory(prefix='cc-source-archive-')as temporary:
            root=Path(temporary);agent=self.agent(root);delivery=agent/'delivery';delivery.mkdir();work=root/'work';work.mkdir()
            protocol=root/'protocol';(protocol/'crates/protocol').mkdir(parents=True)
            native=root/'client/rust/crypto-core/src/lib.rs';native.parent.mkdir(parents=True);native.write_bytes(b'PUBLIC NATIVE SOURCE\n')
            proto=protocol/'crates/protocol/src/lib.rs';proto.parent.mkdir();proto.write_bytes(b'PUBLIC PROTOCOL SOURCE\n')
            (root/'LICENSE').write_bytes(b'PUBLIC PROJECT LICENSE\n')
            files={'LICENSE':root/'LICENSE','client/rust/crypto-core/src/lib.rs':native,'public-protocol/crates/protocol/src/lib.rs':proto}
            frame={'format':'synthetic byte-custody only','files':{n:{'sha256':hashlib.sha256(q.read_bytes()).hexdigest(),'bytes':q.stat().st_size}for n,q in files.items()}}
            (work/'build-public.json').write_text(json.dumps({'dependencySourceAfter':frame,
                'sourceAfter':{'README.md':hashlib.sha256(b'AGENT SOURCE').hexdigest()}}))
            (work/'dependency-metadata-after.json').write_text(json.dumps({'packages':[{'name':'cc-protocol','manifest_path':str(protocol/'crates/protocol/Cargo.toml')}]}))
            def original_git_archive(repo,*args):
                target=Path(next(a.split('=',1)[1]for a in args if a.startswith('--output=')))
                with tarfile.open(target,'w')as out:
                    for name,body in [('LICENSE',files['LICENSE'].read_bytes()),('agent/README.md',b'AGENT SOURCE')]:
                        row=tarfile.TarInfo('consolecrypt-agent-source/'+name);row.size=len(body);out.addfile(row,io.BytesIO(body))
                return b''
            with mock.patch.object(d,'__file__',str(delivery/'release_dependency_sources.py')),mock.patch.object(d,'capture',return_value=frame),mock.patch.object(d,'git',side_effect=original_git_archive):
                d.archive(work)
            with tarfile.open(work/'source.tar','r:')as source:
                self.assertEqual(source.extractfile('consolecrypt-agent-source/agent/README.md').read(),b'AGENT SOURCE')
                for name,path in files.items():self.assertEqual(source.extractfile('consolecrypt-agent-source/'+name).read(),path.read_bytes())


if __name__=='__main__':unittest.main()
