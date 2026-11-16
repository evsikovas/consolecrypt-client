"""Read-only public trust-input check. No key generation or host mutation.
Expected hashes/fingerprint must come from an independently authenticated admin
decision, never from candidate.zip, its manifest or a downloaded bootstrap file.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import stat


def read(path, limit):
    path = Path(path)
    if not path.is_absolute() or path.resolve() != path:
        raise ValueError("absolute no-link public input required")
    for parent in (path, *path.parents):
        if parent.is_symlink():
            raise ValueError("public input link")
    fd = os.open(path, os.O_RDONLY | getattr(os, "O_NONBLOCK", 0) | getattr(os, "O_NOFOLLOW", 0))
    try:
        before = os.fstat(fd)
        if not stat.S_ISREG(before.st_mode) or before.st_nlink != 1 or not 0 < before.st_size <= limit:
            raise ValueError("public input type/size")
        parts = []
        remaining = limit + 1
        while remaining:
            part = os.read(fd, min(remaining, 65536))
            if not part:
                break
            parts.append(part)
            remaining -= len(part)
        raw = b"".join(parts)
        after, named = os.fstat(fd), path.lstat()
        stamp = lambda m: (m.st_dev, m.st_ino, m.st_size, m.st_mtime_ns, m.st_ctime_ns)
        if len(raw) != before.st_size or stamp(before) != stamp(after) or stamp(before) != stamp(named):
            raise ValueError("public input changed")
        return raw
    finally:
        os.close(fd)


def unique(pairs):
    value = {}
    for key, item in pairs:
        if key in value:
            raise ValueError("duplicate public pin field")
        value[key] = item
    return value


def inspect(verifier, public_key, expected_verifier, expected_key, expected_fingerprint, platform):
    if platform not in {"linux-x86_64", "linux-aarch64"} or not all(
            re.fullmatch(r"[0-9a-f]{64}", p or "") and p != "0" * 64
            for p in (expected_verifier, expected_key, expected_fingerprint)):
        raise ValueError("independent exact public trust pins required")
    executable, raw = read(verifier, 64 * 1024 * 1024), read(public_key, 4096)
    if hashlib.sha256(executable).hexdigest() != expected_verifier or hashlib.sha256(raw).hexdigest() != expected_key:
        raise ValueError("public bootstrap byte pin mismatch")
    if (executable[:6] != b"\x7fELF\x02\x01" or int.from_bytes(executable[18:20], "little")
            != {"linux-x86_64": 62, "linux-aarch64": 183}[platform]):
        raise ValueError("reviewed verifier ELF platform mismatch")
    pin = json.loads(raw, object_pairs_hook=unique)
    if (type(pin) is not dict or set(pin) != {"format", "publicKey"} or type(pin["format"]) is not int or pin["format"] != 1
            or type(pin["publicKey"]) is not str or not re.fullmatch(r"[0-9a-f]{64}", pin["publicKey"]) or pin["publicKey"] == "0" * 64):
        raise ValueError("closed public ReleaseSignerPin required")
    fingerprint = hashlib.sha256(b"consolecrypt.agent.public-key.v1\0" + bytes.fromhex(pin["publicKey"])).hexdigest()
    if fingerprint != expected_fingerprint:
        raise ValueError("independently approved release fingerprint mismatch")
    trust = dict(version=1, verifierSha256=expected_verifier, publicKeySha256=expected_key, platform=platform)
    trust_json = json.dumps(trust, sort_keys=True, separators=(",", ":")) + "\n"
    return dict(format="cc-agent-public-bootstrap-check/v1", trustJson=trust_json,
        trustSha256=hashlib.sha256(trust_json.encode()).hexdigest(), verifierSha256=expected_verifier,
        publicKeySha256=expected_key, publicKeyFingerprint=fingerprint,
        signatureVerificationClaimed=False, hostChanged=False)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("verifier", "public-key", "expected-verifier", "expected-key", "expected-fingerprint", "platform"):
        parser.add_argument("--" + name, required=True)
    args = parser.parse_args()
    print(json.dumps(inspect(args.verifier, args.public_key, args.expected_verifier,
        args.expected_key, args.expected_fingerprint, args.platform), sort_keys=True))


if __name__ == "__main__":
    main()
