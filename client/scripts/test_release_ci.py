"""Check the closed release-candidate rule matrix without CI or credentials.

This intentionally handles only the rules used by these native jobs; an
unknown GitLab expression fails the test rather than being interpreted as safe.
"""
from pathlib import Path
import re
import unittest

ROOT = Path(__file__).resolve().parents[2]
CANDIDATE = 'codex/rdp-0.3-source'
JOBS = {
    'build-windows': ('.gitlab-ci.yml', '[windows]', 'consolecrypt-windows'),
    'build-macos': ('.gitlab-ci.yml', '[macos, arm64]', 'consolecrypt-macos'),
    'build-linux': ('client/ci/linux.yml', '[macos, arm64]', 'consolecrypt-macos'),
    'build-ios-preview': ('client/ci/ios.yml', '[macos, arm64]', 'consolecrypt-macos'),
}


def job_text(contents, name):
    match = re.search(r'^' + re.escape(name) + r':\n(?P<body>(?:[ \t].*\n|\n|#.*\n)*)', contents, re.M)
    if match is None:
        raise ValueError('Expected native job')
    return match['body']


def rules_in(body):
    lines = body.splitlines()
    start = lines.index('  rules:')
    rules = []
    for line in lines[start + 1:]:
        if line and not line.startswith('    '):
            break
        if line.startswith('    - if: '):
            value = line[len('    - if: '):]
            if len(value) < 2 or value[0] != "'" or value[-1] != "'":
                raise ValueError('Expected a literal rule expression')
            rules.append({'if': value[1:-1]})
        elif line.startswith('      when: '):
            rules[-1]['when'] = line[len('      when: '):]
        elif line.startswith('      allow_failure: '):
            value = line[len('      allow_failure: '):]
            if value not in ('true', 'false'):
                raise ValueError('Expected a Boolean failure policy')
            rules[-1]['allow_failure'] = value == 'true'
        elif (not line.strip() or line.startswith('      changes:')
              or line.startswith('        - ')):
            continue
        else:
            raise ValueError('Unexpected native rule field')
    return rules


def matches(expression, branch='', default='main', tag=''):
    if expression == '$CI_COMMIT_BRANCH':
        return bool(branch)
    if expression == '$CI_COMMIT_BRANCH == $CI_DEFAULT_BRANCH':
        return bool(branch) and branch == default
    literal = re.fullmatch(r'\$CI_COMMIT_BRANCH == "([a-zA-Z0-9_./-]+)"', expression)
    if literal:
        return bool(branch) and branch == literal[1]
    if expression == '$CI_COMMIT_TAG =~ /^server-v[0-9]+\\.[0-9]+\\.[0-9]+$/':
        return bool(re.fullmatch(r'server-v[0-9]+\.[0-9]+\.[0-9]+', tag))
    raise ValueError('Unsupported condition: safety review required')


def first_rule(rules, **environment):
    return next((rule for rule in rules if matches(rule['if'], **environment)), None)


class ReleaseCandidateCiTests(unittest.TestCase):
    def test_exact_candidate_auto_builds_all_four_platforms_and_failures_block(self):
        for name, (path, _, _) in JOBS.items():
            with self.subTest(job=name):
                body = job_text((ROOT / path).read_text(), name)
                rules = rules_in(body)
                rule = first_rule(rules, branch=CANDIDATE)
                self.assertEqual(rule, {
                    'if': f'$CI_COMMIT_BRANCH == "{CANDIDATE}"',
                    'when': 'on_success', 'allow_failure': False,
                })
                self.assertIs(rule, rules[0])

    def test_other_feature_branches_and_tags_do_not_auto_build(self):
        for name, (path, _, _) in JOBS.items():
            rules = rules_in(job_text((ROOT / path).read_text(), name))
            for branch in ('dev', 'codex/rdp-0.3-release', CANDIDATE + '-test',
                           CANDIDATE + '/extra', 'codex/publish-release-0-3-0'):
                with self.subTest(job=name, branch=branch):
                    rule = first_rule(rules, branch=branch)
                    self.assertEqual(rule['when'], 'manual')
                    self.assertTrue(rule['allow_failure'])
            self.assertIsNone(first_rule(rules, tag='v0.3.0'))

    def test_default_branch_behavior_and_runner_serialization_are_preserved(self):
        for name, (path, tags, resource_group) in JOBS.items():
            with self.subTest(job=name):
                body = job_text((ROOT / path).read_text(), name)
                self.assertIn('  tags: ' + tags, body)
                self.assertIn('  resource_group: ' + resource_group, body)
                rule = first_rule(rules_in(body), branch='main')
                self.assertEqual(rule.get('when', 'on_success'),
                                 'manual' if name == 'build-ios-preview' else 'on_success')

    def test_candidate_does_not_enable_server_or_publication_deployments(self):
        config = (ROOT / '.gitlab-ci.yml').read_text()
        include = re.search(r'  - local: server/ci/registry.yml\n(?P<body>(?:    .*\n|      .*\n)*)', config)
        self.assertIsNotNone(include)
        expressions = re.findall(r"- if: '([^']+)'", include['body'])
        self.assertEqual(len(expressions), 2)
        self.assertFalse(any(matches(value, branch=CANDIDATE) for value in expressions))
        self.assertTrue(any(matches(value, branch='main') for value in expressions))
        self.assertTrue(any(matches(value, tag='server-v0.1.11') for value in expressions))
        for text in [config, (ROOT / 'client/ci/ios.yml').read_text(),
                     (ROOT / 'client/ci/linux.yml').read_text()]:
            self.assertNotIn('publish-release.py', text)
            self.assertNotIn('.local/updates', text)
            self.assertNotIn('site.py', text)

    def test_ios_auto_job_is_unsigned_preview_and_regression_remains_opt_in(self):
        config = (ROOT / 'client/ci/ios.yml').read_text()
        body = job_text(config, 'build-ios-preview')
        self.assertIn('bash client/scripts/ci-ios.sh --build-only', body)
        self.assertIn('dist/ios/ci-preview/ConsoleCrypt-ios-preview.json', body)
        self.assertNotIn('dist/ios/ConsoleCrypt-*-ios-simulator-universal.zip', body)
        self.assertNotIn('--device', body)
        regression = rules_in(job_text(config, 'ios-regression'))
        self.assertEqual(first_rule(regression, branch=CANDIDATE)['when'], 'manual')
        script = (ROOT / 'client/scripts/ci-ios.sh').read_text()
        self.assertIn('CI_JOB_ID', script)
        self.assertIn('retain-ios-preview.py', script)
        self.assertIn('simctl create', script)
        self.assertIn('simctl delete "$simulator_id"', script)


if __name__ == '__main__':
    unittest.main()
