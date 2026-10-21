"""Dependency audit parsing, own-crate rule and issue lifecycle without network."""
import importlib.util
import json
from pathlib import Path
import unittest


def load(name):
    spec = importlib.util.spec_from_file_location(name.replace('-', '_'), Path(__file__).with_name(name + '.py'))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


audit = load('dependency-audit')


def diagnostic(severity, code, advisory=None, krate=None):
    fields = {'severity': severity, 'code': code, 'message': f'{code} message'}
    if advisory:
        fields['advisory'] = advisory
    if krate:
        fields['graphs'] = [{'Krate': krate}]
    return json.dumps({'type': 'diagnostic', 'fields': fields})


def package(name, publish=None, license='MIT', source='registry+https://github.com/rust-lang/crates.io-index'):
    return {'name': name, 'version': '0.1.0', 'publish': publish, 'license': license, 'source': source}


class DenyTests(unittest.TestCase):
    def test_only_errors_become_findings(self):
        lines = [
            diagnostic('error', 'vulnerability', {'id': 'RUSTSEC-2026-0346', 'title': 'TLS secret in Debug'},
                       {'name': 'rustls', 'version': '0.23.45'}),
            diagnostic('warning', 'duplicate', krate={'name': 'syn', 'version': '1.0.0'}),
            diagnostic('error', 'yanked', krate={'name': 'yoke-derive', 'version': '0.8.3'}),
            'not json',
            json.dumps({'type': 'summary', 'fields': {}}),
        ]
        found = audit.deny_findings(lines, 'client/rust/Cargo.toml')
        self.assertEqual([(f['id'], f['package']) for f in found],
                         [('RUSTSEC-2026-0346', 'rustls@0.23.45'), ('yanked', 'yoke-derive@0.8.3')])
        self.assertEqual(found[0]['title'], 'TLS secret in Debug')


class OwnCrateTests(unittest.TestCase):
    def test_namespace_rule(self):
        git = 'git+https://github.com/evsikovas/consolecrypt-server.git?rev=abc#abc'
        ok = [package('cc-new-core', [], 'AGPL-3.0-only'), package('cc-protocol', None, 'AGPL-3.0-only', git),
              package('serde'), package('portable-pty', None, 'MIT', None)]
        self.assertEqual(audit.own_crate_findings({'packages': ok}, 'm'), [])
        bad = [package('vendored', [], 'MIT'), package('cc-core', [], 'MIT'),
               package('evil', None, 'GPL-2.0-only', git), package('CC-Upper', [], 'AGPL-3.0-only')]
        self.assertEqual([f['package'] for f in audit.own_crate_findings({'packages': bad}, 'm')],
                         ['vendored@0.1.0', 'cc-core@0.1.0', 'evil@0.1.0', 'CC-Upper@0.1.0'])

    def test_lookalike_repository_is_not_own(self):
        evil = 'git+https://github.com/evsikovas/consolecrypt-server-fork?rev=abc#abc'
        self.assertIsNone(audit.OWN_GIT.match(evil))


class OsvTests(unittest.TestCase):
    def test_groups_aliases(self):
        report = {'results': [{'source': {'path': str(audit.ROOT / 'client/flutter/pubspec.lock')}, 'packages': [{
            'package': {'name': 'http', 'version': '0.13.0', 'ecosystem': 'Pub'},
            'vulnerabilities': [{'id': 'GHSA-aaaa', 'summary': 'CRLF injection'}, {'id': 'CVE-2020-1'}],
            'groups': [{'ids': ['GHSA-aaaa', 'CVE-2020-1']}],
        }]}]}
        self.assertEqual(audit.osv_findings(report), [{
            'source': 'client/flutter/pubspec.lock', 'id': 'GHSA-aaaa, CVE-2020-1',
            'package': 'http@0.13.0', 'title': 'CRLF injection'}])
        self.assertEqual(audit.osv_findings({'results': []}), [])


class SummaryTests(unittest.TestCase):
    def test_clean_and_findings(self):
        self.assertIn('No vulnerable', audit.summary([], []))
        text = audit.summary([{'source': 'a', 'id': 'X-1', 'package': 'p@1', 'title': 't'}], ['osv-scanner exited with 127'])
        self.assertIn('### `a`', text)
        self.assertIn('- **X-1** `p@1`: t', text)
        self.assertIn('osv-scanner exited with 127', text)

    def test_truncates(self):
        findings = [{'source': 'a', 'id': str(i), 'package': '', 'title': 'x' * 100} for i in range(1000)]
        self.assertLess(len(audit.summary(findings, [])), audit.SUMMARY_LIMIT + 100)


class FakeGh:
    def __init__(self, open_issues):
        self.open_issues = open_issues
        self.calls = []

    def __call__(self, *args, stdin=None):
        self.calls.append((args, stdin))
        return json.dumps(self.open_issues) if args[:2] == ('issue', 'list') else ''


class IssueTests(unittest.TestCase):
    URL = 'https://github.com/o/r/actions/runs/1'

    def actions(self, gh):
        return [call[0][:2] for call in gh.calls[1:]]

    def test_failure_creates_then_updates(self):
        gh = FakeGh([{'number': 3, 'title': 'Уязвимые зависимости (old)'}])
        self.assertEqual(audit.report_issue('failure', '- **X**', self.URL, gh), 'created')
        self.assertEqual(self.actions(gh), [('issue', 'create')])
        self.assertIn('- **X**', gh.calls[1][1])
        self.assertIn(self.URL, gh.calls[1][1])
        gh = FakeGh([{'number': 7, 'title': audit.ISSUE_TITLE}])
        self.assertEqual(audit.report_issue('failure', '- **X**', self.URL, gh), 'updated #7')
        self.assertEqual(gh.calls[1][0][:3], ('issue', 'edit', '7'))

    def test_success_closes_only_open_issue(self):
        gh = FakeGh([{'number': 7, 'title': audit.ISSUE_TITLE}])
        self.assertEqual(audit.report_issue('success', '', self.URL, gh), 'closed #7')
        self.assertEqual(gh.calls[1][0][:3], ('issue', 'close', '7'))
        gh = FakeGh([])
        self.assertEqual(audit.report_issue('success', '', self.URL, gh), 'unchanged')
        self.assertEqual(self.actions(gh), [])

    def test_cancelled_run_changes_nothing(self):
        gh = FakeGh([{'number': 7, 'title': audit.ISSUE_TITLE}])
        self.assertEqual(audit.report_issue('cancelled', '', self.URL, gh), 'unchanged')
        self.assertEqual(self.actions(gh), [])


class PinTests(unittest.TestCase):
    def test_tools_are_pinned_by_version_and_digest(self):
        for name, (url, digest, _member) in audit.TOOLS.items():
            with self.subTest(name=name):
                self.assertRegex(url, r'^https://github\.com/[^/]+/[^/]+/releases/download/v?\d+\.\d+\.\d+/')
                self.assertRegex(digest, r'^[0-9a-f]{64}$')


if __name__ == '__main__':
    unittest.main()
