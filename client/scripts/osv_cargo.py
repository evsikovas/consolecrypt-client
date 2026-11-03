"""Supplement RustSec with OSV/GHSA for the Cargo graphs actually shipped.

Only public crates.io package names/versions are submitted. Local backports
are checked separately; git/private registries are never uploaded to OSV.
"""
import concurrent.futures
import datetime as dt
import json
import hashlib
from pathlib import Path
import tomllib
import urllib.request


def api(path, payload=None):
    request = urllib.request.Request(
        'https://api.osv.dev/v1/' + path,
        data=None if payload is None else json.dumps(payload).encode(),
        headers={'Content-Type': 'application/json', 'User-Agent': 'ConsoleCrypt-dependency-gate'},
    )
    with urllib.request.urlopen(request, timeout=30) as response:
        return json.load(response)


def policy(root, today):
    path = root / 'security-exceptions.toml'
    if not path.exists():
        return {}
    result = {}
    for entry in tomllib.loads(path.read_text(encoding='utf-8')).get('exception', []):
        expiry = dt.date.fromisoformat(entry['expires'])
        if not entry.get('reason') or not entry.get('owner') or expiry < today:
            raise ValueError(f"expired or incomplete security exception: {entry.get('id')}")
        if expiry > today + dt.timedelta(days=93):
            raise ValueError('security exception review must be within three months')
        result[entry['id']] = entry
    return result


def source_digest(source):
    """Hash build manifests and Rust sources, independent of checkout line endings."""
    digest = hashlib.sha256()
    paths = sorted([source / 'Cargo.toml', *(source / 'src').rglob('*.rs')])
    for path in paths:
        digest.update(path.relative_to(source).as_posix().encode() + b'\0')
        digest.update(path.read_bytes().replace(b'\r\n', b'\n') + b'\0')
    return digest.hexdigest()


def verify_backports(root):
    records = json.loads((root / 'client/security-backports.json').read_text(encoding='utf-8'))
    for name, record in records.items():
        manifest = root / record['patch_manifest']
        doc = tomllib.loads(manifest.read_text(encoding='utf-8'))
        patch = doc['patch']['crates-io'][name]
        source = (manifest.parent / patch['path']).resolve()
        if source != (root / record['source']).resolve():
            raise ValueError('security backport is not wired into the build')
        if 'source_sha256' in record:
            valid = source_digest(source) == record['source_sha256']
        else:
            content = (source / record['file']).read_bytes().replace(b'\r\n', b'\n')
            valid = hashlib.sha256(content).hexdigest() == record['sha256']
        if not valid:
            raise ValueError('security backport hash changed; review required')
        packages = tomllib.loads(manifest.with_name('Cargo.lock').read_text(encoding='utf-8'))['package']
        selected = [p for p in packages if p['name'] == name]
        if len(selected) != 1 or selected[0].get('source') or selected[0]['version'] != record['package_version']:
            raise ValueError('lockfile does not select the reviewed local security backport')


def check(root, manifests, request=api, today=None):
    today = today or dt.datetime.now(dt.timezone.utc).date()
    exceptions = policy(root, today)
    packages, direct = {}, set()
    for manifest in manifests:
        path = root / manifest
        document = tomllib.loads(path.read_text(encoding='utf-8'))
        # Direct dependencies of every member, including target-specific ones.
        documents = [document]
        for member in document.get('workspace', {}).get('members', []):
            documents.append(tomllib.loads((path.parent / member / 'Cargo.toml').read_text(encoding='utf-8')))
        for doc in documents:
            scopes = [doc, *doc.get('target', {}).values()]
            for scope in scopes:
                for section in ['dependencies', 'build-dependencies', 'dev-dependencies']:
                    for name, value in scope.get(section, {}).items():
                        direct.add(value.get('package', name) if isinstance(value, dict) else name)
        lock = tomllib.loads(path.with_name('Cargo.lock').read_text(encoding='utf-8'))
        for package in lock['package']:
            if package.get('source') == 'registry+https://github.com/rust-lang/crates.io-index':
                key = (package['name'], package['version'])
                packages.setdefault(key, []).append(str(path.with_name('Cargo.lock').relative_to(root)))
    keys = sorted(packages)
    matches = []
    for offset in range(0, len(keys), 200):
        batch = keys[offset:offset + 200]
        result = request('querybatch', {'queries': [
            {'package': {'ecosystem': 'crates.io', 'name': name}, 'version': version}
            for name, version in batch
        ]})
        if len(result.get('results', [])) != len(batch):
            raise ValueError('incomplete OSV response')
        for key, row in zip(batch, result['results']):
            for vulnerability in row.get('vulns', []):
                matches.append((key, vulnerability['id']))
    ids = sorted({identifier for _, identifier in matches})
    with concurrent.futures.ThreadPoolExecutor(max_workers=4) as pool:
        documents = dict(zip(ids, pool.map(lambda identifier: request('vulns/' + identifier), ids)))
    findings, warnings = [], []
    for (name, version), identifier in matches:
        advisory = documents[identifier]
        if advisory.get('withdrawn'):
            continue
        aliases = {identifier, *advisory.get('aliases', [])}
        exception = next((exceptions[i] for i in aliases if i in exceptions), None)
        if exception:
            if exception['package'] != name:
                raise ValueError('security exception package mismatch')
            warnings.append(f"{identifier}: {name}@{version}, reviewed exception until {exception['expires']}")
            continue
        affected = [entry for entry in advisory.get('affected', []) if entry.get('package', {}).get('name') == name]
        unmaintained = affected and all(entry.get('database_specific', {}).get('informational') == 'unmaintained' for entry in affected)
        if unmaintained and name not in direct:
            warnings.append(f'{identifier}: transitive {name}@{version} is unmaintained')
            continue
        for source in packages[(name, version)]:
            findings.append({'source': source, 'id': identifier, 'package': f'{name}@{version}', 'title': advisory.get('summary', '')})
    print(f'OSV/GHSA: checked {len(keys)} public Cargo package versions in {len(manifests)} build graphs')
    for warning in sorted(set(warnings)):
        print('NOTICE: ' + warning)
    return findings
