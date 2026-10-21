#!/usr/bin/env python3
"""Audit third-party dependencies: cargo-deny per Cargo.lock, osv-scanner for other lockfiles.

  install DIR TOOL...   fetch pinned linux x86_64 tools into DIR after a SHA-256 check
  check                 run every check and exit 1 on any finding
  issue                 open, update or close the dependency issue with the gh CLI

CI: client.yml (validate) runs `check` before any package is built;
dependency-audit.yml runs it weekly and on dependency pull requests, hosted only.
Policy and exceptions: docs/public/BUILDING.md.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import secrets
import shutil
import subprocess
import sys
import tarfile
import tempfile
import urllib.request

ROOT = Path(__file__).resolve().parents[2]
CARGO_MANIFESTS = ('client/rust/Cargo.toml', 'crates/Cargo.toml')
LOCKFILE_NAMES = {'pubspec.lock', 'package-lock.json', 'yarn.lock', 'pnpm-lock.yaml'}
OSV_CONFIG = 'osv-scanner.toml'
DOCS = 'docs/public/BUILDING.md'
# Own crates are AGPL-3.0-only. cargo-deny skips them ([licenses.private]),
# so this namespace rule replaces a per-crate exception list.
OWN_CRATE = re.compile(r'cc-[a-z0-9]+(?:-[a-z0-9]+)*')
OWN_GIT = re.compile(r'git\+https://github\.com/evsikovas/consolecrypt-server(?:\.git)?[?#]')
OWN_LICENSE = 'AGPL-3.0-only'
ISSUE_TITLE = 'Уязвимые зависимости'
SUMMARY_LIMIT = 60000

# Bump together with docs/public/BUILDING.md. Digests come from the upstream
# release checksum files and were re-verified on download.
TOOLS = {
    'cargo-deny': (
        'https://github.com/EmbarkStudios/cargo-deny/releases/download/0.20.2/'
        'cargo-deny-0.20.2-x86_64-unknown-linux-musl.tar.gz',
        '9f12ed4c49936e09b48bf862b595cde2fe64fcbd9d74dfacac6131ca824c8d5f',
        'cargo-deny-0.20.2-x86_64-unknown-linux-musl/cargo-deny',
    ),
    'osv-scanner': (
        'https://github.com/google/osv-scanner/releases/download/v2.6.0/osv-scanner_linux_amd64',
        'ca69b3d3cd08f889a49dc0a383122f71cc528b83803671df5fd874d97485b108',
        None,
    ),
}


def install(dest, names):
    if (platform.system(), platform.machine()) != ('Linux', 'x86_64'):
        raise SystemExit(f'install supports linux x86_64 CI runners only; see {DOCS} for local setup')
    dest.mkdir(parents=True, exist_ok=True)
    for name in names:
        url, digest, member = TOOLS[name]
        with tempfile.TemporaryDirectory() as tmp:
            download = Path(tmp) / 'download'
            with urllib.request.urlopen(url, timeout=120) as response, download.open('wb') as stream:
                shutil.copyfileobj(response, stream)
            actual = hashlib.sha256(download.read_bytes()).hexdigest()
            if actual != digest:
                raise SystemExit(f'{name}: SHA-256 {actual} does not match pinned {digest}')
            target = dest / name
            if member is None:
                shutil.copyfile(download, target)
            else:
                with tarfile.open(download) as archive, archive.extractfile(member) as source, \
                        target.open('wb') as stream:
                    shutil.copyfileobj(source, stream)
            target.chmod(0o755)
        print(f'Installed {name} ({digest[:12]}) into {dest}')


def deny_findings(lines, manifest):
    """Error diagnostics from `cargo deny --format json` (one JSON object per line)."""
    findings = []
    for line in lines:
        try:
            item = json.loads(line)
        except json.JSONDecodeError:
            continue
        fields = item.get('fields', {})
        if item.get('type') != 'diagnostic' or fields.get('severity') != 'error':
            continue
        advisory = fields.get('advisory') or {}
        graphs = fields.get('graphs') or [{}]
        krate = graphs[0].get('Krate') or {}
        package = f"{krate['name']}@{krate['version']}" if krate else advisory.get('package', '')
        findings.append({
            'source': manifest,
            'id': advisory.get('id') or fields.get('code', 'error'),
            'package': package,
            'title': advisory.get('title') or fields.get('message', ''),
        })
    return findings


def own_crate_findings(metadata, manifest):
    """Crates skipped by [licenses.private] must be own cc-* crates under AGPL-3.0-only."""
    findings = []
    for package in metadata['packages']:
        skipped = package.get('publish') == [] or OWN_GIT.match(package.get('source') or '')
        if skipped and not (OWN_CRATE.fullmatch(package['name']) and package.get('license') == OWN_LICENSE):
            findings.append({
                'source': manifest,
                'id': 'own-crate-license',
                'package': f"{package['name']}@{package['version']}",
                'title': f'unpublished crates must be named cc-* and licensed {OWN_LICENSE}',
            })
    return findings


def osv_findings(report):
    findings = []
    for result in report.get('results', []):
        path = result.get('source', {}).get('path', '')
        try:
            path = str(Path(path).resolve().relative_to(ROOT))
        except ValueError:
            pass
        for entry in result.get('packages', []):
            package = entry.get('package', {})
            titles = {v.get('id'): v.get('summary', '') for v in entry.get('vulnerabilities', [])}
            groups = entry.get('groups') or [{'ids': [i]} for i in titles]
            for group in groups:
                ids = group.get('ids') or ['?']
                findings.append({
                    'source': path,
                    'id': ', '.join(ids),
                    'package': f"{package.get('name')}@{package.get('version')}",
                    'title': next((titles[i] for i in ids if titles.get(i)), ''),
                })
    return findings


def lockfiles(root):
    tracked = subprocess.check_output(['git', 'ls-files', '-z'], cwd=root, text=True).split('\0')
    return sorted(path for path in tracked if path and Path(path).name in LOCKFILE_NAMES)


def run(cmd, root, **kwargs):
    print('+ ' + ' '.join(cmd), flush=True)
    return subprocess.run(cmd, cwd=root, text=True, **kwargs)


def check(root):
    findings, errors = [], []
    for manifest in CARGO_MANIFESTS:
        base = ['cargo', 'deny', '--manifest-path', manifest, '--locked']
        checks = ['advisories', 'bans', 'sources', 'licenses']
        if run([*base, '--log-level', 'error', 'check', *checks], root).returncode:
            result = run([*base, '--offline', '--format', 'json', 'check', *checks], root,
                         stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
            found = deny_findings(result.stderr.splitlines(), manifest)
            findings += found or [{'source': manifest, 'id': 'cargo-deny', 'package': '',
                                   'title': 'cargo-deny failed; see the job log'}]
        metadata = run(['cargo', 'metadata', '--manifest-path', manifest, '--locked', '--format-version', '1'],
                       root, capture_output=True)
        if metadata.returncode:
            errors.append(f'cargo metadata {manifest}: {metadata.stderr.strip()[-500:]}')
        else:
            findings += own_crate_findings(json.loads(metadata.stdout), manifest)
    files = lockfiles(root)
    if files and not shutil.which('osv-scanner'):
        errors.append(f'osv-scanner is not on PATH; see {DOCS}')
    elif files:
        cmd = ['osv-scanner', 'scan', 'source', '--config', OSV_CONFIG, '--format', 'json']
        for path in files:
            cmd += ['--lockfile', path]
        result = run(cmd, root, capture_output=True)
        sys.stderr.write(result.stderr)
        # 0: clean, 1: vulnerabilities found; anything else is a tool failure.
        found = osv_findings(json.loads(result.stdout)) if result.returncode in (0, 1) else []
        findings += found
        if result.returncode not in (0, 1) or (result.returncode == 1 and not found):
            errors.append(f'osv-scanner exited with {result.returncode}')
    return findings, errors


def summary(findings, errors):
    if not findings and not errors:
        return 'No vulnerable, yanked, banned or unlicensed dependencies found.\n'
    lines = []
    for source in sorted({f['source'] for f in findings}):
        lines.append(f'### `{source}`\n')
        for f in (f for f in findings if f['source'] == source):
            package = f" `{f['package']}`" if f['package'] else ''
            lines.append(f"- **{f['id']}**{package}: {f['title']}")
        lines.append('')
    if errors:
        lines.append('### Tool errors\n')
        lines += [f'- {error}' for error in errors]
        lines.append('')
    text = '\n'.join(lines)
    return text if len(text) <= SUMMARY_LIMIT else text[:SUMMARY_LIMIT] + '\n\n… (truncated; see the job log)\n'


def write_github_output(text):
    if output := os.environ.get('GITHUB_OUTPUT'):
        delimiter = 'EOF_' + secrets.token_hex(16)
        with open(output, 'a', encoding='utf-8') as stream:
            stream.write(f'summary<<{delimiter}\n{text}\n{delimiter}\n')
    if step_summary := os.environ.get('GITHUB_STEP_SUMMARY'):
        with open(step_summary, 'a', encoding='utf-8') as stream:
            stream.write('## Dependency audit\n\n' + text)


def gh(*args, stdin=None):
    return subprocess.run(['gh', *args], input=stdin, text=True, check=True, capture_output=True).stdout


def report_issue(outcome, text, run_url, gh=gh):
    """Open or update the issue on findings; close it after a clean run."""
    rows = json.loads(gh('issue', 'list', '--state', 'open', '--search', f'"{ISSUE_TITLE}" in:title',
                         '--json', 'number,title', '--limit', '100'))
    number = next((str(row['number']) for row in rows if row['title'] == ISSUE_TITLE), None)
    if outcome == 'failure':
        body = (f'Проверка зависимостей нашла проблемы: {run_url}\n\n{text}\n'
                f'Обновите зависимость или оформите исключение по `{DOCS}`. '
                'Issue закроется автоматически после чистого прогона.\n')
        if number:
            gh('issue', 'edit', number, '--body-file', '-', stdin=body)
            return f'updated #{number}'
        gh('issue', 'create', '--title', ISSUE_TITLE, '--body-file', '-', stdin=body)
        return 'created'
    if outcome == 'success' and number:
        gh('issue', 'close', number, '--comment', f'Чистый прогон: {run_url}')
        return f'closed #{number}'
    return 'unchanged'


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--root', type=Path, default=ROOT)
    commands = parser.add_subparsers(dest='command', required=True)
    install_cmd = commands.add_parser('install')
    install_cmd.add_argument('dest', type=Path)
    install_cmd.add_argument('tools', nargs='+', choices=sorted(TOOLS))
    commands.add_parser('check')
    issue_cmd = commands.add_parser('issue')
    issue_cmd.add_argument('--outcome', required=True)
    args = parser.parse_args()

    if args.command == 'install':
        install(args.dest, args.tools)
    elif args.command == 'check':
        findings, errors = check(args.root)
        text = summary(findings, errors)
        print('\n' + text)
        write_github_output(text)
        sys.exit(1 if findings or errors else 0)
    else:
        run_url = '{}/{}/actions/runs/{}'.format(os.environ['GITHUB_SERVER_URL'], os.environ['GITHUB_REPOSITORY'],
                                                os.environ['GITHUB_RUN_ID'])
        print(report_issue(args.outcome, os.environ.get('AUDIT_SUMMARY', ''), run_url))


if __name__ == '__main__':
    main()
