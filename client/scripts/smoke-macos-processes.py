#!/usr/bin/env python3
"""Exercise local zsh and the signed AI helper with the packaged macOS policy.

No downloads and no changes to the source application. The default checks a
login/interactive zsh on a PTY and the runtime/version/devices; --model-path
also makes one local chat request.
Use the same --identity as the packaged app for a Developer ID build.
"""

import argparse
import json
from pathlib import Path
import platform
import plistlib
import shutil
import stat
import subprocess
import sys
import tempfile
import uuid


HELPER_ENTITLEMENTS = {}
ARCHIVE_DIGESTS = {
    "macArm64": "a2073ed1aa00904d408a6694429aa9b6dcf4949c2683206c403930849ce222ac",
    "macX64": "f439aa26fa4d60ff60053b06ed901efe75865cd381063d1e65a0407a0d2761e6",
}


def checked(command, **kwargs):
    return subprocess.run(command, check=True, **kwargs)


def read_entitlements(path):
    result = checked(["/usr/bin/codesign", "-d", "--entitlements", "-", "--xml", str(path)], capture_output=True)
    return plistlib.loads(result.stdout) if result.stdout.strip() else {}


def check_process_policy(host, helper):
    if helper != HELPER_ENTITLEMENTS:
        raise ValueError("Packaged helper must clear obsolete sandbox/inherit entitlements")
    for key in ("com.apple.security.app-sandbox", "com.apple.security.inherit",
                "com.apple.security.cs.disable-library-validation"):
        if host.get(key):
            raise ValueError(f"Packaged host has an incompatible process entitlement: {key}")


def copy_regular(source, destination):
    if not stat.S_ISREG(source.lstat().st_mode):
        raise ValueError(f"Expected a materialized regular file: {source}")
    shutil.copyfile(source, destination)
    destination.chmod(source.stat().st_mode & 0o777)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--app", type=Path, required=True)
    parser.add_argument("--identity", default="-", help="Same existing signing identity as the source app")
    parser.add_argument("--model-path", type=Path, help="Optional already-downloaded GGUF for one generation")
    args = parser.parse_args()
    if sys.platform != "darwin":
        parser.error("This smoke requires macOS")
    architecture = {"arm64": "macArm64", "x86_64": "macX64"}.get(platform.machine())
    if architecture is None:
        parser.error("Unsupported host architecture")
    scripts = Path(__file__).resolve().parent
    source_contents = args.app.resolve(strict=True) / "Contents"
    source = source_contents / "Helpers" / "LocalAI" / architecture
    helper = source / "llama-server"
    host_entitlements = read_entitlements(args.app)
    check_process_policy(host_entitlements, read_entitlements(helper))
    checked(["/usr/bin/codesign", "--verify", "--deep", "--strict", str(args.app)])
    checked(["/usr/bin/codesign", "--verify", "--strict", str(helper)])
    with (source_contents / "Resources" / "LocalAI" / architecture / "runtime.json").open() as file:
        metadata = json.load(file)
    if (metadata.get("version") != "b11739" or metadata.get("platform") != architecture
            or metadata.get("archiveSha256") != ARCHIVE_DIGESTS[architecture]):
        raise ValueError("Packaged runtime metadata does not match this smoke")

    identifier = "io.consolecrypt.process.smoke." + uuid.uuid4().hex
    workspace = scripts.parents[1] / ".local" / "macos-process-smoke"
    workspace.mkdir(parents=True, exist_ok=True, mode=0o700)
    with tempfile.TemporaryDirectory(prefix="run-", dir=workspace) as temporary:
        app = Path(temporary) / "LocalAiSmoke.app"
        contents = app / "Contents"
        macos = contents / "MacOS"
        resources = contents / "Resources"
        runtime = contents / "Helpers" / "LocalAI" / architecture
        home = Path(temporary) / "home"
        for directory in (macos, resources, runtime):
            directory.mkdir(parents=True, exist_ok=True)
        (home / "tmp").mkdir(parents=True, mode=0o700)
        (home / ".zprofile").write_text("export CC_SMOKE_LOGIN=loaded\n")
        (home / ".zshrc").write_text("export CC_SMOKE_INTERACTIVE=loaded\n")
        entitlements = Path(temporary) / "host.entitlements"
        with entitlements.open("wb") as file:
            plistlib.dump(host_entitlements, file)
        for item in source.iterdir():
            if item.name == "llama-server" or item.suffix == ".dylib":
                copy_regular(item, runtime / item.name)
        # Preserve the original helper/dylib signatures. Re-signing them
        # here could hide a packaging error that this boundary test needs
        # to catch (including mismatched Developer ID teams).
        info = {
            "CFBundleExecutable": "LocalAiSmoke",
            "CFBundleIdentifier": identifier,
            "CFBundleName": "ConsoleCrypt Local AI Smoke",
            "CFBundlePackageType": "APPL",
            "CFBundleVersion": "1",
            "LSUIElement": True,
            "LocalAiSmokePlatform": architecture,
            "LocalAiSmokeModel": args.model_path is not None,
            "LocalProcessSmokeHome": str(home),
        }
        with (contents / "Info.plist").open("wb") as file:
            plistlib.dump(info, file)
        if args.model_path is not None:
            model = args.model_path.resolve(strict=True)
            if not model.is_file():
                raise ValueError("--model-path must refer to an existing GGUF file")
            # APFS clone avoids making two physical copies of a large GGUF.
            cloned = subprocess.run(["/bin/cp", "-c", str(model), str(resources / "model.gguf")], capture_output=True)
            if cloned.returncode:
                shutil.copyfile(model, resources / "model.gguf")
        checked([
            "/usr/bin/xcrun", "clang", "-fobjc-arc", "-framework", "Foundation",
            str(scripts / "macos-process-smoke.m"), "-o", str(macos / "LocalAiSmoke"),
        ])
        sign = ["/usr/bin/codesign", "--force", "--sign", args.identity]
        if args.identity != "-":
            sign += ["--options", "runtime"]
        checked(sign + ["--entitlements", str(entitlements), str(app)])
        checked(["/usr/bin/codesign", "--verify", "--deep", "--strict", str(app)])
        # Run only the isolated fixture. It never loads the real app/core,
        # accesses Keychain, or reads the user's shell startup files.
        result = subprocess.run([str(macos / "LocalAiSmoke")], timeout=180,
                                env={"HOME": str(home), "TMPDIR": str(home / "tmp"), "PATH": "/usr/bin:/bin"})
        report = (home / "local-ai-smoke.txt").read_text()
        print(report, end="")
        if result.returncode or not report.endswith("RESULT PASS\n"):
            raise RuntimeError("Packaged macOS process smoke failed")
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (OSError, ValueError, RuntimeError, subprocess.SubprocessError) as error:
        print(f"macOS process smoke failed: {error}", file=sys.stderr)
        sys.exit(1)
