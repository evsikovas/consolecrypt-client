#!/usr/bin/env python3
"""Check the canonical source tree without importing/starting the Agent."""
import argparse
import ast
import hashlib
import json
from pathlib import Path, PurePosixPath
import re
import stat
import subprocess
import tomllib
import zipfile

ROOT = Path(__file__).resolve().parents[1]
REPO = ROOT.parent
IGNORED = {'target', '.cache', 'cache', 'evidence', 'state', 'private', 'secrets', 'examples', '__pycache__', '.git', 'dist', 'artifacts'}
PRIVATE_SUFFIXES = {'.key','.pem','.p12','.pfx','.jks','.keystore','.db','.sqlite','.sqlite3','.log','.pyc','.exe','.dll','.so','.a','.o','.elf','.test'}
WHEEL = 'third_party/websockets/websockets-17.4-py3-none-any.whl'
SEAL = 'SHIPPING_SOURCE_MANIFEST.json'


def digest(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream,'sha256').hexdigest()


def source_name(name):
    p = PurePosixPath(name)
    if p.is_absolute() or '..' in p.parts or str(p)!=name or '\\' in name or ':' in name:
        return False
    if any(part in IGNORED for part in p.parts) or p.name.lower().startswith('.env'):
        return False
    if p.suffix.lower() in PRIVATE_SUFFIXES or p.suffix=='.whl' and name!=WHEEL:
        return False
    return True


def inventory():
    raw = subprocess.check_output(['git','-C',str(REPO),'ls-files','-co','--exclude-standard','-z','--','agent/'])
    names = sorted({n.removeprefix('agent/') for n in raw.decode('utf-8').split('\0') if n})
    result={}
    for name in names:
        if not source_name(name):
            raise ValueError('private/output path is Git-visible: '+name)
        path=ROOT/name
        info=path.lstat()
        if not stat.S_ISREG(info.st_mode) or getattr(info,'st_file_attributes',0)&0x400 or path.resolve(strict=True)!=path.absolute():
            raise ValueError('regular source file required: '+name)
        if info.st_size>32*1024**2:
            raise ValueError('source file exceeds32MiB: '+name)
        with path.open('rb') as stream:head=stream.read(4)
        if head.startswith(b'\x7fELF') or head.startswith(b'MZ'):
            raise ValueError('compiled artifact in source: '+name)
        if name!=SEAL:
            result[name]=dict(sha256=digest(path),bytes=info.st_size)
    if sum(r['bytes'] for r in result.values())>64*1024**2:
        raise ValueError('source closure exceeds64MiB')
    return result



def package_sources(text, files):
    # The maintained packager owns the literal list. Never execute/import it to
    # discover archive inputs, and never freeze a stale count in this checker.
    tree = ast.parse(text)
    rows = [node for node in tree.body if isinstance(node, ast.Assign)
            and len(node.targets) == 1 and isinstance(node.targets[0], ast.Name)
            and node.targets[0].id == 'SOURCES']
    stores = [node for node in ast.walk(tree) if isinstance(node, ast.Name)
              and isinstance(node.ctx, ast.Store) and node.id == 'SOURCES']
    if (len(rows) != 1 or len(stores) != 1 or not isinstance(rows[0].value, ast.Tuple)
            or not all(isinstance(node, ast.Constant) and type(node.value) is str
                       for node in rows[0].value.elts)):
        raise ValueError('one explicit literal package source tuple required')
    names = ast.literal_eval(rows[0].value)
    if not 1 <= len(names) <= 128 or len(names) != len(set(names)):
        raise ValueError('bounded unique package source closure required')
    for name in names:
        if (len(name) > 160 or not source_name(name)
                or any(part in {'tests', 'examples', '.cache', 'target', '__pycache__'}
                       for part in PurePosixPath(name).parts)
                or name not in files):
            raise ValueError('package source outside reviewed source inventory: ' + name)
    return names


def check(files):
    required={'LICENSE','DEVICE_BUILD_MANIFEST.json','identity/Cargo.toml','identity/Cargo.lock','third_party/sqlite/SOURCE.json','third_party/prometheus-agent/SOURCE.json',WHEEL}
    if not required<=files.keys():
        raise ValueError('required shipping source inputs missing')
    cargo=tomllib.loads((ROOT/'identity/Cargo.toml').read_text(encoding='utf-8'))
    if cargo.get('patch',{}).get('crates-io',{}).get('libsqlite3-sys',{}).get('path')!='vendor/libsqlite3-sys':
        raise ValueError('reviewed SQLite path patch required')
    lock=tomllib.loads((ROOT/'identity/Cargo.lock').read_text(encoding='utf-8'))
    selected=[p for p in lock['package'] if p['name']=='libsqlite3-sys']
    if len(selected)!=1 or selected[0]['version']!='0.38.2' or 'source' in selected[0] or 'checksum' in selected[0]:
        raise ValueError('SQLite lock must select the reviewed path component')
    package_sources((ROOT/'delivery/prototype_package.py').read_text(encoding='utf-8'), files)
    if (ROOT/'launch.py').read_bytes()!=(ROOT/'delivery/launch.py').read_bytes():
        raise ValueError('stable launch copies differ')
    for name in files:
        path=ROOT/name
        if path.suffix=='.py':ast.parse(path.read_text(encoding='utf-8'),filename=name)
        if path.suffix=='.rs' and not name.startswith('identity/vendor/'):
            text=path.read_text(encoding='utf-8')
            refs=re.findall(r'include_(?:str|bytes)!\s*\(\s*"([^"]+)"',text)
            refs+=re.findall(r'#\[path\s*=\s*"([^"]+)"\]',text)
            for ref in refs:
                if not (path.parent/ref).is_file():raise ValueError('unresolved Rust source fixture: '+name+' -> '+ref)
    for component in ('sqlite','websockets','prometheus-agent'):
        value=json.loads((ROOT/'third_party'/component/'SOURCE.json').read_bytes())
        pins=value.get('files_sha256',{})
        if component=='websockets':pins={value['wheel']['path']:value['wheel']['sha256'],value['license_path']:'3d6a0c050d8bec52fabad502e45fb25bd02bcadbd70dea34d447b6a0ff4e6da8'}
        if component=='prometheus-agent':
            pins={r['path']:r['sha256'] for r in value['patches']}
            pins.update({r['source']:r['sha256'] for r in value['additions']})
            pins.update({'third_party/prometheus-agent/upstream/'+n:h for n,h in value['upstream_pins'].items() if n in {'go.mod','go.sum','LICENSE','NOTICE'}})
        for name,expected in pins.items():
            if name not in files:raise ValueError('component source excluded from Git-visible closure: '+name)
            if digest(ROOT/name)!=expected:raise ValueError('component source changed: '+name)
    wheel=json.loads((ROOT/'third_party/websockets/SOURCE.json').read_bytes())
    if digest(ROOT/wheel['license_path'])!='3d6a0c050d8bec52fabad502e45fb25bd02bcadbd70dea34d447b6a0ff4e6da8':raise ValueError('WSS license changed')
    with zipfile.ZipFile(ROOT/WHEEL) as z:
        if sum(i.file_size for i in z.infolist())>2*1024**2 or len(z.infolist())>128:raise ValueError('wheel bounds')
        for name in z.namelist():
            p=PurePosixPath(name)
            if p.is_absolute() or '..' in p.parts or p.suffix.lower() in {'.so','.dll','.pyd','.exe'}:raise ValueError('pure wheel required')
    seal=ROOT/SEAL
    if seal.exists():
        bound=json.loads(seal.read_bytes())
        if bound.get('format')!='cc-agent-shipping-source-manifest/v1' or bound.get('files')!=files:raise ValueError('shipping source seal differs; review current source before resealing')
    return dict(files=len(files),bytes=sum(v['bytes'] for v in files.values()),source_sealed=seal.exists(),activation=False)


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--seal',action='store_true',help='Explicit final source binding AFTER reviewed phase2 merges; no binary/package release')
    args=parser.parse_args()
    files=inventory();result=check(files)
    if args.seal:
        with (ROOT/SEAL).open('x',encoding='ascii') as stream:
            json.dump(dict(format='cc-agent-shipping-source-manifest/v1',files=files,binary_release=False),stream,indent=2);stream.write('\n')
        result['source_sealed']=True
    print(json.dumps(result,sort_keys=True))


if __name__=='__main__':main()
