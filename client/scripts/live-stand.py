#!/usr/bin/env python3
"""Reproducible stand for the client's #[ignore] live tests (DOC-06).

Run before a release, on the owner's Mac with Docker (OrbStack) and rustup:

    live-stand.py list                  # every live group and what it needs
    live-stand.py sharing               # disposable PostgreSQL + server, 5 tests
    live-stand.py fingerprint --host 10.10.10.58   # the certificate SHA-256 to pin
    live-stand.py rdp --host 10.10.10.58 --user Администратор --certificate-sha256 HEX
    live-stand.py all --rdp-host 10.10.10.58 --rdp-user Администратор --rdp-certificate-sha256 HEX

sharing: starts a throwaway PostgreSQL 16 container on 127.0.0.1, builds and
starts the sibling consolecrypt-server on loopback with every sharing flag,
owner-online enrollment and a private FileMailer directory, runs the sharing
and enrollment tests of cc-sync-core and cc-app-core, then removes all of it.

rdp: writes the private mode-0600 CC_RDP_TEST_CONFIG for an owner-authorized
Windows host and runs the rdp-core live tests. The password comes from
CC_RDP_LIVE_PASSWORD or ~/.config/consolecrypt-test/rdp.env and is never
printed. The host certificate is pinned with --certificate-sha256; read it
with `fingerprint` and compare it with the certificate on the host.

Kubernetes (k3s + Flux + Argo) and the Linux Secret Service tests run in the
Linux CI job (ci-kubernetes-linux.py, ci-linux.sh); the OS keychain, Secure
Enclave and Windows Hello tests show system prompts and are run by hand.
"""

from __future__ import annotations

import argparse
import contextlib
import json
import os
from pathlib import Path
import secrets
import shutil
import socket
import stat
import subprocess
import sys
import tempfile
import time
import urllib.request

CLIENT = Path(__file__).resolve().parents[2]
RUST = CLIENT / "client" / "rust"
RDP_SECRET = Path.home() / ".config" / "consolecrypt-test" / "rdp.env"
POSTGRES_IMAGE = "postgres:16"

# (cargo test target, test name filter)
SHARING_TESTS = [
    (["-p", "cc-sync-core", "--test", "sharing_live"], None),
    (["-p", "cc-sync-core", "--test", "sharing_enrollment_live"], None),
    (["-p", "cc-app-core", "--test", "sharing_app"], "live_"),
]
# One Windows session at a time, each test in its own run; the confirmed
# paste drives the desktop (Run dialog) and goes last, on a settled session.
RDP_TESTS = [
    (["-p", "cc-rdp-core", "--lib"], f"manager::{name}")
    for name in (
        "live_redirect_tests::live_clipboard_and_selected_folder_roundtrip",
        "live_redirect_tests::live_selected_folder_core_windows_operations",
        "live_redirect_tests::live_selected_folder_real_windows_operations",
        "live_redirect_tests::live_selected_folder_stable_writable_windows_operations",
        "clipboard_live_tests::live_confirmed_ack_ticket_paste_real_windows",
    )
]
SHARING_FLAGS = [
    "CC_OBJECT_SHARING_ENABLED", "CC_SHARED_GROUPS_ENABLED", "CC_SHARED_SECRETS_ENABLED",
    "CC_SHARED_KUBE_CONTEXTS_ENABLED", "CC_SHARED_DB_CONNECTIONS_ENABLED", "CC_SHARED_NOTES_ENABLED",
    "CC_SHARING_OWNER_ONLINE_ENROLLMENT_ENABLED",
]
GROUPS = {
    "sharing": "PostgreSQL + server with FileMailer on loopback (Docker, sibling consolecrypt-server)",
    "rdp": "owner-authorized Windows host, CC_RDP_LIVE_PASSWORD or ~/.config/consolecrypt-test/rdp.env",
    "kubernetes": "Linux CI job: client/scripts/ci-kubernetes-linux.py (k3s, Flux, Argo)",
    "linux-keyring": "Linux CI job: client/scripts/ci-linux.sh (session bus + disposable GNOME keyring)",
    "os-prompts": "by hand: os_store, Secure Enclave, Windows Hello (system prompts)",
}


def cargo_env() -> dict[str, str]:
    env = dict(os.environ)
    # rustup's cargo honours rust-toolchain.toml; Homebrew's does not.
    env["PATH"] = f"{Path.home() / '.cargo' / 'bin'}{os.pathsep}{env.get('PATH', '')}"
    return env


def run_tests(groups: list[tuple[list[str], str | None]], env: dict[str, str]) -> bool:
    ok = True
    for target, name in groups:
        command = ["cargo", "test", "--manifest-path", str(RUST / "Cargo.toml"), *target, "--", "--ignored", "--test-threads=1"]
        command += [name] if name else []
        print("$", " ".join(command), flush=True)
        ok &= subprocess.run(command, env=env).returncode == 0
    return ok


def free_port() -> int:
    with socket.socket() as probe:
        probe.bind(("127.0.0.1", 0))
        return probe.getsockname()[1]


def wait(check, what: str, seconds: int = 90) -> None:
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        with contextlib.suppress(Exception):
            if check():
                return
        time.sleep(1)
    raise SystemExit(f"{what} did not become ready in {seconds} s")


@contextlib.contextmanager
def postgres():
    name = f"cc-live-pg-{secrets.token_hex(4)}"
    password = secrets.token_urlsafe(24)
    subprocess.run(
        ["docker", "run", "-d", "--rm", "--name", name, "-e", "POSTGRES_USER=cclive", "-e", f"POSTGRES_PASSWORD={password}",
         "-e", "POSTGRES_DB=cclive", "-p", "127.0.0.1::5432", POSTGRES_IMAGE],
        check=True, stdout=subprocess.DEVNULL,
    )
    try:
        port = subprocess.run(["docker", "port", name, "5432/tcp"], check=True, capture_output=True, text=True).stdout.split(":")[-1].strip()
        wait(lambda: subprocess.run(["docker", "exec", name, "pg_isready", "-U", "cclive", "-d", "cclive"],
                                    capture_output=True).returncode == 0, "PostgreSQL")
        yield f"postgres://cclive:{password}@127.0.0.1:{port}/cclive"
    finally:
        subprocess.run(["docker", "rm", "-f", name], capture_output=True)


@contextlib.contextmanager
def server(database_url: str, server_repo: Path, mail_dir: Path):
    manifest = server_repo / "server" / "Cargo.toml"
    subprocess.run(["cargo", "build", "--release", "--manifest-path", str(manifest), "--bin", "consolecrypt-server"],
                   check=True, env=cargo_env())
    binary = server_repo / "server" / "target" / "release" / "consolecrypt-server"
    if not binary.exists():
        binary = server_repo / "target" / "release" / "consolecrypt-server"
    port = free_port()
    env = {
        "PATH": os.environ.get("PATH", ""), "HOME": str(Path.home()),
        "CC_DATABASE_URL": database_url, "CC_LISTEN_ADDR": f"127.0.0.1:{port}", "CC_METRICS_LISTEN": "off",
        "CC_MAIL_TRANSPORT": "file", "CC_MAIL_DIR": str(mail_dir), "CC_MAIL_FROM": "ConsoleCrypt <noreply@example.test>",
        "CC_REGISTRATION_OPEN": "true", "CC_LOG_FORMAT": "pretty", "CC_RUN_MIGRATIONS": "true",
        **{flag: "true" for flag in SHARING_FLAGS},
    }
    log = open(mail_dir.parent / "server.log", "wb")
    process = subprocess.Popen([str(binary)], env=env, stdout=log, stderr=subprocess.STDOUT)
    url = f"http://127.0.0.1:{port}"
    try:
        def meta() -> bool:
            if process.poll() is not None:
                raise SystemExit(f"the server exited ({process.returncode}); see {log.name}")
            with urllib.request.urlopen(f"{url}/v1/meta", timeout=2) as response:
                return response.status == 200
        wait(meta, "the server")
        yield url
    finally:
        process.terminate()
        with contextlib.suppress(subprocess.TimeoutExpired):
            process.wait(10)
        if process.poll() is None:
            process.kill()
        log.close()


def sharing(args) -> bool:
    if not shutil.which("docker"):
        raise SystemExit("Docker is required for the sharing stand")
    server_repo = Path(args.server).resolve()
    with tempfile.TemporaryDirectory(prefix="cc-live-sharing-") as scratch:
        mail = Path(scratch) / "mail"
        mail.mkdir(mode=0o700)
        with postgres() as database, server(database, server_repo, mail) as url:
            env = cargo_env() | {"CC_SHARING_TEST_SERVER_URL": url, "CC_SHARING_TEST_MAIL_DIR": str(mail)}
            ok = run_tests(SHARING_TESTS, env)
            if not ok:
                print(f"server log: {Path(scratch) / 'server.log'} (kept until this command exits)", file=sys.stderr)
            return ok


def rdp_password() -> str:
    if password := os.environ.get("CC_RDP_LIVE_PASSWORD"):
        return password
    if not RDP_SECRET.exists():
        raise SystemExit(f"Set CC_RDP_LIVE_PASSWORD or create {RDP_SECRET} (mode 600)")
    if RDP_SECRET.stat().st_mode & (stat.S_IRWXG | stat.S_IRWXO):
        raise SystemExit(f"{RDP_SECRET} must be readable only by you (chmod 600)")
    for line in RDP_SECRET.read_text().splitlines():
        key, _, value = line.strip().partition("=")
        if key.strip() == "CC_RDP_LIVE_PASSWORD" and value.strip():
            return value.strip().strip("'\"")
    raise SystemExit(f"{RDP_SECRET} has no CC_RDP_LIVE_PASSWORD")


def rdp(args) -> bool:
    with tempfile.TemporaryDirectory(prefix="cc-live-rdp-") as scratch:
        config = Path(scratch) / "rdp.json"
        # --output keeps the test's frames (remote screenshots) for diagnosis.
        output = Path(args.output) if getattr(args, "output", None) else Path(scratch) / "output"
        output.mkdir(mode=0o700, parents=True, exist_ok=True)
        values = {
            "address": args.host, "port": args.port, "username": args.user, "domain": args.domain or None,
            "password": rdp_password(), "certificate_sha256": args.certificate_sha256,
            "width": 1280, "height": 720,  # the clipboard test expects this viewport
        }
        descriptor = os.open(config, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
        with os.fdopen(descriptor, "w") as handle:
            json.dump(values, handle)
        del values
        env = cargo_env() | {"CC_RDP_TEST_CONFIG": str(config), "CC_RDP_TEST_OUTPUT_DIR": str(output)}
        return run_tests(RDP_TESTS, env)


def certificate_fingerprint(host: str, port: int) -> str:
    """SHA-256 of the RDP server certificate: X.224 Connection Request for TLS
    (and CredSSP), then a TLS handshake. Nothing is sent after the handshake."""
    import hashlib
    import ssl

    # TPKT (19 bytes), X.224 Connection Request, RDP_NEG_REQ for TLS | CredSSP.
    request = bytes.fromhex("03000013" "0ee00000000000" "0100080003000000")
    with socket.create_connection((host, port), timeout=10) as raw:
        raw.sendall(request)
        reply = raw.recv(64)
        if len(reply) < 19 or reply[5] != 0xD0 or reply[11] != 0x02:
            raise SystemExit("the host did not accept TLS for RDP")
        context = ssl.SSLContext(ssl.PROTOCOL_TLS_CLIENT)
        context.check_hostname = False
        context.verify_mode = ssl.CERT_NONE  # Only read the certificate to pin it.
        with context.wrap_socket(raw, server_hostname=host) as tls:
            return hashlib.sha256(tls.getpeercert(binary_form=True)).hexdigest()


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    commands = parser.add_subparsers(dest="command", required=True)
    commands.add_parser("list")
    probe = commands.add_parser("fingerprint", help="print the RDP certificate SHA-256 to pin")
    probe.add_argument("--host", required=True)
    probe.add_argument("--port", type=int, default=3389)
    share = commands.add_parser("sharing")
    share.add_argument("--server", default=str(CLIENT.parent / "consolecrypt-server"), help="consolecrypt-server checkout")
    remote = commands.add_parser("rdp")
    everything = commands.add_parser("all")
    everything.add_argument("--server", default=str(CLIENT.parent / "consolecrypt-server"))
    for target, prefix in ((remote, ""), (everything, "rdp-")):
        target.add_argument(f"--{prefix}host", dest="host", required=target is remote)
        target.add_argument(f"--{prefix}user", dest="user", required=target is remote)
        target.add_argument(f"--{prefix}port", dest="port", type=int, default=3389)
        target.add_argument(f"--{prefix}domain", dest="domain", default="")
        target.add_argument(f"--{prefix}output", dest="output", default=None,
                            help="keep the RDP test frames in this private folder")
        target.add_argument(f"--{prefix}certificate-sha256", dest="certificate_sha256", required=target is remote,
                            help="the pinned certificate; see `fingerprint` (confirm it on the host)")
    args = parser.parse_args()
    if args.command == "list":
        for group, needs in GROUPS.items():
            print(f"{group:14} {needs}")
        return
    if args.command == "fingerprint":
        print(certificate_fingerprint(args.host, args.port))
        return
    results = {}
    if args.command in ("sharing", "all"):
        results["sharing"] = sharing(args)
    if args.command == "rdp" or (args.command == "all" and args.host):
        results["rdp"] = rdp(args)
    for group, ok in results.items():
        print(f"{group}: {'passed' if ok else 'FAILED'}")
    sys.exit(0 if all(results.values()) else 1)


if __name__ == "__main__":
    main()
