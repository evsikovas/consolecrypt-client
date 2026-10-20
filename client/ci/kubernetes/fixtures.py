#!/usr/bin/env python3
"""Synthetic Git/OCI/chart fixtures for the disposable Kubernetes CI cluster."""
import argparse
import hashlib
import http.server
import io
import ipaddress
import json
import os
from pathlib import Path
import subprocess
import tarfile
import urllib.parse
import urllib.request

NAMESPACE = "cc-acceptance"
MARKER = {"consolecrypt.dev/acceptance": "0.6.2"}
BUSYBOX = "busybox:1.37.0@sha256:bdf57e528e45e4433820e045b29b4597825a1c9e38353532d90a01445013f82e"


def metadata(name, namespace=NAMESPACE, labels=None):
    value = {"name": name, "labels": {**MARKER, **(labels or {})}}
    if namespace is not None:
        value["namespace"] = namespace
    return value


def object_(kind, name, spec=None, api="v1", **extra):
    value = {"apiVersion": api, "kind": kind, "metadata": metadata(name), **extra}
    if spec is not None:
        value["spec"] = spec
    return value


def workload(name="podinfo", port=9898):
    return object_("Deployment", name, {
        "replicas": 1,
        "selector": {"matchLabels": {"app": name}},
        "template": {
            "metadata": {"labels": {"app": name}},
            "spec": {
                "automountServiceAccountToken": False,
                "containers": [{
                    "name": "web", "image": BUSYBOX,
                    "command": ["sh", "-c"],
                    "args": [f"mkdir -p /www; echo acceptance-podinfo > /www/index.html; httpd -f -p {port} -h /www"],
                    "ports": [{"name": "http", "containerPort": port}],
                    "readinessProbe": {"httpGet": {"path": "/", "port": port}, "initialDelaySeconds": 1},
                    "resources": {"requests": {"cpu": "10m", "memory": "16Mi"},
                                  "limits": {"cpu": "100m", "memory": "32Mi"}},
                }],
            },
        },
    }, api="apps/v1")


def repository_files():
    service = object_("Service", "podinfo", {
        "selector": {"app": "podinfo"},
        "ports": [{"name": "http", "port": 9898, "targetPort": 9898}],
    })
    deployment = workload()
    # Helm templates deliberately exercise stored chart defaults and overrides.
    chart = json.dumps(workload("fixture-helm")).replace(
        '"fixture-helm"', '"{{ .Values.fullnameOverride }}"'
    )
    return {
        "kustomize/deployment.yaml": json.dumps(deployment),
        "kustomize/service.yaml": json.dumps(service),
        "kustomize/kustomization.yaml": json.dumps({
            "apiVersion": "kustomize.config.k8s.io/v1beta1", "kind": "Kustomization",
            "resources": ["deployment.yaml", "service.yaml"],
        }),
        "local/config.yaml": json.dumps(object_("ConfigMap", "from-local-git", data={"fixture": "local-git"})),
        "charts/podinfo/Chart.yaml": "apiVersion: v2\nname: acceptance\nversion: 0.1.0\nappVersion: '1.37.0'\n",
        "charts/podinfo/values.yaml": "fullnameOverride: fixture-helm\nreplicaCount: 1\n",
        "charts/podinfo/templates/deployment.yaml": chart,
    }


def base_objects(node_ip):
    address = ipaddress.ip_address(node_ip)
    if address.is_loopback or address.is_unspecified or address.version != 4:
        raise ValueError("Expected an isolated container IPv4 address")
    items = [{"apiVersion": "v1", "kind": "Namespace",
              "metadata": metadata(NAMESPACE, None)}]
    # These services route only to fixture processes in the owned k3s network
    # namespace; no host ports, existing cluster services or external Git are used.
    for name, port, target in [("fixture-git", 8080, 18080), ("fixture-registry", 5000, 5000)]:
        items.append(object_("Service", name, {"ports": [{"name": "http", "port": port, "targetPort": target}]}))
        items.append(object_("Endpoints", name, subsets=[{
            "addresses": [{"ip": str(address)}], "ports": [{"name": "http", "port": target}],
        }]))
    items.extend([
        object_("ConfigMap", "fixture-config", data={"greeting": "acceptance"}),
        object_("Secret", "fixture-secret", stringData={"password": "cc-acceptance-must-stay-masked"}),
        object_("Secret", "fixture-secret-second", stringData={"token": "synthetic-only"}),
        object_("Pod", "fixture-shell", {
            "automountServiceAccountToken": False,
            "containers": [{
                "name": "shell", "image": BUSYBOX, "command": ["sh", "-c"],
                "args": ["echo acceptance-ready; mkdir -p /www; echo acceptance-forward > /www/index.html; httpd -f -p 8080 -h /www"],
                "resources": {"requests": {"cpu": "10m", "memory": "16Mi"},
                              "limits": {"cpu": "100m", "memory": "32Mi"}},
            }],
        }),
    ])
    return items


def flux_objects():
    items = []
    git_url = "http://fixture-git.cc-acceptance.svc:8080/repo.git"
    for name in ["fixture-source", "fixture-local-git", "fixture-broken-source"]:
        items.append(object_("GitRepository", name, {
            "interval": "1h", "timeout": "5s",
            "url": git_url if name != "fixture-broken-source" else git_url.replace("repo.git", "missing.git"),
            "ref": {"branch": "main"},
        }, api="source.toolkit.fluxcd.io/v1"))
    items.append(object_("OCIRepository", "fixture-local-oci", {
        "interval": "1h", "insecure": True,
        "url": "oci://fixture-registry.cc-acceptance.svc:5000/fixture", "ref": {"tag": "v1"},
    }, api="source.toolkit.fluxcd.io/v1"))
    for name, source, kind, path in [
        ("fixture-ready", "fixture-source", "GitRepository", "./kustomize"),
        ("fixture-blocked", "fixture-source", "GitRepository", "./kustomize"),
        ("fixture-local-ready", "fixture-local-git", "GitRepository", "./local"),
        ("fixture-oci-ready", "fixture-local-oci", "OCIRepository", "./"),
    ]:
        spec = {"interval": "1h", "retryInterval": "30s", "timeout": "2m", "prune": True,
                "wait": True, "targetNamespace": NAMESPACE, "path": path,
                "sourceRef": {"kind": kind, "name": source}}
        if name == "fixture-blocked":
            spec["dependsOn"] = [{"name": "missing-dependency"}]
        items.append(object_("Kustomization", name, spec, api="kustomize.toolkit.fluxcd.io/v1"))
    items.append(object_("HelmRelease", "fixture-helm", {
        "interval": "1h", "timeout": "2m", "releaseName": "fixture-helm",
        "chart": {"spec": {"chart": "./charts/podinfo", "reconcileStrategy": "Revision",
                           "sourceRef": {"kind": "GitRepository", "name": "fixture-source"}}},
        "values": {"fullnameOverride": "fixture-helm", "replicaCount": 1},
    }, api="helm.toolkit.fluxcd.io/v2"))
    return items


def scale_objects():
    labels = {**MARKER, "consolecrypt.dev/fixture": "scale-1000"}
    namespace = "cc-acceptance-scale"
    items = [{"apiVersion": "v1", "kind": "Namespace",
              "metadata": metadata(namespace, None, labels)}]
    for number in range(1000):
        items.append({"apiVersion": "v1", "kind": "Pod",
                      "metadata": metadata(f"fixture-scale-{number:04}", namespace, labels),
                      "spec": {"automountServiceAccountToken": False, "enableServiceLinks": False,
                               "restartPolicy": "Never",
                               "schedulingGates": [{"name": "consolecrypt.dev/acceptance"}],
                               "containers": [{"name": "never-start", "image": BUSYBOX}]}})
    return items


def serve_git(root=Path("/srv/repo.git"), port=18080, bind_host="0.0.0.0"):
    root.mkdir(parents=True)
    environment = {**os.environ, "GIT_CONFIG_NOSYSTEM": "1", "GIT_CONFIG_GLOBAL": "/dev/null",
                   "GIT_AUTHOR_DATE": "2026-01-01T00:00:00Z", "GIT_COMMITTER_DATE": "2026-01-01T00:00:00Z"}
    def git(*args):
        subprocess.run(["git", "-C", str(root), *args], check=True, env=environment,
                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    git("init", "-b", "main")
    git("config", "user.name", "Acceptance")
    git("config", "user.email", "acceptance@example.invalid")
    for name, value in repository_files().items():
        path = root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(value, encoding="utf-8")
    git("add", ".")
    git("commit", "-m", "Synthetic acceptance fixtures")
    (root / "git-daemon-export-ok").touch()

    class Handler(http.server.BaseHTTPRequestHandler):
        def log_message(self, *_args):
            pass

        def do_GET(self):
            self.handle_git()

        def do_POST(self):
            self.handle_git()

        def handle_git(self):
            url = urllib.parse.urlsplit(self.path)
            length = int(self.headers.get("Content-Length", 0))
            if not 0 <= length <= 1024 * 1024 or not url.path.startswith("/repo.git/") or "receive-pack" in self.path:
                self.send_error(400)
                return
            env = {**environment, "GIT_PROJECT_ROOT": str(root.parent), "GIT_HTTP_EXPORT_ALL": "1",
                   "PATH_INFO": url.path, "QUERY_STRING": url.query, "REQUEST_METHOD": self.command,
                   "CONTENT_TYPE": self.headers.get("Content-Type", ""), "CONTENT_LENGTH": str(length),
                   "REMOTE_ADDR": self.client_address[0]}
            output = subprocess.run(["git", "http-backend"], input=self.rfile.read(length),
                                    stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
                                    env=env, timeout=20, check=True).stdout
            headers, body = output.split(b"\r\n\r\n", 1)
            fields = [line.decode().split(":", 1) for line in headers.split(b"\r\n") if b":" in line]
            status = next((int(value.strip().split()[0]) for key, value in fields if key.lower() == "status"), 200)
            self.send_response(status)
            for key, value in fields:
                if key.lower() != "status":
                    self.send_header(key, value.strip())
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

    http.server.ThreadingHTTPServer((bind_host, port), Handler).serve_forever()


def seed_oci():
    # The process shares only the disposable k3s network namespace.
    base = "http://127.0.0.1:5000"
    def request(method, path, data=None, content_type=None):
        target = urllib.parse.urljoin(base, path)
        if urllib.parse.urlsplit(target).netloc != "127.0.0.1:5000":
            raise ValueError("Registry upload redirected outside the fixture")
        headers = {} if content_type is None else {"Content-Type": content_type}
        return urllib.request.urlopen(urllib.request.Request(target, data=data, method=method, headers=headers), timeout=20)
    def upload(data, media):
        digest = "sha256:" + hashlib.sha256(data).hexdigest()
        with request("POST", "/v2/fixture/blobs/uploads/", b"") as response:
            location = response.headers["Location"]
        location += ("&" if "?" in location else "?") + "digest=" + digest
        request("PUT", location, data, "application/octet-stream").close()
        return {"mediaType": media, "digest": digest, "size": len(data)}
    payload = json.dumps(object_("ConfigMap", "from-local-oci", data={"fixture": "local-oci"})).encode()
    archive = io.BytesIO()
    with tarfile.open(fileobj=archive, mode="w:gz") as tar:
        entry = tarfile.TarInfo("manifest.yaml")
        entry.size, entry.mode, entry.mtime = len(payload), 0o644, 0
        tar.addfile(entry, io.BytesIO(payload))
    manifest = {"schemaVersion": 2, "mediaType": "application/vnd.oci.image.manifest.v1+json",
                "config": upload(b"{}", "application/vnd.cncf.flux.config.v1+json"),
                "layers": [upload(archive.getvalue(), "application/vnd.cncf.flux.content.v1.tar+gzip")]}
    request("PUT", "/v2/fixture/manifests/v1", json.dumps(manifest).encode(),
            "application/vnd.oci.image.manifest.v1+json").close()


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("action", choices=["serve-git", "seed-oci"])
    action = parser.parse_args().action
    if action == "serve-git":
        serve_git()
    else:
        seed_oci()
