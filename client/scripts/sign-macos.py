#!/usr/bin/env python3
"""Sign a built app with an existing, stable code-signing identity.

Does not create/import certificates or change Keychain access controls. Reusing
the same identity and bundle identifier lets macOS recognize app updates.
"""

import argparse
import os
from pathlib import Path
import re
import subprocess
import sys

MACHO_MAGIC = {
    bytes.fromhex(value)
    for value in ("feedface", "cefaedfe", "feedfacf", "cffaedfe", "cafebabe", "bebafeca", "cafebabf", "bfbafeca")
}
BUNDLE_SUFFIXES = {".app", ".framework", ".xpc", ".appex"}
LOCAL_AI_ENTITLEMENTS = Path(__file__).with_name("local-ai-helper.entitlements")


def resolve_identity(requested):
    result = subprocess.run(
        ["/usr/bin/security", "find-identity", "-v", "-p", "codesigning"],
        check=True, capture_output=True, text=True,
    )
    identities = re.findall(r'^\s*\d+\) ([0-9A-Fa-f]{40}) "([^"]+)"\s*$', result.stdout, re.MULTILINE)
    matches = {fingerprint.upper() for fingerprint, name in identities
               if requested == name or requested.upper() == fingerprint.upper()}
    if len(matches) != 1:
        raise ValueError("Signing identity must match exactly one valid code-signing certificate by name or SHA-1. "
                         "Check: security find-identity -v -p codesigning")
    return matches.pop()


def signing_targets(app):
    if app.suffix != ".app" or not app.is_dir() or app.is_symlink():
        raise ValueError("--app must be an existing application bundle, not a symlink")
    targets = {app}
    # Never follow framework symlinks or traverse outside the application.
    for parent, dirs, files in os.walk(app, followlinks=False):
        parent = Path(parent)
        dirs[:] = [name for name in dirs if not (parent / name).is_symlink()]
        for name in dirs:
            path = parent / name
            if path.suffix in BUNDLE_SUFFIXES:
                targets.add(path)
        for name in files:
            path = parent / name
            if path.is_symlink():
                continue
            with path.open("rb") as source:
                if source.read(4) in MACHO_MAGIC:
                    targets.add(path)
    return sorted(targets, key=lambda path: (-len(path.parts), str(path)))


def sign(app, identity):
    for target in signing_targets(app):
        command = ["/usr/bin/codesign", "--force", "--sign", identity]
        if identity != "-":
            # Hardened Runtime requires matching Team IDs on the helper/dylibs.
            # Ad-hoc previews have no Team ID; retain their existing non-hardened
            # policy. Stable identities still enforce Hardened Runtime.
            command += ["--options", "runtime"]
        local_ai = app / "Contents/Helpers/LocalAI"
        if local_ai in target.parents:
            if target.name == "llama-server":
                command += ["--entitlements", str(LOCAL_AI_ENTITLEMENTS)]
            command += ["--preserve-metadata=identifier"]
        else:
            # Do NOT preserve the old ad-hoc designated requirement (cdhash).
            command += ["--preserve-metadata=identifier,entitlements"]
        subprocess.run(command + [str(target)], check=True)
    subprocess.run(["/usr/bin/codesign", "--verify", "--deep", "--strict", "--verbose=2", str(app)], check=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    identity = parser.add_mutually_exclusive_group(required=True)
    identity.add_argument("--identity", help="Exact certificate name or SHA-1 fingerprint")
    identity.add_argument("--adhoc", action="store_true", help="Development preview, without Hardened Runtime")
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--check", action="store_true", help="Validate identity without signing anything")
    mode.add_argument("--app", type=Path)
    args = parser.parse_args()
    try:
        if args.adhoc and args.check:
            raise ValueError("--check requires an existing signing identity")
        identity = "-" if args.adhoc else resolve_identity(args.identity)
        if args.app is not None:
            sign(args.app.absolute(), identity)
        else:
            print("Code-signing identity is available.")
    except (ValueError, OSError, subprocess.CalledProcessError) as error:
        print(f"Signing failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
