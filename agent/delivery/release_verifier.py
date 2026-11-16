"""Adapter to the existing Rust Ed25519 release policy. No Python crypto fallback."""

import hashlib
import json
from pathlib import Path
import re
import subprocess


class ReleaseVerifier:
    def __init__(self, executable, executable_sha256, public_key, manifest, *, platform,
                 current_version, protocol=1, run=subprocess.run):
        self.executable, self.public_key, self.manifest = map(Path, (executable, public_key, manifest))
        if not all(path.is_absolute() for path in (self.executable, self.public_key, self.manifest)):
            raise ValueError("trusted absolute release-verifier paths required")
        if not re.fullmatch(r"[0-9a-f]{64}", executable_sha256):
            raise ValueError("verifier digest required")
        if platform not in {"linux-x86_64", "linux-aarch64"} or type(current_version) is not int or current_version < 0 or protocol != 1:
            raise ValueError("unsupported release verification context")
        self.executable_sha256 = executable_sha256
        self.platform, self.current_version, self.protocol = platform, current_version, protocol
        self.run = run
        self.verified_release = None

    def __call__(self, artifact):
        self.verified_release = None
        try:
            if hashlib.sha256(self.executable.read_bytes()).hexdigest() != self.executable_sha256:
                return False
            result = self.run([str(self.executable), "verify-release", "--public-key", str(self.public_key),
                               "--manifest", str(self.manifest), "--artifact", str(Path(artifact).absolute()),
                               "--platform", self.platform, "--protocol", str(self.protocol),
                               "--current-version", str(self.current_version)],
                              stdin=subprocess.DEVNULL, capture_output=True, timeout=10, check=False)
            if result.returncode != 0 or len(result.stdout) > 4096:
                return False
            value = json.loads(result.stdout)
            expected_keys = {"version", "displayVersion", "protocol", "platform", "artifactSha256", "artifactBytes"}
            if (not isinstance(value, dict) or set(value) != expected_keys
                    or type(value["version"]) is not int or value["version"] <= self.current_version
                    or value["platform"] != self.platform or type(value["protocol"]) is not int or value["protocol"] != self.protocol
                    or type(value["artifactBytes"]) is not int or value["artifactBytes"] != Path(artifact).stat().st_size
                    or value["artifactSha256"] != hashlib.sha256(Path(artifact).read_bytes()).hexdigest()):
                return False
            self.verified_release = value
            return True
        except (OSError, ValueError, TypeError, subprocess.SubprocessError):
            return False
