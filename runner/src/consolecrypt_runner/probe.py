from __future__ import annotations

import hashlib
from importlib import metadata
import json
import os
import platform
from pathlib import Path
import re
import shutil
import sys

from . import VERSION, PROTOCOL_VERSION
from .protocol import decode, encode
from .sandbox import probe_sandbox
from .integrity import dependency_fingerprint
from .errors import RunnerError, require

IDENTITY_FIELDS = ('helper', 'protocol', 'python', 'system', 'machine', 'packages',
                   'installed', 'helper_digest', 'dependency_lock', 'dependency_files_digest')


def environment_receipt(report, approved_digest):
    """Detached, bounded version evidence, never a copy of process environment."""
    require(isinstance(report, dict) and all(key in report for key in IDENTITY_FIELDS),
            'environment_receipt', 'Incomplete execution environment evidence')
    identity = {key: report[key] for key in IDENTITY_FIELDS}
    raw = encode(identity)
    require(len(raw) <= 64 * 1024, 'environment_receipt', 'Execution environment evidence exceeds limit')
    require(report.get('environment_digest') == approved_digest == hashlib.sha256(raw).hexdigest(),
            'environment_receipt', 'Execution environment evidence differs from approval')
    return {'version': 1, 'environment_digest': approved_digest,
            'identity': decode(raw, max_bytes=64 * 1024)}

PACKAGES = {"ansible-core": "2.20.11", "ansible-runner": "2.4.3", "ansible-lint": "26.11.0",
            "cryptography": "50.0.2", "PyYAML": "6.0.3"}


def probe():
    packages = {}
    for name, expected in PACKAGES.items():
        try:
            actual = metadata.version(name)
        except metadata.PackageNotFoundError:
            actual = None
        packages[name] = {"expected": expected, "actual": actual, "compatible": actual == expected}
    distributions = list(metadata.distributions())
    installed = {re.sub(r"[-_.]+", "-", item.metadata['Name']).lower(): item.version
                 for item in distributions if item.metadata['Name'] and item.metadata['Name'] != 'consolecrypt-runner'}
    try:
        dependency_digest = dependency_fingerprint(distributions, Path(sys.prefix))
    except (OSError, RunnerError):
        dependency_digest = None
    source = hashlib.sha256()
    package_root = Path(__file__).resolve().parent
    for file in sorted(package_root.rglob('*.py')):
        source.update(file.relative_to(package_root).as_posix().encode())
        source.update(b'\x00')
        source.update(file.read_bytes())
        source.update(b'\x00')
    lock_file = package_root / 'environments' / f'linux-{platform.machine()}-py{sys.version_info.major}{sys.version_info.minor}.json'
    lock = json.loads(lock_file.read_text(encoding='utf-8')) if lock_file.is_file() and sys.platform == 'linux' else None
    lock_matches = lock is not None and installed == lock['packages']
    identity = {"helper": VERSION, "protocol": PROTOCOL_VERSION, "python": platform.python_version(),
                "system": platform.system(), "machine": platform.machine(), "packages": packages,
                "installed": installed, "helper_digest": source.hexdigest(),
                "dependency_lock": lock['lock_sha256'] if lock else None,
                "dependency_files_digest": dependency_digest}
    sandbox = probe_sandbox()
    dependencies_ready = sys.version_info >= (3, 12) and lock_matches and dependency_digest is not None and all(item["compatible"] for item in packages.values())
    non_root = os.name == "posix" and os.getuid() != 0
    return {**identity, "environment_digest": hashlib.sha256(encode(identity)).hexdigest(),
            "non_root": non_root, "wsl": "microsoft" in platform.release().lower(), "sandbox": sandbox,
            "tools": {name: shutil.which(name) is not None for name in ("ssh", "ssh-agent", "git", "bwrap")},
            "capabilities": {"project_snapshot": os.name == "posix", "validate": dependencies_ready and non_root and sandbox["network"],
                             "trusted_worker_bootstrap_v1": True,
                             "vault_job_v1": sys.platform=='linux' and dependencies_ready and non_root and all(sandbox[key] for key in ('filesystem','pid','network')),
                             "become_native_v1": sys.platform=='linux' and dependencies_ready and non_root and all(sandbox[key] for key in ('filesystem','pid','network')),
                             "vault_ephemeral_v1": sys.platform=='linux' and dependencies_ready and non_root and all(sandbox[key] for key in ('filesystem','pid','network')),
                             "target_match_v1": dependencies_ready and non_root and all(sandbox[key] for key in ('filesystem','pid','network')),
                             "declared_inputs_v1": dependencies_ready and non_root and all(sandbox[key] for key in ('filesystem','pid','network')),
                             "locked_dependencies_v1": dependencies_ready and non_root and all(sandbox[key] for key in ('filesystem','pid','network')),
                             "project_requirements_v1": dependencies_ready and non_root and all(sandbox[key] for key in ('filesystem','pid','network')),
                             "static_inventory_sources": non_root and sys.version_info>=(3,12) and dependency_digest is not None
                                 and packages['PyYAML']['compatible'] and all(sandbox[key] for key in ('filesystem','pid','network')),
                             "execute": False, "strict_egress": False, "service": False},
            "dependencies_ready": dependencies_ready}
