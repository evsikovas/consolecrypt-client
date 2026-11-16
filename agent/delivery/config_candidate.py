"""Public immutable configuration candidates. No live selector or authority."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import stat
import sys
import tempfile
import uuid

PROFILE_MAX = 32768
RUNTIME_MAX = 65536
MANIFEST_MAX = 16384
TOTAL_MAX = 2 * 1024 * 1024
MAX_CANDIDATES = 8
MARKER = '.cc-config-root.json'
LEDGER = '.cc-config-candidates.json'
HEX = re.compile('[0-9a-f]{64}\\Z')


class CandidateError(ValueError): pass
class CandidateUnknown(CandidateError): pass

INPUT_CODES = {'duplicate_json','invalid_number','unsafe_path','linked_path','invalid_identity',
    'profile_budget','identity_mismatch','nullable_agent_mode','derived_budget','invalid_options',
    'unsupported_platform','invalid_protected_roots','profile_input_in_protected_root',
    'profile_input_budget_or_kind','profile_input_changed'}
ROOT_CODES = {'unsafe_path','linked_path','physical_root_required','private_owned_root_required',
    'protected_root_overlap','root_not_initialized'}
INVENTORY_CODES = {'unsafe_path','linked_path','file_budget_or_kind','file_changed','file_mode_or_owner',
    'directory_kind','directory_mode_or_owner','duplicate_json','invalid_number','root_binding_mismatch',
    'invalid_ledger','ledger_budget','total_budget'}
QUARANTINE_CODES = {'directory_entry_budget','candidate_inventory_quarantine','root_inventory_quarantine',
    'candidate_modified','candidate_manifest','candidate_binding'}
MODEL_CODES = {
    ('integration.profile_manifest','ProfileError'): {
        'unsupported_profile_fields','invalid_profile_identity','invalid_profile_name','invalid_backend_url',
        'insecure_backend_denied','profile_size_limit','unsupported_profile_format','public_key_fingerprint_required',
        'fingerprint_without_agent','unsupported_collection_mode','agent_identity_required','unsupported_base_profile',
        'unexpected_delivery_buffer','exporter_count_limit','invalid_exporter_enabled','invalid_exporter_ownership',
        'unsupported_exporter_kind','invalid_exporter_url','duplicate_exporter_denied','external_exporter_lifecycle_denied',
        'pinned_managed_exporter_required','connection_only_cannot_collect','enabled_exporter_required',
        'linux_basic_requires_one_explicit_node_exporter','unexpected_backend','runtime_bind_must_be_string','agent_not_requested'},
    ('cc_agent.manifest','ManifestError'): {
        'unsupported manifest schema or fields','invalid agent_id','noncanonical agent_id','invalid host_id',
        'noncanonical host_id','numeric loopback bind required','this prototype supports IPv4 loopback only',
        'invalid history configuration','invalid heartbeat_resource_id','noncanonical heartbeat_resource_id',
        'invalid history_resource_id','noncanonical history_resource_id','endpoint resources must differ',
        'invalid port','invalid sample interval','invalid history bytes','invalid history age','invalid history points'},
    ('collector.manager','CollectorError'): {
        'invalid_object','unsupported_field','invalid_budget','invalid_label','invalid_endpoint','literal_ip_required',
        'endpoint_zone_id_denied','endpoint_secret_or_query_denied','endpoint_address_denied','insecure_non_loopback_denied',
        'endpoint_path_denied','endpoint_noncanonical','unsupported_schema','scope_denied','unsupported_mode',
        'invalid_host_id','host_binding_denied','target_limit','endpoint_not_authorized','duplicate_collection_denied',
        'unsupported_exporter','receiver_not_authorized','pull_remote_write_conflict'}}


def known_model_error(error, code):
    for (module, name), codes in MODEL_CODES.items():
        cls = getattr(sys.modules.get(module), name, None)
        if cls is not None and type(error) is cls and code in codes:return True
    return False


class Attempt:
    """Internal observation only; never supplied by profile/HTTP/role config."""
    def __init__(self):
        self.phase = 'INPUT'
        self.mutation_attempted = False


def public_failure(error, attempt):
    reason = 'internal_unknown'
    code = error.args[0] if len(error.args) == 1 and type(error.args[0]) is str else None
    if isinstance(error, CandidateUnknown):
        reason = {'root_initialization_unknown':'initialization_unknown',
            'published_candidate_unknown':'publication_unknown',
            'temporary_file_changed':'temporary_cleanup_unknown',
            'unpublished_cleanup_unknown':'temporary_cleanup_unknown',
            'ledger_temporary_cleanup_unknown':'temporary_cleanup_unknown'}.get(
                code, 'inventory_unknown' if code in QUARANTINE_CODES else 'internal_unknown')
    elif attempt.mutation_attempted:
        reason = {'INIT':'initialization_unknown','PUBLISH':'publication_unknown',
            'CLEANUP':'temporary_cleanup_unknown'}.get(attempt.phase,
                'io_unknown' if isinstance(error, OSError) else 'internal_unknown')
    elif attempt.phase == 'INVENTORY':
        if ((type(error) is CandidateError and code in INVENTORY_CODES)
                or isinstance(error, (json.JSONDecodeError, UnicodeDecodeError, OSError))):reason = 'inventory_unknown'
    elif attempt.phase == 'INPUT':
        if ((type(error) is CandidateError and code in INPUT_CODES)
                or isinstance(error, (json.JSONDecodeError, UnicodeDecodeError)) or known_model_error(error, code)):
            reason = 'input_rejected'
    elif attempt.phase == 'ROOT_PRECONDITION':
        if type(error) is CandidateError and code in ROOT_CODES:reason = 'root_rejected'
        elif type(error) is CandidateError and code == 'candidate_budget':reason = 'candidate_budget'
        elif isinstance(error, OSError):reason = 'io_unknown'
    rejected = reason in {'input_rejected','root_rejected','candidate_budget'}
    return {'format':'cc-config-candidate-error/v1','outcome':'rejected' if rejected else 'unknown',
        'reason':reason,'change_state':'not_changed' if rejected else 'unknown','changed':False if rejected else None}


def canonical(value): return (json.dumps(value, sort_keys=True, separators=(',', ':')) + '\n').encode()
def digest(raw): return hashlib.sha256(raw).hexdigest()


def strict(raw):
    def unique(pairs):
        result = {}
        for key, value in pairs:
            if key in result: raise CandidateError('duplicate_json')
            result[key] = value
        return result
    return json.loads(raw, object_pairs_hook=unique, parse_constant=lambda value: (_ for _ in ()).throw(CandidateError('invalid_number')))


def safe(path):
    path = Path(path)
    if not path.is_absolute() or '..' in path.parts or len(path.parts) < 3: raise CandidateError('unsafe_path')
    for item in (path, *path.parents):
        try: info = item.lstat()
        except FileNotFoundError: continue
        if stat.S_ISLNK(info.st_mode) or getattr(info, 'st_file_attributes', 0) & 1024:
            raise CandidateError('linked_path')
    return path


def read(path, limit):
    path = safe(path); before = path.lstat()
    if not stat.S_ISREG(before.st_mode) or before.st_nlink != 1 or before.st_size > limit:
        raise CandidateError('file_budget_or_kind')
    with os.fdopen(os.open(path, os.O_RDONLY | getattr(os, 'O_NOFOLLOW', 0)), 'rb') as stream:
        raw = stream.read(limit + 1); after = os.fstat(stream.fileno())
    attr = lambda s: (s.st_dev, s.st_ino, s.st_size, s.st_mtime_ns, s.st_ctime_ns)
    if len(raw) > limit or attr(before) != attr(after) or attr(after) != attr(path.lstat()):
        raise CandidateError('file_changed')
    if os.name != 'nt' and (stat.S_IMODE(after.st_mode) != 0o600 or after.st_uid != os.getuid()): raise CandidateError('file_mode_or_owner')
    return raw


def sync_directory(path):
    if os.name == 'nt': return  # Portable logic tests only; CLI writes are Linux-only.
    fd = os.open(safe(path), os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
    try: os.fsync(fd)
    finally: os.close(fd)


def write(path, raw):
    path = safe(path)
    fd = os.open(path, os.O_CREAT | os.O_EXCL | os.O_WRONLY | getattr(os, 'O_NOFOLLOW', 0), 0o600)
    with os.fdopen(fd, 'wb') as stream:
        stream.write(raw); stream.flush(); os.fsync(stream.fileno())


def owner_uid():
    # Windows runs portable logic tests only; there is no POSIX owner UID there.
    return os.getuid() if os.name != 'nt' else -1


def owned_file(path, raw, identity=None):
    actual = read(path, len(raw))
    info = path.lstat()
    if actual != raw or (identity is not None and (info.st_dev, info.st_ino) != identity):
        raise CandidateUnknown('temporary_file_changed')
    return (info.st_dev, info.st_ino)


def cleanup_unpublished(staging, directory_identity, files):
    # External single writer required. Validate the WHOLE map before any unlink;
    # no recursive removal, replacement adoption or partial-byte guessing.
    private_directory(staging)
    info = staging.lstat()
    if (info.st_dev, info.st_ino) != directory_identity or names(staging, 3) != set(files):
        raise CandidateUnknown('unpublished_cleanup_unknown')
    for name, (raw, identity) in files.items(): owned_file(staging / name, raw, identity)
    for name, (raw, identity) in files.items():
        owned_file(staging / name, raw, identity)
        (staging / name).unlink()
    staging.rmdir()


def atomic(path, raw):
    fd, temporary = tempfile.mkstemp(prefix='.candidate-ledger-', dir=path.parent)
    temporary = Path(temporary)
    created = os.fstat(fd); identity = (created.st_dev, created.st_ino)
    try:
        with os.fdopen(fd, 'wb') as stream:
            stream.write(raw); stream.flush(); os.fsync(stream.fileno())
        os.chmod(temporary, 0o600); os.replace(temporary, path); sync_directory(path.parent)
    finally:
        try: temporary.lstat()
        except FileNotFoundError: pass
        else:
            # A partial/foreign file is quarantined, not silently unlinked.
            try: owned_file(temporary, raw, identity)
            except Exception as error: raise CandidateUnknown('ledger_temporary_cleanup_unknown') from error
            temporary.unlink()


def identifier(value, nullable=False):
    if nullable and value is None: return value
    if type(value) is not str: raise CandidateError('invalid_identity')
    try: parsed = str(uuid.UUID(value))
    except ValueError: raise CandidateError('invalid_identity') from None
    if parsed != value or parsed == str(uuid.UUID(int=0)): raise CandidateError('invalid_identity')
    return value


def root_directory(root, protected):
    root = safe(root); info = root.lstat()
    if not stat.S_ISDIR(info.st_mode): raise CandidateError('physical_root_required')
    if os.name != 'nt' and (info.st_uid != os.getuid() or stat.S_IMODE(info.st_mode) != 0o700):
        raise CandidateError('private_owned_root_required')
    for item in protected:
        item = safe(item)
        if root == item or root in item.parents or item in root.parents or (item.exists() and os.path.samefile(root, item)):
            raise CandidateError('protected_root_overlap')
    return root, info


def private_directory(path):
    info = safe(path).lstat()
    if not stat.S_ISDIR(info.st_mode): raise CandidateError('directory_kind')
    if os.name != 'nt' and (stat.S_IMODE(info.st_mode) != 0o700 or info.st_uid != os.getuid()):
        raise CandidateError('directory_mode_or_owner')


def names(path, limit):
    result = []
    with os.scandir(path) as entries:
        for item in entries:
            result.append(item.name)
            if len(result) > limit: raise CandidateUnknown('directory_entry_budget')
    return set(result)


def render(raw, agent_id, host_id):
    if type(raw) is not bytes or len(raw) > PROFILE_MAX: raise CandidateError('profile_budget')
    from integration.profile_manifest import ProfileManifest
    profile = ProfileManifest.from_dict(strict(raw))
    data = profile.data; runtime = profile.runtime_manifest() if profile.plan()['agent_required'] else None
    identifier(host_id); identifier(agent_id, nullable=True)
    actual = data['identity'].get('agent_id')
    if data['identity']['host_id'] != host_id or actual != agent_id: raise CandidateError('identity_mismatch')
    if agent_id is None and not (data['collection']['mode'] == 'existing_prometheus' and runtime is None and not profile.plan()['agent_required']):
        raise CandidateError('nullable_agent_mode')
    payload = {'profile.json': canonical(data), 'runtime.json': canonical(runtime)}
    if len(payload['profile.json']) > PROFILE_MAX or len(payload['runtime.json']) > RUNTIME_MAX:
        raise CandidateError('derived_budget')
    return payload, profile.digest


def marker(root, info, agent_id, host_id):
    value = strict(read(root / MARKER, MANIFEST_MAX))
    expected = {'format', 'path', 'dev', 'ino', 'uid', 'instance', 'agent_id', 'host_id'}
    if (type(value) is not dict or set(value) != expected or value['format'] != 'cc-config-root/v1'
            or value['path'] != str(root) or value['dev'] != info.st_dev or value['ino'] != info.st_ino
            or type(value['uid']) is not int or value['uid'] != owner_uid()
            or (os.name != 'nt' and value['uid'] != info.st_uid)
            or type(value['instance']) is not str or not re.fullmatch('[0-9a-f]{32}', value['instance'])
            or value['agent_id'] != agent_id or value['host_id'] != host_id):
        raise CandidateError('root_binding_mismatch')
    return value


def inventory(root, binding):
    value = strict(read(root / LEDGER, MANIFEST_MAX))
    if type(value) is not dict or set(value) != {'format', 'instance', 'entries'} or value['format'] != 'cc-config-ledger/v1' or value['instance'] != binding['instance']:
        raise CandidateError('invalid_ledger')
    entries = value['entries']
    if type(entries) is not dict or len(entries) > MAX_CANDIDATES or any(not HEX.fullmatch(k) or type(v) is not str or not HEX.fullmatch(v) for k,v in entries.items()):
        raise CandidateError('ledger_budget')
    directory = safe(root / 'candidates')
    private_directory(directory)
    if names(directory, MAX_CANDIDATES) != set(entries):
        raise CandidateUnknown('candidate_inventory_quarantine')
    if names(root, 3) != {MARKER, LEDGER, 'candidates'}:
        raise CandidateUnknown('root_inventory_quarantine')
    total = 0
    for name, expected in entries.items():
        candidate = safe(directory / name)
        private_directory(candidate)
        if names(candidate, 3) != {'profile.json', 'runtime.json', 'manifest.json'}:
            raise CandidateUnknown('candidate_inventory_quarantine')
        manifest_raw = read(candidate / 'manifest.json', MANIFEST_MAX); manifest = strict(manifest_raw)
        if digest(manifest_raw) != expected or type(manifest) is not dict:
            raise CandidateUnknown('candidate_modified')
        if set(manifest) != {'format','instance','agent_id','host_id','profile_digest','payloads','validator_bundle_sha256'}:
            raise CandidateUnknown('candidate_manifest')
        if (manifest['format'] != 'cc-config-candidate/v1' or digest(canonical(manifest)) != name
                or manifest['instance'] != binding['instance'] or manifest['agent_id'] != binding['agent_id']
                or manifest['host_id'] != binding['host_id'] or set(manifest['payloads']) != {'profile.json','runtime.json'}):
            raise CandidateUnknown('candidate_binding')
        total += len(manifest_raw)
        for file, limit in (('profile.json',PROFILE_MAX),('runtime.json',RUNTIME_MAX)):
            raw = read(candidate / file, limit); total += len(raw)
            if digest(raw) != manifest['payloads'][file]: raise CandidateUnknown('candidate_modified')
    if total > TOTAL_MAX: raise CandidateError('total_budget')
    return value, total


def apply(root, raw, *, agent_id, host_id, validator_sha256, protected=(), initialize=False, check=False, _attempt=None):
    attempt = _attempt if _attempt is not None else Attempt()
    attempt.phase = 'INPUT'
    if type(initialize) is not bool or type(check) is not bool or type(validator_sha256) is not str or not HEX.fullmatch(validator_sha256):
        raise CandidateError('invalid_options')
    payload, profile_digest = render(raw, agent_id, host_id)
    attempt.phase = 'ROOT_PRECONDITION'
    root, info = root_directory(root, protected)
    attempt.phase = 'INVENTORY'
    try:(root / MARKER).lstat(); marker_present = True
    except FileNotFoundError:marker_present = False
    if not marker_present:
        if any(root.iterdir()):
            attempt.phase = 'INVENTORY'
            raise CandidateUnknown('root_inventory_quarantine')
        if not initialize:
            attempt.phase = 'ROOT_PRECONDITION'
            raise CandidateError('root_not_initialized')
        if check:
            return {'changed':True,'would_init':True,'would_create':True,'root_instance':None,'candidate_digest':None,
                'candidate_path':None,'profile_digest':profile_digest,'runtime_digest':digest(payload['runtime.json'])}
        binding = {'format':'cc-config-root/v1','path':str(root),'dev':info.st_dev,'ino':info.st_ino,
            'uid':owner_uid(),'instance':uuid.uuid4().hex,'agent_id':agent_id,'host_id':host_id}
        attempt.phase = 'INIT'; attempt.mutation_attempted = True
        try:
            write(root / MARKER, canonical(binding)); (root / 'candidates').mkdir(mode=0o700)
            write(root / LEDGER, canonical({'format':'cc-config-ledger/v1','instance':binding['instance'],'entries':{}}))
            sync_directory(root)
        except Exception as error: raise CandidateUnknown('root_initialization_unknown') from error
    attempt.phase = 'INVENTORY'
    binding = marker(root, info, agent_id, host_id); ledger, total = inventory(root, binding)
    manifest = {'format':'cc-config-candidate/v1','instance':binding['instance'],'agent_id':agent_id,'host_id':host_id,
        'profile_digest':profile_digest,'payloads':{n:digest(v) for n,v in payload.items()},'validator_bundle_sha256':validator_sha256}
    manifest_raw = canonical(manifest); candidate = digest(manifest_raw)
    result = {'changed':candidate not in ledger['entries'],'would_init':False,'would_create':candidate not in ledger['entries'],
        'root_instance':binding['instance'],'candidate_digest':candidate,'candidate_path':str(root/'candidates'/candidate),
        'profile_digest':profile_digest,'runtime_digest':digest(payload['runtime.json'])}
    if candidate in ledger['entries']: return result
    attempt.phase = 'ROOT_PRECONDITION'
    if len(ledger['entries']) >= MAX_CANDIDATES or total + len(manifest_raw) + sum(map(len,payload.values())) > TOTAL_MAX:
        raise CandidateError('candidate_budget')
    if check: return result
    target = root / 'candidates' / candidate
    attempt.phase = 'UNPUBLISHED'; attempt.mutation_attempted = True
    staging = Path(tempfile.mkdtemp(prefix='.candidate-tmp-', dir=root)); staging.chmod(0o700)
    info = staging.lstat(); directory_identity = (info.st_dev, info.st_ino)
    files = {}; published = False
    try:
        for name, value in {**payload,'manifest.json':manifest_raw}.items():
            write(staging / name, value)
            files[name] = (value, owned_file(staging / name, value))
        sync_directory(staging)
        # Never automatically remove a published candidate, even on ordinary error.
        try:
            attempt.phase = 'PUBLISH'
            os.rename(staging, target); published = True; sync_directory(target.parent)
            ledger['entries'][candidate] = digest(manifest_raw)
            atomic(root / LEDGER, canonical(ledger)); inventory(root, binding)
        except Exception as error: raise CandidateUnknown('published_candidate_unknown') from error
    except Exception:
        if not published:
            previous_phase = attempt.phase; attempt.phase = 'CLEANUP'
            try: cleanup_unpublished(staging, directory_identity, files)
            except Exception as error: raise CandidateUnknown('unpublished_cleanup_unknown') from error
            attempt.phase = previous_phase
        raise
    return result


def main(argv=None, *, validator_sha256=None, _attempt=None):
    if sys.platform != 'linux': raise CandidateError('unsupported_platform')
    class SafeParser(argparse.ArgumentParser):
        def error(self, message): raise CandidateError('invalid_options')
    parser = SafeParser(add_help=False)
    parser.add_argument('--root',required=True,type=Path); parser.add_argument('--profile',required=True,type=Path)
    parser.add_argument('--expected-agent-id',required=True); parser.add_argument('--expected-host-id',required=True)
    parser.add_argument('--protected-roots',required=True); parser.add_argument('--initialize',action='store_true')
    parser.add_argument('--check',action='store_true')
    args=parser.parse_args(argv)
    agent_id=None if args.expected_agent_id=='none' else args.expected_agent_id
    protected=strict(args.protected_roots)
    if type(protected) is not list or not all(type(p) is str for p in protected): raise CandidateError('invalid_protected_roots')
    source=safe(args.profile)
    for protected_root in protected:
        boundary=safe(protected_root)
        if source==boundary or boundary in source.parents: raise CandidateError('profile_input_in_protected_root')
    info=source.lstat()
    if not stat.S_ISREG(info.st_mode) or info.st_nlink!=1 or info.st_size>PROFILE_MAX: raise CandidateError('profile_input_budget_or_kind')
    with os.fdopen(os.open(source,os.O_RDONLY|getattr(os,'O_NOFOLLOW',0)),'rb') as stream:
        opened=os.fstat(stream.fileno())
        if (opened.st_dev,opened.st_ino)!=(info.st_dev,info.st_ino): raise CandidateError('profile_input_changed')
        raw=stream.read(PROFILE_MAX+1)
    return apply(args.root,raw,agent_id=agent_id,host_id=args.expected_host_id,
        validator_sha256=validator_sha256,protected=protected,initialize=args.initialize,check=args.check,_attempt=_attempt)


if __name__ == '__main__':
    raise SystemExit('use_separately_pinned_candidate_bootstrap')
