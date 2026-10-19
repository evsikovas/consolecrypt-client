import hashlib
import importlib.util
import io
import json
from pathlib import Path
import tarfile
from tempfile import TemporaryDirectory
import unittest
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location("bundle_macos_local_ai", Path(__file__).with_name("bundle-macos-local-ai.py"))
bundler = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(bundler)


def archive(root, extra=(), filename="fixture.tar.gz"):
    path = root / filename
    with tarfile.open(path, "w:gz") as output:
        items = [
            ("llama-server", b"server", None),
            ("libllama-server-impl.dylib", b"library", None),
            ("libggml.0.27.0.dylib", b"ggml", None),
            ("libggml.0.dylib", b"", "libggml.0.27.0.dylib"),
            ("libggml.dylib", b"", "libggml.0.dylib"),
            ("llama-bench", b"unneeded tool", None),
            ("LICENSE", b"MIT License", None),
            *extra,
        ]
        for name, content, link in items:
            member = tarfile.TarInfo(f"llama-{bundler.VERSION}/{name}")
            if link is not None:
                member.type = tarfile.SYMTYPE
                member.linkname = link
                output.addfile(member)
            else:
                member.size = len(content)
                output.addfile(member, io.BytesIO(content))
    artifact = {"filename": filename, "bytes": path.stat().st_size, "sha256": hashlib.sha256(path.read_bytes()).hexdigest()}
    return path, artifact


class MacLocalAiBundleTests(unittest.TestCase):
    def test_materializes_internal_aliases_and_keeps_only_server_dependencies(self):
        with TemporaryDirectory() as temp:
            root = Path(temp)
            source, artifact = archive(root)
            output = root / "runtime"
            bundler.extract(source, output, artifact, "macArm64")
            self.assertEqual((output / "libggml.dylib").read_bytes(), b"ggml")
            self.assertFalse(any(p.is_symlink() for p in output.iterdir()))
            self.assertFalse((output / "llama-bench").exists())
            self.assertEqual((output / "llama-server").stat().st_mode & 0o777, 0o755)
            self.assertEqual(json.loads((output / "runtime.json").read_text()), {
                "version": bundler.VERSION, "platform": "macArm64",
                "archiveSha256": artifact["sha256"], "sourceCommit": bundler.COMMIT,
            })

    def test_archive_digest_is_checked_before_extraction(self):
        with TemporaryDirectory() as temp:
            root = Path(temp)
            source, artifact = archive(root)
            source.write_bytes(source.read_bytes()[:-1] + b"X")
            with self.assertRaisesRegex(ValueError, "SHA-256"):
                bundler.extract(source, root / "runtime", artifact, "macArm64")
            self.assertFalse((root / "runtime").exists())

    def test_rejects_traversal_external_links_cycles_and_case_collisions(self):
        fixtures = [
            [("../escape", b"evil", None)],
            [("libevil.dylib", b"", "../escape")],
            [("libevil.dylib", b"", "libevil.dylib")],
            [("LICENSE", b"replacement", None)],
            [("license", b"replacement", None)],
        ]
        for extra in fixtures:
            with self.subTest(extra=extra), TemporaryDirectory() as temp:
                root = Path(temp)
                source, artifact = archive(root, extra)
                with self.assertRaises(ValueError):
                    bundler.extract(source, root / "runtime", artifact, "macArm64")
                self.assertFalse((root / "runtime").exists())
                self.assertFalse((root / "escape").exists())

    def test_failed_second_architecture_keeps_previous_bundle(self):
        with TemporaryDirectory() as temp:
            root = Path(temp)
            source, artifact = archive(root)
            app = root / "Test.app"
            current = app / "Contents/Helpers/LocalAI"
            current.mkdir(parents=True)
            (app / "Contents/Info.plist").write_text("fixture")
            (current / "previous").write_text("valid")
            with patch.object(bundler, "RUNTIMES", {"macArm64": artifact, "macX64": artifact}), \
                    patch.object(bundler, "download", side_effect=[source, OSError("unavailable")]):
                with self.assertRaises(OSError):
                    bundler.bundle(app, root / "cache")
            self.assertEqual((current / "previous").read_text(), "valid")
            self.assertEqual(list(current.parent.iterdir()), [current])

    def test_bundle_puts_metadata_and_licenses_in_resources_not_nested_code(self):
        with TemporaryDirectory() as temp:
            root = Path(temp)
            source, artifact = archive(root)
            app = root / "Test.app"
            (app / "Contents").mkdir(parents=True)
            (app / "Contents/Info.plist").write_text("fixture")
            with patch.object(bundler, "RUNTIMES", {"macArm64": artifact, "macX64": artifact}), \
                    patch.object(bundler, "download", return_value=source):
                bundler.bundle(app, root / "cache")
            for platform in ["macArm64", "macX64"]:
                code = app / "Contents/Helpers/LocalAI" / platform
                data = app / "Contents/Resources/LocalAI" / platform
                self.assertTrue((code / "llama-server").is_file())
                self.assertFalse((code / "runtime.json").exists())
                self.assertFalse((code / "LICENSE").exists())
                self.assertTrue((data / "runtime.json").is_file())
                self.assertTrue((data / "LICENSE").is_file())
            self.assertTrue((app / "Contents/Resources/LOCAL-AI-THIRD-PARTY-NOTICES.txt").is_file())

    def test_pins_match_runtime_catalog_and_reject_untrusted_redirects(self):
        dart = (Path(__file__).parents[1] / "flutter/lib/ai/local/local_ai_catalog.dart").read_text()
        for artifact in bundler.RUNTIMES.values():
            for value in artifact.values():
                self.assertIn(str(value), dart)
        self.assertIn(bundler.COMMIT, dart)
        for url in ["http://github.com/file", "https://github.com.evil.test/file", "https://u:p@github.com/file", "file:///tmp/runtime"]:
            with self.assertRaises(ValueError):
                bundler.trusted_url(url)
        self.assertEqual(bundler.trusted_url("https://release-assets.githubusercontent.com/file"),
                         "https://release-assets.githubusercontent.com/file")


if __name__ == "__main__":
    unittest.main()
