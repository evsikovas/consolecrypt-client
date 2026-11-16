"""Fixed public Native/protocol source provenance for Agent release candidates.
No build, network, signing key, runtime profile or authority constructor.
"""
import argparse
import hashlib
import io
import json
import os
from pathlib import Path
import stat
import subprocess
import tarfile
import tomllib

REV = 'b13f632bf04d92753de8971c5ec0343ec68f3afe'
GIT_SOURCE = 'git+https://github.com/evsikovas/consolecrypt-server.git?rev=' + REV + '#' + REV
ROOTS = ('client/rust/crypto-core', 'client/rust/agent-node-wire', 'crates/models')
METADATA = ('client/rust/Cargo.toml', 'client/rust/Cargo.lock', 'crates/Cargo.toml', 'crates/Cargo.lock')
SELECTED = ('client/rust/Cargo.toml', 'client/rust/agent-node-wire/Cargo.toml', 'client/rust/agent-node-wire/src/inner.rs', 'client/rust/agent-node-wire/src/inner_v2.rs', 'client/rust/agent-node-wire/src/lib.rs', 'client/rust/agent-node-wire/src/owner.rs', 'client/rust/agent-node-wire/src/owner_tests.rs', 'client/rust/agent-node-wire/src/reader_node_owner.rs', 'client/rust/agent-node-wire/src/reader_node_owner_tests.rs', 'client/rust/agent-node-wire/src/tunnel.rs', 'client/rust/agent-node-wire/src/tunnel_policy_v2_vector.json', 'client/rust/agent-node-wire/src/tunnel_policy_vector.json', 'client/rust/agent-node-wire/src/tunnel_tests.rs', 'client/rust/agent-node-wire/src/tunnel_v2.rs', 'client/rust/agent-node-wire/src/tunnel_v2_public_vectors.json', 'client/rust/agent-node-wire/src/tunnel_v2_tests.rs', 'client/rust/agent-node-wire/src/wire.rs', 'client/rust/crypto-core/Cargo.toml', 'client/rust/crypto-core/src/aead.rs', 'client/rust/crypto-core/src/agent_reader.rs', 'client/rust/crypto-core/src/agent_reader/issuer.rs', 'client/rust/crypto-core/src/agent_reader/personal_account_tests.rs', 'client/rust/crypto-core/src/agent_reader/personal_account_vectors.json', 'client/rust/crypto-core/src/agent_reader/signer.rs', 'client/rust/crypto-core/src/agent_reader/tests.rs', 'client/rust/crypto-core/src/agent_reader/validation.rs', 'client/rust/crypto-core/src/agent_reader/vectors.json', 'client/rust/crypto-core/src/agent_reader/wire.rs', 'client/rust/crypto-core/src/agent_reader_node_owner.rs', 'client/rust/crypto-core/src/agent_tunnel.rs', 'client/rust/crypto-core/src/automation.rs', 'client/rust/crypto-core/src/backup.rs', 'client/rust/crypto-core/src/device.rs', 'client/rust/crypto-core/src/envelope.rs', 'client/rust/crypto-core/src/error.rs', 'client/rust/crypto-core/src/kdf.rs', 'client/rust/crypto-core/src/keys.rs', 'client/rust/crypto-core/src/lib.rs', 'client/rust/crypto-core/src/local.rs', 'client/rust/crypto-core/src/object.rs', 'client/rust/crypto-core/src/portable_project.rs', 'client/rust/crypto-core/src/production_owner.rs', 'client/rust/crypto-core/src/production_owner/tests.rs', 'client/rust/crypto-core/src/recovery.rs', 'client/rust/crypto-core/src/rng.rs', 'client/rust/crypto-core/src/service_credential.rs', 'client/rust/crypto-core/src/service_credential_tests.rs', 'client/rust/crypto-core/src/service_delegation.rs', 'client/rust/crypto-core/src/service_delegation/tests.rs', 'client/rust/crypto-core/src/service_job.rs', 'client/rust/crypto-core/src/service_origin.rs', 'client/rust/crypto-core/src/sharing.rs', 'client/rust/crypto-core/src/sharing_enrollment.rs', 'client/rust/crypto-core/src/sharing_notes.rs', 'client/rust/crypto-core/src/sharing_verification.rs', 'client/rust/crypto-core/src/teams.rs', 'client/rust/crypto-core/src/vectors.rs', 'client/rust/crypto-core/tests/envelopes.rs', 'client/rust/crypto-core/tests/objects.rs', 'client/rust/crypto-core/tests/sharing.rs', 'client/rust/crypto-core/tests/sharing_enrollment.rs', 'client/rust/crypto-core/tests/vectors/backup.json', 'client/rust/crypto-core/tests/vectors/device.json', 'client/rust/crypto-core/tests/vectors/envelopes.json', 'client/rust/crypto-core/tests/vectors/key_hierarchy.json', 'client/rust/crypto-core/tests/vectors/key_trust.json', 'client/rust/crypto-core/tests/vectors/objects.json', 'client/rust/crypto-core/tests/vectors/recovery_key.json', 'client/rust/crypto-core/tests/vectors_public_api.rs', 'crates/Cargo.lock', 'crates/Cargo.toml', 'crates/models/Cargo.toml', 'crates/models/src/ai.rs', 'crates/models/src/automation.rs', 'crates/models/src/credential.rs', 'crates/models/src/database.rs', 'crates/models/src/group.rs', 'crates/models/src/history.rs', 'crates/models/src/host.rs', 'crates/models/src/known_host.rs', 'crates/models/src/kubernetes.rs', 'crates/models/src/lib.rs', 'crates/models/src/note.rs', 'crates/models/src/peer_pin.rs', 'crates/models/src/rdp.rs', 'crates/models/src/secret.rs', 'crates/models/src/settings.rs', 'crates/models/src/snippet.rs', 'crates/models/src/team.rs', 'crates/models/src/tunnel.rs', 'public-protocol/LICENSE', 'public-protocol/crates/Cargo.lock', 'public-protocol/crates/Cargo.toml', 'public-protocol/crates/protocol/Cargo.toml', 'public-protocol/crates/protocol/src/auth.rs', 'public-protocol/crates/protocol/src/bytes.rs', 'public-protocol/crates/protocol/src/canonical.rs', 'public-protocol/crates/protocol/src/devices.rs', 'public-protocol/crates/protocol/src/enterprise_auth.rs', 'public-protocol/crates/protocol/src/envelopes.rs', 'public-protocol/crates/protocol/src/error.rs', 'public-protocol/crates/protocol/src/events.rs', 'public-protocol/crates/protocol/src/ids.rs', 'public-protocol/crates/protocol/src/lib.rs', 'public-protocol/crates/protocol/src/limits.rs', 'public-protocol/crates/protocol/src/meta.rs', 'public-protocol/crates/protocol/src/paths.rs', 'public-protocol/crates/protocol/src/quotas.rs', 'public-protocol/crates/protocol/src/recovery.rs', 'public-protocol/crates/protocol/src/sharing.rs', 'public-protocol/crates/protocol/src/sharing_enrollment.rs', 'public-protocol/crates/protocol/src/sharing_notes.rs', 'public-protocol/crates/protocol/src/sync.rs', 'public-protocol/crates/protocol/src/teams.rs', 'public-protocol/crates/protocol/src/vaults.rs', 'public-protocol/crates/protocol/src/version.rs', 'public-protocol/crates/protocol/tests/sharing_capabilities.rs', 'public-protocol/crates/protocol/tests/sharing_contract.rs', 'public-protocol/crates/protocol/tests/sharing_enrollment_contract.rs')
# Original Native lock is a separately retained9c input, not one of119 source leaves.
SELECTED += ('client/rust/Cargo.lock',)
# Exact seven public Native leaves selected by reviewed union157; no directory glob.
SELECTED += (
    'client/rust/agent-node-wire/src/collector_service_owner.rs',
    'client/rust/agent-node-wire/src/wire/collector_credentials.rs',
    'client/rust/agent-node-wire/src/wire/collector_service.rs',
    'client/rust/crypto-core/src/agent_collector_bundle.rs',
    'client/rust/crypto-core/src/agent_collector_credentials.rs',
    'client/rust/crypto-core/src/agent_collector_credentials_tests.rs',
    'client/rust/crypto-core/src/collector_service.rs',
)

# Exact new public lifetime/ARM source names from immutable union191; no glob.
SELECTED += ('client/rust/agent-node-wire/src/wire/collector_service_credentials.rs', 'client/rust/crypto-core/src/collector_service_credential_frames.rs', 'client/rust/crypto-core/src/collector_service_credentials.rs', 'client/rust/crypto-core/src/collector_service_credentials_tests.rs', 'client/rust/crypto-core/src/service_credential_authenticated_metadata.rs')

# Exact fixed ExportersOnly public wire additions from reviewed managed-exporter packet60bf.
SELECTED += (
    'client/rust/agent-node-wire/src/exporter.rs',
    'client/rust/agent-node-wire/src/owner_exporter.rs',
)

def need(value, reason):
    if not value:
        raise ValueError(reason)


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


def enabled(agent):
    value = tomllib.loads((Path(agent)/'identity/Cargo.toml').read_text(encoding='utf-8'))['dependencies']
    present = {'cc-crypto-core', 'cc-agent-node-wire'} & value.keys()
    need(not present or len(present) == 2, 'both original Native dependencies required')
    for name in present:
        need(value[name] == {'path': '../../client/rust/' + name.removeprefix('cc-')}, 'fixed Native dependency path required')
    return bool(present)


def dirty_paths(agent):
    return (*ROOTS, *METADATA, 'LICENSE') if enabled(agent) else ()


def git(repo, *args):
    env = {n: os.environ[n] for n in ('PATH', 'SYSTEMROOT', 'WINDIR', 'TEMP', 'TMP') if n in os.environ}
    env.update(GIT_CONFIG_NOSYSTEM='1', GIT_CONFIG_GLOBAL=os.devnull, GIT_NO_REPLACE_OBJECTS='1', GIT_TERMINAL_PROMPT='0')
    r = subprocess.run(['git', '-c', 'safe.directory=' + str(repo), '-C', str(repo), *args], env=env,
                       stdin=subprocess.DEVNULL, capture_output=True, timeout=30, check=True)
    need(len(r.stdout) <= 32*1024**2 and len(r.stderr) <= 4096, 'public source Git output bound')
    return r.stdout


def selected_names(actual, expected):
    need(set(actual) == set(expected), 'omitted or unreviewed public dependency source')


def capture(agent, metadata, read):
    agent = Path(agent).resolve(); repo = agent.parent
    if not enabled(agent):
        return {'format': 'cc-agent-public-dependency-source/v1', 'mode': 'standalone', 'files': {}}
    need(metadata is not None, 'actual locked Cargo dependency metadata required')
    native_names = [n for n in SELECTED if not n.startswith('public-protocol/')]
    protocol_names = [n.removeprefix('public-protocol/') for n in SELECTED if n.startswith('public-protocol/')]
    need(git(repo, 'status', '--porcelain', '--untracked-files=all', '--', *dirty_paths(agent)).strip() == b'', 'dirty Native dependency source')
    listing = git(repo, 'ls-files', '-z', '--', *ROOTS, *METADATA).decode('utf-8').split('\0')
    selected_names([n for n in listing if n], native_names)
    local = {'cc-crypto-core': repo/'client/rust/crypto-core/Cargo.toml',
             'cc-agent-node-wire': repo/'client/rust/agent-node-wire/Cargo.toml',
             'cc-models': repo/'crates/models/Cargo.toml'}
    for name, path in local.items():
        rows = [p for p in metadata['packages'] if p['name'] == name]
        need(len(rows) == 1 and rows[0]['source'] is None and Path(rows[0]['manifest_path']) == path, 'actual compiled Native source path mismatch')
    rows = [p for p in metadata['packages'] if p['name'] == 'cc-protocol']
    need(len(rows) == 1 and rows[0]['source'] == GIT_SOURCE and rows[0]['version'] == '0.1.0', 'original exact protocol revision required')
    manifest = Path(rows[0]['manifest_path'])
    need(manifest.is_absolute() and manifest.parts[-3:] == ('crates', 'protocol', 'Cargo.toml'), 'protocol actual source path')
    protocol = manifest.parents[2]
    need(git(protocol, 'rev-parse', REV+'^{commit}').decode().strip() == REV, 'actual protocol commit')
    listing = git(protocol, 'ls-tree', '-r', '-z', REV, '--', 'LICENSE', 'crates/Cargo.toml', 'crates/Cargo.lock', 'crates/protocol')
    blobs = {}
    for entry in listing.split(b'\0'):
        if not entry: continue
        header, name = entry.split(b'\t', 1); mode, kind, blob = header.decode().split(); name = name.decode('utf-8')
        need(mode == '100644' and kind == 'blob' and name in protocol_names, 'protocol tracked regular source required')
        blobs[name] = blob
    selected_names(blobs, protocol_names)
    files = {}; total = 0
    for name in native_names + ['LICENSE']:
        raw = read(repo/name, 32*1024**2); total += len(raw)
        files[name] = {'sha256': sha(raw), 'bytes': len(raw)}
    for name in protocol_names:
        raw = read(protocol/name, 32*1024**2)
        need(raw == git(protocol, 'cat-file', 'blob', blobs[name]), 'compiled protocol checkout differs from pinned tracked blob')
        total += len(raw); files['public-protocol/'+name] = {'sha256': sha(raw), 'bytes': len(raw)}
    need(total <= 64*1024**2, 'public dependency source byte cap')
    need(git(repo, 'status', '--porcelain', '--untracked-files=all', '--', *dirty_paths(agent)).strip() == b'', 'Native source changed during capture')
    return {'format': 'cc-agent-public-dependency-source/v1', 'mode': 'exact Native and protocol tracked source',
            'protocolCommit': REV, 'files': files}


def notice_components(agent, descriptor, metadata, locked, read, notice):
    """Full public licenses from the SAME captured compiled dependency frame.

    This is source/byte provenance, not a signature or reproducible-build claim.
    No source path is accepted from a Cargo.lock Git URL or caller template.
    """
    agent = Path(agent).resolve()
    for item in locked:
        origin = item.get('source')
        if origin is not None and origin != 'registry+https://github.com/rust-lang/crates.io-index':
            need((item['name'], item['version'], origin) == ('cc-protocol', '0.1.0', GIT_SOURCE),
                 'unreviewed package source')
    if not enabled(agent):
        need(not any(p.get('source') == GIT_SOURCE for p in locked), 'compiled protocol dependency frame required')
        need(metadata is None, 'standalone notice must not borrow Native metadata')
        return []
    before, frame = descriptor.get('dependencySourceBefore'), descriptor.get('dependencySourceAfter')
    need(isinstance(frame, dict) and before == frame, 'original dependency source frame required')
    need(capture(agent, metadata, read) == frame, 'compiled dependency notice source changed')
    need(frame.get('protocolCommit') == REV, 'original exact protocol revision required')
    files = frame['files']; repo = agent.parent
    rows = [p for p in metadata['packages'] if p['name'] == 'cc-protocol']
    need(len(rows) == 1 and rows[0]['source'] == GIT_SOURCE and rows[0]['version'] == '0.1.0',
         'original exact protocol metadata required')
    protocol = Path(rows[0]['manifest_path']).parents[2]
    def body(name, limit=128*1024):
        need(name in files, 'full tracked dependency license/source missing')
        path = protocol/name.removeprefix('public-protocol/') if name.startswith('public-protocol/') else repo/name
        raw = read(path, limit); pin = files[name]
        need(len(raw) == pin['bytes'] and sha(raw) == pin['sha256'], 'dependency notice source changed')
        return raw
    def package(root, workspace, name, expected_source, license_name):
        raw = body(root+'/Cargo.toml'); meta = tomllib.loads(raw.decode('utf-8'))['package']
        inherited = tomllib.loads(body(workspace).decode('utf-8'))['workspace']['package']
        def field(key):
            value = meta[key]
            return inherited[key] if value == {'workspace': True} else value
        need(meta['name'] == name and field('license') == 'AGPL-3.0-only', 'actual dependency license identity')
        version = field('version')
        matches = [p for p in locked if p['name'] == name]
        need(len(matches) == 1 and matches[0]['version'] == version and matches[0].get('source') == expected_source,
             'actual dependency lock/source identity')
        license_raw = body(license_name)
        need(bool(license_raw.strip()), 'nonempty full dependency license required')
        return dict(name=name, version=version,
                    kind='rust-pinned-git-dependency' if expected_source else 'rust-local-path-dependency',
                    source=expected_source, source_frame_sha256=sha((json.dumps(frame,sort_keys=True,separators=(',',':'))+'\n').encode()),
                    source_file_sha256={n:p['sha256'] for n,p in files.items()
                                        if n.startswith(root+'/') or n in {workspace,license_name}},
                    scope='exact tracked compiled source; not ELF linkage or reproducibility certification',
                    license_declared='AGPL-3.0-only', notices=[notice(license_name, license_raw)])
    return [package('client/rust/crypto-core','client/rust/Cargo.toml','cc-crypto-core',None,'LICENSE'),
            package('client/rust/agent-node-wire','client/rust/Cargo.toml','cc-agent-node-wire',None,'LICENSE'),
            package('crates/models','crates/Cargo.toml','cc-models',None,'LICENSE'),
            package('public-protocol/crates/protocol','public-protocol/crates/Cargo.toml','cc-protocol',GIT_SOURCE,'public-protocol/LICENSE')]


def archive(work):
    # Called before the CI signing step. Missing/mismatched source fails before
    # any dedicated release key is used. The existing Agent archive is retained.
    import notice_generate
    agent = Path(__file__).resolve().parent.parent; repo = agent.parent
    work = Path(work); need(work.is_absolute() and work.is_dir() and not work.is_symlink(), 'owned release work required')
    descriptor = json.loads(notice_generate.read(work/'build-public.json', 8*1024**2))
    expected = descriptor['dependencySourceAfter']
    metadata = json.loads(notice_generate.read(work/'dependency-metadata-after.json', 8*1024**2)) if enabled(agent) else None
    need(capture(agent, metadata, notice_generate.read) == expected, 'original build dependency source changed before archive')
    output = work/'source.tar'; need(not output.exists() and not output.is_symlink(), 'new source archive required')
    git(repo, 'archive', '--format=tar', '--prefix=consolecrypt-agent-source/', '--output='+str(output), 'HEAD', 'agent', 'LICENSE')
    protocol = Path(next(p['manifest_path'] for p in metadata['packages'] if p['name']=='cc-protocol')).parents[2] if metadata else None
    with tarfile.open(output, 'r:') as original:
        names = {m.name for m in original.getmembers()}
        agent_prefix='consolecrypt-agent-source/agent/'
        agent_files={m.name[len(agent_prefix):]:m for m in original.getmembers()
                     if m.name.startswith(agent_prefix) and m.isfile()}
        need(set(agent_files)==set(descriptor['sourceAfter']), 'source archive omitted or unreviewed Agent source')
        for name, member in agent_files.items():
            need(member.size<=32*1024**2, 'source archive Agent file bound')
            raw=original.extractfile(member).read(32*1024**2+1)
            need(len(raw)==member.size and sha(raw)==descriptor['sourceAfter'][name], 'source archive differs from actual compiled Agent bytes')
        if 'LICENSE' in expected['files']:
            pin = expected['files']['LICENSE']; raw = original.extractfile('consolecrypt-agent-source/LICENSE').read(32*1024**2+1)
            need(len(raw) == pin['bytes'] and sha(raw) == pin['sha256'], 'original repository license missing')
    with tarfile.open(output, 'a') as target:
        for name, pin in sorted(expected['files'].items()):
            if name == 'LICENSE':
                continue
            source = protocol/name.removeprefix('public-protocol/') if name.startswith('public-protocol/') else repo/name
            raw = notice_generate.read(source,32*1024**2); need(len(raw)==pin['bytes'] and sha(raw)==pin['sha256'], 'source archive dependency bytes changed')
            path = 'consolecrypt-agent-source/'+name; need(path not in names, 'source archive collision')
            row=tarfile.TarInfo(path); row.mode=0o644; row.mtime=0; row.size=len(raw)
            target.addfile(row,io.BytesIO(raw)); names.add(path)
    need(capture(agent,metadata,notice_generate.read)==expected,'dependency source changed during archive')
    receipt={'format':'cc-agent-public-source-archive/v1','sourceArchiveSha256':sha(output.read_bytes()),
             'dependencySource':expected,'productionLegalCertification':False}
    with (work/'source-public.json').open('x',encoding='utf-8') as f: json.dump(receipt,f,sort_keys=True,indent=2)


if __name__ == '__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('operation',choices=['archive']); parser.add_argument('--work',type=Path,required=True)
    args=parser.parse_args(); archive(args.work)
