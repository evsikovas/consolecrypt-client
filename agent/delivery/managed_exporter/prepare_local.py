"""Explicit offline administrator preparation; never run by Agent/Native.

Reuse the delivery notice-inventory codec. Archive and normal worker are real
release inputs, not fixture pins. This verifies integrity, not Go provenance or
legal certification; reviewed release CI must supply the complete materials.
No download, binary execution, host mutation, credentials or signing.
"""
import argparse
import hashlib
import io
import json
import os
from pathlib import Path
import re
import stat
import sys
import tarfile
import uuid

# Deliberately the existing codec in the same selected delivery composition.
sys.path.insert(0, str(Path(__file__).absolute().parent.parent))
sys.path.insert(0, str(Path(__file__).absolute().parent))
from notice_inventory import validate_notice_inventory

PINS = {
    'linux-x86_64': ('amd64', 62, 'b51d8a76aa2a9156a55d501aca6276fae09e262259a5e4e831d2c2222f084e63'),
    'linux-aarch64': ('arm64', 183, 'ad35b605f9954b9f1ffddf5ba054bdc5a98d790b9eae5291e1eeb83f1ecbd0e7'),
}


def need(value, code):
    if not value:
        raise ValueError(code)


def digest(raw):
    return hashlib.sha256(raw).hexdigest()


def unique(rows):
    result = {}
    for key, item in rows:
        need(key not in result, 'duplicate JSON key')
        result[key] = item
    return result


def read(path, cap):
    path = Path(path)
    need(path.is_absolute(), 'absolute input required')
    for parent in path.parents:
        need(stat.S_ISDIR(parent.lstat().st_mode), 'physical input ancestry required')
    fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK | os.O_CLOEXEC)
    try:
        before = os.fstat(fd)
        need(stat.S_ISREG(before.st_mode) and before.st_nlink == 1 and 0 < before.st_size <= cap,
             'bounded singlelink input required')
        raw = bytearray()
        while len(raw) <= cap:
            part = os.read(fd, min(65536, cap+1-len(raw)))
            if not part:
                break
            raw.extend(part)
        ident = lambda m: (m.st_dev, m.st_ino, m.st_size, m.st_mode, m.st_mtime_ns, m.st_ctime_ns)
        need(len(raw) == before.st_size and ident(before) == ident(os.fstat(fd)) == ident(path.lstat()),
             'stable complete input required')
        return bytes(raw)
    finally:
        os.close(fd)


def elf(raw, machine):
    need(64 <= len(raw) <= 64*1024*1024 and raw[:6] == b'\x7fELF\x02\x01'
         and int.from_bytes(raw[18:20], 'little') == machine, 'matching normal Linux ELF required')


def prepare(args):
    for value in (args.host_id, args.agent_id):
        need(str(uuid.UUID(value)) == value and uuid.UUID(value).int != 0, 'canonical nonzero UUID required')
    need(30 <= args.max_seconds <= 300, 'finite policy budget required')
    arch, machine, archive_pin = PINS[args.platform]
    archive = read(args.archive, 64*1024*1024)
    need(digest(archive) == archive_pin, 'exact approved official archive required')
    prefix = 'node_exporter-1.12.1.linux-' + arch
    binary = None
    seen = set()
    with tarfile.open(fileobj=io.BytesIO(archive), mode='r:gz') as contents:
        for index, member in enumerate(contents):
            need(index < 128 and member.name not in seen, 'archive entry budget/duplicate')
            seen.add(member.name)
            need(member.name in (prefix, prefix+'/node_exporter', prefix+'/LICENSE', prefix+'/NOTICE')
                 and not member.issym() and not member.islnk(), 'closed official archive layout required')
            if member.name == prefix:
                need(member.isdir(), 'archive directory required')
            else:
                need(member.isfile(), 'archive regular file required')
                need(0 < member.size <= (64*1024*1024 if member.name.endswith('/node_exporter') else 128*1024),
                     'archive member budget')
                if member.name.endswith('/node_exporter'):
                    with contents.extractfile(member) as stream:
                        binary = stream.read(64*1024*1024+1)
                    need(len(binary) == member.size, 'complete official binary required')
    need(binary is not None, 'official node_exporter missing')
    elf(binary, machine)
    worker = read(args.worker, 64*1024*1024)
    elf(worker, machine)
    inventory_raw = read(args.materials / 'notices/inventory.json', 256*1024)
    inventory = json.loads(inventory_raw, object_pairs_hook=unique)
    need(type(inventory) is dict and type(inventory.get('components')) is list
         and 1 <= len(inventory['components']) <= 128, 'bounded existing material inventory required')
    files = {'notices/inventory.json': inventory_raw}
    for component in inventory['components']:
        need(type(component) is dict and type(component.get('notices', [])) is list, 'notice component schema')
        for notice in component.get('notices', []):
            need(type(notice) is dict and type(notice.get('package_path')) is str
                 and re.fullmatch(r'notices/text/[0-9a-f]{64}\.txt', notice['package_path'])
                 and type(notice.get('original_path')) is str and 1 <= len(notice['original_path']) <= 512,
                 'content-addressed complete material path')
            name = notice['package_path']
            if name not in files:
                need(len(files) <= 128, 'material count budget')
                files[name] = read(args.materials / name, 128*1024)
    integrity = validate_notice_inventory(files, required=True)
    nodes = [c for c in inventory['components'] if c.get('kind') == 'bundled-collector-binary'
             and c.get('name') == 'node_exporter']
    need(len(nodes) == 1 and all(nodes[0].get(k) == v for k, v in {
        'version': 'v1.12.1', 'platform': 'linux-'+arch, 'binary_sha256': digest(binary),
        'binary_bytes': len(binary), 'archive_sha256': archive_pin}.items()) and nodes[0].get('notices'),
         'actual official binary/material binding required')
    policy = dict(format='cc-agent-managed-node-exporter/v1', hostId=args.host_id, agentId=args.agent_id,
                  artifact=dict(name='node_exporter', version='v1.12.1', platform=args.platform,
                                archiveSha256=archive_pin, binarySha256=digest(binary), binaryBytes=len(binary),
                                noticeInventorySha256=digest(inventory_raw)),
                  bindIp='127.0.0.1', bindPort=19100, collectors=['cpu', 'meminfo'], maxSeconds=args.max_seconds)
    encode = lambda value: (json.dumps(value, sort_keys=True, separators=(',', ':'), ensure_ascii=True)+'\n').encode()
    output = args.output
    need(output.is_absolute() and not os.path.lexists(output) and output.parent.resolve() == output.parent,
         'fresh absolute private output required')
    output.mkdir(mode=0o700)
    # Retain the exact official input for independent target-side revalidation.
    # The privileged role never installs this archive in the executable tree.
    files['official-archive.tar.gz'] = archive
    files['node_exporter'] = binary
    files['managed-node-exporter.json'] = encode(dict(format='cc-agent-installed-managed-exporter/v1',
                                                     policy=policy, workerSha256=digest(worker)))
    for name, raw in files.items():
        path = output / name
        path.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
        with path.open('xb') as stream:
            stream.write(raw)
        path.chmod(0o755 if name == 'node_exporter' else 0o644)
    (output / 'preparation.json').write_bytes(encode(dict(format='cc-managed-exporter-offline-preparation/v1',
        policy=policy, workerSha256=digest(worker), files={name:digest(raw) for name,raw in files.items()},
        integrity=integrity, binaryExecuted=False, provenanceCertified=False, installed=False)))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('archive', 'worker', 'materials', 'output'):
        parser.add_argument('--'+name, type=Path, required=True)
    parser.add_argument('--platform', choices=PINS, required=True)
    parser.add_argument('--host-id', required=True)
    parser.add_argument('--agent-id', required=True)
    parser.add_argument('--max-seconds', type=int, default=300)
    prepare(parser.parse_args())


if __name__ == '__main__':
    main()
