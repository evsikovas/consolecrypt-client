"""Trusted local paths for the separate Linux reader worker, never credentials."""
from dataclasses import dataclass
import math
import os
from pathlib import Path
import re
import stat

from .identity_gate import strict_json
from .reader_files import read_reader_file


def _startup_path(path, *, directory=False):
    for component in (path, *path.parents):
        info = component.lstat()
        if stat.S_ISLNK(info.st_mode) or getattr(info, "st_file_attributes", 0) & 0x400:
            raise ValueError("reader_startup_linked_path")
    info = path.stat()
    if directory:
        if not stat.S_ISDIR(info.st_mode):
            raise ValueError("reader_existing_state_required")
        if os.name != "nt" and (info.st_uid != os.geteuid() or info.st_mode & 0o077):
            raise ValueError("reader_private_state_required")
    else:
        if not stat.S_ISREG(info.st_mode) or info.st_nlink != 1:
            raise ValueError("reader_regular_file_required")
        if os.name != "nt" and (info.st_uid not in (0, os.geteuid()) or info.st_mode & 0o022):
            raise ValueError("reader_trusted_file_required")


@dataclass(frozen=True, slots=True, repr=False)
class ReaderGateConfig:
    executable: Path
    executable_sha256: str
    reader_store: Path
    reader_anchor: Path
    issuer_pin: Path
    node_store: Path
    node_anchor: Path
    node_owner_pin: Path
    target: Path
    timeout_seconds: float = 5.0

    @classmethod
    def from_dict(cls, value):
        paths = {"executable", "reader_store", "reader_anchor", "issuer_pin",
                 "node_store", "node_anchor", "node_owner_pin", "target"}
        required = paths | {"executable_sha256"}
        if (type(value) is not dict or not required <= value.keys()
                or set(value) - (required | {"timeout_seconds"})):
            raise ValueError("invalid_reader_configuration")
        if any(type(value[name]) is not str or not 0 < len(value[name]) <= 4096
               or "\x00" in value[name] for name in paths):
            raise ValueError("invalid_reader_configuration")
        return cls(**{name: Path(item) if name in paths else item for name, item in value.items()})

    @classmethod
    def from_file(cls, path):
        path = Path(path).absolute()
        _startup_path(path)
        config = cls.from_dict(strict_json(read_reader_file(path, 8192)))
        config.validate_files()
        return config

    def validate_files(self):
        """Check local startup inputs without creating state or granting access.

        Actual pin types, certificates, checkpoints and signatures remain the
        native ReaderActor's responsibility on every admitted request.
        """
        for path in (self.reader_store, self.reader_anchor, self.node_store, self.node_anchor):
            _startup_path(path, directory=True)
        for path in (self.executable, self.issuer_pin, self.node_owner_pin, self.target):
            _startup_path(path)
        if read_reader_file(self.executable, 128 * 1024**2, digest=True) != self.executable_sha256:
            raise ValueError("reader_executable_digest_mismatch")
        if os.name != "nt" and not os.access(self.executable, os.X_OK):
            raise ValueError("reader_executable_not_executable")
        for path in (self.issuer_pin, self.node_owner_pin, self.target):
            value = strict_json(read_reader_file(path, 16384))
            if type(value) is not dict or not value:
                raise ValueError("reader_public_input_invalid")

    def validate_node_config(self, config):
        from .identity_gate import GateConfig

        if (type(config) is not GateConfig
                or config.executable_sha256 != self.executable_sha256
                or any(left.resolve(strict=True) != right.resolve(strict=True) for left, right in (
                    (config.executable, self.executable), (config.store, self.node_store),
                    (config.anchor, self.node_anchor), (config.owner_pin, self.node_owner_pin)))):
            raise ValueError("reader_node_configuration_mismatch")

    def __post_init__(self):
        paths = (self.executable, self.reader_store, self.reader_anchor, self.issuer_pin,
                 self.node_store, self.node_anchor, self.node_owner_pin, self.target)
        for path in paths:
            if not isinstance(path, Path) or not path.is_absolute() or ".." in path.parts:
                raise ValueError("absolute_trusted_reader_paths_required")
        if (type(self.executable_sha256) is not str
                or re.fullmatch("[0-9a-f]{64}", self.executable_sha256) is None):
            raise ValueError("reader_verifier_digest_required")
        if (type(self.timeout_seconds) not in (int, float)
                or not 0 < self.timeout_seconds <= 5
                or not math.isfinite(self.timeout_seconds)):
            raise ValueError("reader_timeout_outside_budget")
        # Resolve aliases before selecting a worker; this creates no directories
        # or trust. Native open/init must independently check actual file IDs,
        # ownership, private permissions, schema and integrity before writing.
        directories = tuple(path.resolve() for path in (
            self.reader_store, self.reader_anchor, self.node_store, self.node_anchor))
        for index, path in enumerate(directories):
            for other in directories[index + 1:]:
                if path == other or path.is_relative_to(other) or other.is_relative_to(path):
                    raise ValueError("reader_state_namespaces_overlap")
        files = tuple(path.resolve() for path in (
            self.executable, self.issuer_pin, self.node_owner_pin, self.target))
        if len(set(files)) != len(files):
            raise ValueError("reader_trusted_files_overlap")
        if any(file == directory or file.is_relative_to(directory)
               for file in files for directory in directories):
            raise ValueError("reader_trusted_file_in_state_namespace")

    def __repr__(self):
        return "ReaderGateConfig([redacted])"

    def __reduce_ex__(self, protocol):
        raise TypeError("reader_config_not_portable")
