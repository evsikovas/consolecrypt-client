#!/usr/bin/env python3
"""Own one GitHub Linux builder; stop it independently of a killed build shell."""

import argparse
import importlib.util
import json
import os
from pathlib import Path
import re
import stat
import subprocess
import sys


def load(name, filename):
    spec = importlib.util.spec_from_file_location(name, Path(__file__).with_name(filename))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


cleanup = load('owned_ci_workspace', 'cleanup-ci-workspace.py')
retention = load('safe_linux_retention', 'retain-linux-receipt.py')
IMAGE = 'consolecrypt-linux-builder:3.47.5-rust1.98'
OWNER_FILE = '.consolecrypt-linux-owner.json'
LABEL_PREFIX = 'io.consolecrypt.ci.'
INSPECT_FORMAT = '[{{json .Id}},{{json .Name}},{{json .Config.Image}},{{json .Config.Labels}},{{json .Mounts}}]'


def ownership(workspace, environment):
    owner = cleanup.identity(environment)
    if owner['job'] != 'linux':
        raise ValueError('Only the Linux job can own a Linux builder')
    temp = environment.get('RUNNER_TEMP', '')
    if not temp or not Path(temp).is_absolute():
        raise ValueError('Expected absolute RUNNER_TEMP')
    temp = cleanup.absolute(temp)
    cleanup.no_link_ancestors(temp)
    name = 'consolecrypt-linux-ci-' + '-'.join(owner[key] for key in ('run', 'attempt', 'job'))
    root = temp / name
    cleanup.no_link_ancestors(root / 'source')
    receipt = {'schema': 1, **owner, 'workspace': str(workspace),
               'container': name, 'source': str(root / 'source'), 'image': IMAGE}
    return root, receipt


def labels(receipt):
    return {LABEL_PREFIX + key: receipt[key] for key in ('repository', 'run', 'attempt', 'job')}


def read_owner(path):
    cleanup.no_link_ancestors(path)
    descriptor = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    with os.fdopen(descriptor, 'rb') as reader:
        info = os.fstat(reader.fileno())
        if not stat.S_ISREG(info.st_mode) or info.st_nlink != 1 or info.st_size > 8192:
            raise ValueError('Invalid Linux job ownership file')
        return json.loads(reader.read(8193))


def prepare(workspace, environment):
    root, receipt = ownership(workspace, environment)
    # Existing state is never overwritten, even for a duplicate launch of a job.
    root.mkdir(mode=0o700)
    descriptor = os.open(root / OWNER_FILE, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
    with os.fdopen(descriptor, 'w') as writer:
        json.dump(receipt, writer, sort_keys=True)
        writer.write('\n')
        writer.flush()
        os.fsync(writer.fileno())
    return root


def docker(*arguments):
    # Inspect only selected ownership fields; never print container env or logs.
    return subprocess.run(['docker', *arguments], check=True, capture_output=True,
                          text=True, timeout=45).stdout.strip()


def find_container(name):
    result = docker('container', 'ls', '--all', '--no-trunc',
                    '--filter', 'name=^/' + name + '$', '--format', '{{.ID}}')
    if not result:
        return None
    if not re.fullmatch(r'[a-f0-9]{64}', result):
        raise ValueError('Ambiguous Linux builder container identity')
    return result


def validate_container(container_id, expected):
    value = json.loads(docker('container', 'inspect', '--format', INSPECT_FORMAT, container_id))
    if not isinstance(value, list) or len(value) != 5:
        raise ValueError('Unexpected Docker ownership metadata')
    actual_id, name, image, actual_labels, mounts = value
    if (actual_id != container_id or name != '/' + expected['container'] or image != IMAGE
            or not isinstance(actual_labels, dict)
            or any(actual_labels.get(key) != value for key, value in labels(expected).items())):
        raise ValueError('Linux builder labels, image or identity differ from this job')
    if not isinstance(mounts, list) or len(mounts) != 1 or not isinstance(mounts[0], dict):
        raise ValueError('Unexpected Linux builder mounts')
    mount = mounts[0]
    if (mount.get('Type') != 'bind' or mount.get('Source') != expected['source']
            or mount.get('Destination') != '/work' or mount.get('RW') is not True):
        raise ValueError('Linux builder source mount differs from this job')
    cleanup.no_link_ancestors(Path(expected['source']))


def container_gone(container_id, name):
    # A successful second query distinguishes --rm races from daemon failures.
    # Check both immutable ID and name; a replacement might still use /work.
    remaining = docker('container', 'ls', '--all', '--no-trunc',
                       '--filter', 'id=' + container_id, '--format', '{{.ID}}')
    return not remaining and find_container(name) is None


def finish(workspace, environment):
    root, expected = ownership(workspace, environment)
    container_id = find_container(expected['container'])
    if not root.exists():
        if container_id is not None:
            raise ValueError('Builder exists without a matching ownership receipt')
        return
    if read_owner(root / OWNER_FILE) != expected:
        raise ValueError('Linux builder ownership receipt differs from this job')
    if container_id is not None:
        try:
            validate_container(container_id, expected)
            # Remove by the inspected immutable ID, never by a reused name.
            docker('container', 'rm', '--force', container_id)
        except subprocess.CalledProcessError:
            # --rm can win either ls→inspect or inspect→rm. Metadata/ownership
            # errors are not caught, and an unavailable daemon cannot prove
            # absence. Never remove scratch under a replacement container.
            if not container_gone(container_id, expected['container']):
                raise
    if find_container(expected['container']) is not None:
        raise ValueError('A replacement builder may still be using the owned scratch')
    # An interrupted Docker CLI must not leave writers alive during traversal.
    # Receipt validation failures still clean owned scratch, but fail the step.
    try:
        retention.retain(root / 'source', workspace)
    finally:
        cleanup.remove_owned(root)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--workspace', type=Path, required=True)
    parser.add_argument('--prepare', action='store_true')
    args = parser.parse_args()
    workspace = cleanup.absolute(args.workspace)
    cleanup.validate_workspace(workspace, os.environ, cleanup.absolute(__file__).parents[2])
    if args.prepare:
        print(prepare(workspace, os.environ))
    else:
        finish(workspace, os.environ)
        print('LINUX_JOB_CLEANUP_PASS')


if __name__ == '__main__':
    try:
        main()
    except (OSError, ValueError, RuntimeError, subprocess.SubprocessError) as error:
        print('Linux job cleanup refused or failed (' + cleanup.error_code(error) + ').', file=sys.stderr)
        sys.exit(1)
