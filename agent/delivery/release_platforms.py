"""Two fixed Linux release targets; selection and integrity are not OS acceptance.

Checksums below are existing reviewed supply-lock facts. No new download,
signer, runtime capability or ARM binary/material observation is implied.
"""
import hashlib
import platform as host_platform
import re
import sys

VERSION = 'v1.156.0'
MAX_DISK_BINARY = 192 * 1024 * 1024
MAX_COLLECTOR = 64 * 1024 * 1024  # Existing expanded package ceiling, never raised.
_ROWS = (
    ('x86_64-unknown-linux-gnu', 'linux-x86_64', 'linux-amd64', 'x86_64', 'amd64', 62,
     'vmutils-linux-amd64-v1.156.0.tar.gz',
     'd71f7f00e07782531000620ec6c2a9d7ec83324f364dd1f36095ab378b5fb43b',
     '2039bc05b6ad5c96b34b72801908896bf8a6a7bcab8ae32120cbb8e6217ef6f6', 23209584),
    ('aarch64-unknown-linux-gnu', 'linux-aarch64', 'linux-arm64', 'aarch64', 'arm64', 183,
     'vmutils-linux-arm64-v1.156.0.tar.gz',
     '1a8e4ed55df2a59d5cd03056213f50d293d4fab029e38dd960597362d92cf176',
     '564346d8acd0979b62c2671ad586d63b243a925307bfa54d0d5f94bd50dda4f5', 21299768),
)
_KEYS = ('target', 'release', 'collector', 'machine', 'goarch', 'elfMachine',
         'archive', 'archiveSha256', 'binarySha256', 'observedBytes')


def select(value, domain):
    if domain not in {'target', 'release', 'collector'}:
        raise ValueError('fixed Linux platform domain required')
    rows = [dict(zip(_KEYS, row)) for row in _ROWS]
    found = [row for row in rows if row[domain] == value]
    if len(found) != 1:
        raise ValueError('unsupported Linux release platform')
    return found[0]


def _fixed(row):
    if row != select(row.get("target"), "target"):
        raise ValueError("fixed Linux platform record required")


def require_native(row):
    _fixed(row)
    if sys.platform != 'linux' or host_platform.machine() != row['machine']:
        raise ValueError('native matching Linux release host required')


def verified_release(value):
    """Closed DATA copied only after the existing real release verifier ACK.

    Parsing this public object is not independent signature verification.
    The installer supplies the original verifier result, never request JSON.
    """
    keys={'version','displayVersion','protocol','platform','artifactSha256','artifactBytes'}
    if (type(value) is not dict or set(value)!=keys
            or type(value['version']) is not int or not 1<=value['version']<=2**64-1
            or type(value['protocol']) is not int or value['protocol']!=1
            or type(value['displayVersion']) is not str
            or not re.fullmatch(r'[0-9]{1,6}\.[0-9]{1,6}\.[0-9]{1,6}',value['displayVersion'])
            or type(value['artifactSha256']) is not str
            or not re.fullmatch('[0-9a-f]{64}',value['artifactSha256'])
            or type(value['artifactBytes']) is not int or not 0<value['artifactBytes']<=96*1024**2):
        raise ValueError('original verified release metadata required')
    select(value['platform'],'release')
    return dict(value)


def installed_platform(current, record):
    """No legacy/global-anchor fallback for architecture-bound runtime.

    Both inputs are read by the fixed caller with original root-owned FD
    custody. The copied per-release attestation also supports an explicitly
    selected older release without borrowing the latest replay-floor anchor.
    """
    if type(record) is not dict or record.get('format')!='cc-agent-installed-release/v2':
        raise ValueError('architecture-bound installed release required')
    value=verified_release(record.get('verified_release'))
    if (type(current) is not dict or value['displayVersion']!=record.get('version')
            or value['displayVersion']!=current.get('version')
            or value['artifactSha256']!=record.get('artifact_sha256')
            or value['artifactSha256']!=current.get('artifact_sha256')):
        raise ValueError('selected verified release metadata changed')
    row=select(value['platform'],'release')
    require_native(row)
    return row


def elf_platform(raw):
    if len(raw) < 64 or raw[:7] != b'\x7fELF\x02\x01\x01':
        raise ValueError('Linux ELF64 little-endian input required')
    machine = int.from_bytes(raw[18:20], 'little')
    rows = [dict(zip(_KEYS, row)) for row in _ROWS if row[5] == machine]
    if len(rows) != 1:
        raise ValueError('unsupported Linux ELF machine')
    return rows[0]


def require_elf(raw, row):
    _fixed(row)
    if elf_platform(raw)['target'] != row['target']:
        raise ValueError('cross-component ELF architecture mismatch')


def supply_entry(lock, row):
    _fixed(row)
    collector = lock['collector']
    entry = collector['artifacts'][row['collector']]
    if (collector['name'] != 'vmagent' or collector['version'] != VERSION
            or entry != {'archive': row['archive'], 'archive_sha256': row['archiveSha256'],
                         'binary': 'vmagent-prod', 'binary_sha256': row['binarySha256']}):
        raise ValueError('fixed reviewed collector supply tuple changed')
    return entry


def collector_payload(raw, row):
    _fixed(row)
    require_elf(raw, row)
    if (not 64 <= len(raw) <= MAX_COLLECTOR
            or (row['observedBytes'] is not None and len(raw) != row['observedBytes'])
            or hashlib.sha256(raw).hexdigest() != row['binarySha256']):
        raise ValueError('original pinned collector bytes unavailable')
    return raw


def go_build_platform(info, row):
    _fixed(row)
    settings = info.get('Settings')
    if not isinstance(settings, list):
        raise ValueError('actual Go platform settings required')
    selected = {}
    for item in settings:
        if not isinstance(item, dict) or not isinstance(item.get('Key'), str):
            raise ValueError('invalid actual Go platform settings')
        key = item['Key']
        if key in selected:
            raise ValueError('duplicate actual Go platform setting')
        selected[key] = item.get('Value')
    if selected.get('GOOS') != 'linux' or selected.get('GOARCH') != row['goarch']:
        raise ValueError('actual Go build architecture mismatch')


def material_identity(value, row):
    _fixed(row)
    # Full canonical collector_materials.checked must still validate every
    # source/module/stdlib notice. This function is only the platform fence.
    if value.get('binarySha256') != row['binarySha256']:
        raise ValueError('cross-platform collector materials denied')
    size = value.get('binaryBytes')
    if (type(size) is not int or not 64 <= size <= MAX_COLLECTOR
            or (row['observedBytes'] is not None and size != row['observedBytes'])):
        raise ValueError('actual collector size unavailable')
    go_build_platform(value['buildInfo'], row)


def volatile_runtime_metadata(binary, materials_raw):
    """DATA for a future full v2 manifest, never an admission or OS-ready flag.

    Derive the fixed platform from actual pinned ELF bytes. Call the SAME
    canonical complete notice checker; single project LICENSE or metadata
    copied from another architecture is insufficient. Qualified ARM parser
    records retain that explicit provenance; this function grants no readiness.
    """
    row=elf_platform(binary)
    collector_payload(binary,row)
    if materials_raw is None:
        raise ValueError('complete actual collector materials unavailable')
    import collector_materials
    value=collector_materials.checked(materials_raw,row['binarySha256'])
    material_identity(value,row)
    return dict(kind='vmagent',upstreamTag=VERSION,platform=row['release'],
                binarySha256=row['binarySha256'],binaryBytes=len(binary))
