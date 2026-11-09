"""Security gate regressions with synthetic registries; no live network."""
import datetime as dt
import json
from pathlib import Path
import tempfile
import unittest

import osv_cargo


class OsvCargoTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        (self.root / 'Cargo.toml').write_text('[package]\nname="test"\nversion="1.0.0"\n[dependencies]\npublic="1"\n')
        (self.root / 'Cargo.lock').write_text(
            'version=4\n[[package]]\nname="public"\nversion="1.0.0"\n'
            'source="registry+https://github.com/rust-lang/crates.io-index"\n'
            '[[package]]\nname="private"\nversion="1.0.0"\nsource="registry+https://private.invalid/index"\n')

    def request(self, informational=None, withdrawn=False):
        def request(path, payload=None):
            if path == 'querybatch':
                self.assertEqual([q['package']['name'] for q in payload['queries']], ['public'])
                return {'results': [{'vulns': [{'id': 'GHSA-example'}]}]}
            return {'summary': 'test advisory', 'withdrawn': '2026-01-01' if withdrawn else None,
                    'affected': [{'package': {'name': 'public'}, 'database_specific': {'informational': informational}}]}
        return request

    def check(self, request):
        return osv_cargo.check(self.root, ['Cargo.toml'], request=request, today=dt.date(2026, 11, 4))

    def test_ghsa_only_advisory_blocks_and_private_registry_is_never_submitted(self):
        findings = self.check(self.request())
        self.assertEqual(findings[0]['id'], 'GHSA-example')
        self.assertEqual(findings[0]['package'], 'public@1.0.0')

    def test_direct_unmaintained_blocks_and_transitive_is_reported(self):
        self.assertEqual(len(self.check(self.request('unmaintained'))), 1)
        (self.root / 'Cargo.toml').write_text('[package]\nname="test"\nversion="1.0.0"\n')
        self.assertEqual(self.check(self.request('unmaintained')), [])

    def test_unsound_is_not_treated_as_unmaintained(self):
        (self.root / 'Cargo.toml').write_text('[package]\nname="test"\nversion="1.0.0"\n')
        self.assertEqual(len(self.check(self.request('unsound'))), 1)

    def test_withdrawn_advisory_does_not_block(self):
        self.assertEqual(self.check(self.request(withdrawn=True)), [])

    def test_incomplete_response_fails_closed(self):
        with self.assertRaises(ValueError):
            self.check(lambda *_: {'results': []})

    def test_expired_exception_fails_closed(self):
        (self.root / 'security-exceptions.toml').write_text(
            '[[exception]]\nid="GHSA-example"\npackage="public"\n'
            'owner="maintainer"\nreason="test"\nexpires="2026-11-03"\n')
        with self.assertRaises(ValueError):
            self.check(self.request())

    def test_valid_exception_is_package_bound(self):
        policy = ('[[exception]]\nid="GHSA-example"\npackage="public"\n'
                  'owner="maintainer"\nreason="test"\nexpires="2027-01-01"\n')
        (self.root / 'security-exceptions.toml').write_text(policy)
        self.assertEqual(self.check(self.request()), [])
        (self.root / 'security-exceptions.toml').write_text(policy.replace('package="public"', 'package="wrong"'))
        with self.assertRaises(ValueError):
            self.check(self.request())

    def test_backport_guard_rejects_source_tampering_and_registry_replacement(self):
        client = self.root / 'client'
        source = self.root / 'local'
        (source / 'src').mkdir(parents=True)
        client.mkdir()
        (source / 'Cargo.toml').write_text('[package]\nname="public"\nversion="1.0.0"\n')
        code = source / 'src/lib.rs'
        code.write_bytes(b'// reviewed\r\n')
        digest = osv_cargo.source_digest(source)
        code.write_bytes(b'// reviewed\n')
        self.assertEqual(digest, osv_cargo.source_digest(source))
        manifest = self.root / 'Cargo.toml'
        manifest.write_text('[patch.crates-io]\npublic={path="local"}\n')
        lock = self.root / 'Cargo.lock'
        lock.write_text('[[package]]\nname="public"\nversion="1.0.0"\n')
        (client / 'security-backports.json').write_text(json.dumps({'public': {
            'patch_manifest': 'Cargo.toml', 'source': 'local',
            'package_version': '1.0.0', 'source_sha256': digest,
        }}))
        osv_cargo.verify_backports(self.root)
        code.write_text('// missing fix\n')
        with self.assertRaises(ValueError):
            osv_cargo.verify_backports(self.root)
        code.write_text('// reviewed\n')
        lock.write_text(lock.read_text() + 'source="registry+https://github.com/rust-lang/crates.io-index"\n')
        with self.assertRaises(ValueError):
            osv_cargo.verify_backports(self.root)

    def test_native_backport_digest_covers_build_script_c_and_headers(self):
        self.public_backport()
        source = self.root / 'local'
        (source / 'build.rs').write_text('// reviewed build\n')
        (source / 'engine.c').write_text('// reviewed fix\n')
        (source / 'engine.h').write_text('// reviewed interface\n')
        records_path = self.root / 'client/security-backports.json'
        records = json.loads(records_path.read_text())
        records['public'].update(native_sources=True,
            source_sha256=osv_cargo.source_digest(source, native_sources=True))
        records_path.write_text(json.dumps(records))
        osv_cargo.verify_backports(self.root)
        for name in ['build.rs', 'engine.c', 'engine.h']:
            path = source / name
            original = path.read_bytes()
            path.write_bytes(original + b'// unreviewed\n')
            with self.assertRaises(ValueError):
                osv_cargo.verify_backports(self.root)
            path.write_bytes(original)
        # New native build inputs also invalidate the reviewed digest.
        (source / 'extra.c').write_text('// new unreviewed file\n')
        with self.assertRaises(ValueError):
            osv_cargo.verify_backports(self.root)

    def test_native_digest_uses_the_same_case_sensitive_order_on_every_platform(self):
        self.public_backport()
        source = self.root / 'local'
        for name in ['Z.c', 'a.c', 'build.rs']:
            (source / name).write_bytes(b'// reviewed\n')
        import hashlib
        expected = hashlib.sha256()
        for name in ['Cargo.toml', 'Z.c', 'a.c', 'build.rs', 'src/lib.rs']:
            expected.update(name.encode() + b'\0')
            expected.update((source / name).read_bytes().replace(b'\r\n', b'\n') + b'\0')
        self.assertEqual(osv_cargo.source_digest(source, native_sources=True), expected.hexdigest())

    def public_backport(self):
        source = self.root / 'local'
        (source / 'src').mkdir(parents=True)
        (self.root / 'client').mkdir()
        (source / 'Cargo.toml').write_text('[package]\nname="public"\nversion="1.0.0"\n')
        (source / 'src/lib.rs').write_text('// reviewed\n')
        (self.root / 'Cargo.toml').write_text('[package]\nname="test"\nversion="1.0.0"\n[patch.crates-io]\npublic={path="local"}\n')
        (self.root / 'Cargo.lock').write_text('[[package]]\nname="public"\nversion="1.0.0"\n'
            '[[package]]\nname="private-local"\nversion="1.0.0"\n')
        (self.root / 'client/security-backports.json').write_text(json.dumps({'public': {
            'patch_manifest': 'Cargo.toml', 'source': 'local', 'package_version': '1.0.0',
            'source_sha256': osv_cargo.source_digest(source), 'public_crates_io_upstream': True,
        }}))

    def test_reviewed_public_local_upstream_is_scanned_but_private_local_is_not(self):
        self.public_backport()
        self.assertEqual(len(self.check(self.request())), 1)

    def test_backport_exception_does_not_cover_same_version_in_another_registry_graph(self):
        self.public_backport()
        (self.root / 'security-exceptions.toml').write_text(
            '[[exception]]\nid="GHSA-example"\npackage="public"\nrequires_backport="public"\n'
            'owner="maintainer"\nreason="test"\nexpires="2027-01-01"\n')
        self.assertEqual(self.check(self.request()), [])
        other = self.root / 'other'
        other.mkdir()
        (other / 'Cargo.toml').write_text('[package]\nname="other"\nversion="1.0.0"\n')
        (other / 'Cargo.lock').write_text('[[package]]\nname="public"\nversion="1.0.0"\n'
            'source="registry+https://github.com/rust-lang/crates.io-index"\n')
        with self.assertRaises(ValueError):
            osv_cargo.check(self.root, ['Cargo.toml', 'other/Cargo.toml'],
                            request=self.request(), today=dt.date(2026, 11, 4))


if __name__ == '__main__':
    unittest.main()
