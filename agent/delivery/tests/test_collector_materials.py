"""Synthetic PUBLIC buildinfo/archive causals. No Go/network/binary execution."""
import base64
import copy
import importlib.util
import io
import json
import os
from pathlib import Path
import stat
import sys
import tarfile
import tempfile
import time
import unittest
from unittest import mock
import zipfile

DELIVERY = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(DELIVERY))
import collector_materials as materials
import release_pipeline as pipeline


def info():
    # Actual observed public binary metadata; full notice texts below remain
    # synthetic and these unit fixtures claim no actual Go download/ZIP proof.
    return copy.deepcopy(materials.official_source()["observedBuildInfo"])


def full_fixture():
    raw = materials.encoded(info())
    source = materials.official_source()
    version, modules = materials.build_modules(raw, source)
    return dict(format="cc-vmagent-go-materials/v1", binarySha256=materials.PIN, binaryBytes=23209584,
        buildInfo=info(), buildInfoJson=raw.decode(), buildInfoSha256=materials.digest(raw),
        modules=[dict(**row, archiveSha256="a" * 64, materials=[materials.material("LICENSE", b"Synthetic complete public module license\n")]) for row in modules],
        stdlib=dict(version=version, archiveSha256="b" * 64,
            materials=[materials.material(name, ("Synthetic public " + name + "\n").encode())
                       for name in ("LICENSE", "PATENTS", "AUTHORS", "CONTRIBUTORS")]),
        goToolSha256="c" * 64, sourceNoticeProvenance=materials.source_provenance(source), legalCertification=False, binaryExecuted=False)


class CollectorMaterials(unittest.TestCase):
    def test_native_arm_official_capture_format_keeps_full_source_and_architecture_fences(self):
        # Real canonical ARM buildinfo; license/tool fields remain the explicit
        # synthetic unit fixture above. This is not an actual Go capture record.
        source=materials.official_source("linux-arm64")
        selected=materials.release_platforms.select("linux-arm64","collector")
        value=full_fixture()
        raw=materials.encoded(source["observedBuildInfo"])
        value.update(binarySha256=selected["binarySha256"],binaryBytes=selected["observedBytes"],
            buildInfo=source["observedBuildInfo"],buildInfoJson=raw.decode(),
            buildInfoSha256=materials.digest(raw),sourceNoticeProvenance=materials.source_provenance(source))
        self.assertEqual(materials.checked(materials.encoded(value),selected["binarySha256"]),value)
        wrong=copy.deepcopy(value)
        for row in wrong["buildInfo"]["Settings"]:
            if row["Key"]=="GOARCH":row["Value"]="amd64"
        raw=materials.encoded(wrong["buildInfo"]);wrong.update(buildInfoJson=raw.decode(),buildInfoSha256=materials.digest(raw))
        with self.assertRaisesRegex(ValueError,"architecture mismatch"):
            materials.checked(materials.encoded(wrong),selected["binarySha256"])
        for bad in ("0"*64,"not-a-tool-hash"):
            wrong=copy.deepcopy(value);wrong["goToolSha256"]=bad
            with self.assertRaisesRegex(ValueError,"original Go tool capture hash"):
                materials.checked(materials.encoded(wrong),selected["binarySha256"])

    def test_actual_19_missing_sums_resolve_only_exact_pinned_source_without_binary_commit_claim(self):
        source = materials.official_source()
        actual = source["observedBuildInfo"]
        self.assertEqual(actual["Main"]["Version"], "(devel)")
        self.assertFalse(any(row.get("Sum") for row in actual["Deps"]))
        version, rows = materials.build_modules(materials.encoded(actual), source)
        self.assertEqual(version, "go1.27.3")
        self.assertEqual(len(rows), 19)
        self.assertEqual({(row["path"], row["version"]) for row in rows},
                         {(row["Path"], row["Version"]) for row in actual["Deps"]})
        self.assertTrue(all(row["sum"].startswith("h1:") for row in rows))
        provenance = materials.source_provenance(source)
        self.assertIsNone(provenance["binaryVcsCommit"])
        self.assertFalse(provenance["sourceBuildCorrespondence"])
        self.assertFalse(provenance["authenticatedSignerClaimed"])
        self.assertEqual(provenance["officialSourceCommit"], "0c1bf87a0c9843c30566df0c0aa6593466425ac5")

    def test_missing_exact_source_checksum_row_cannot_resolve_by_path_or_latest_version(self):
        source = materials.official_source()
        actual = source["observedBuildInfo"]
        first = actual["Deps"][0]
        source["_sums"] = "\n".join(line for line in source["_sums"].splitlines()
                                   if not line.startswith(first["Path"] + " " + first["Version"] + " "))
        with self.assertRaisesRegex(ValueError, "exact actual dependency missing"):
            materials.build_modules(materials.encoded(actual), source)

    def test_unknown_material_root_fields_cannot_be_forwarded_as_public_receipt(self):
        value = full_fixture()
        value["unexpected_raw_payload"] = "synthetic-never-publish"
        with self.assertRaisesRegex(ValueError, "actual original collector materials"):
            materials.checked(materials.encoded(value), materials.PIN)

    def test_exact_versions_sums_and_original_raw_info_are_retained(self):
        value = full_fixture()
        self.assertEqual(materials.checked(materials.encoded(value), materials.PIN), value)
        changed = copy.deepcopy(value)
        changed["buildInfoJson"] += " "
        with self.assertRaisesRegex(ValueError, "original Go buildinfo"):
            materials.checked(materials.encoded(changed), materials.PIN)

    def test_local_replace_unresolved_version_conflicting_sum_and_prerelease_go_deny(self):
        cases = []
        for key, value in (("Version", "(devel)"), ("Sum", "h1:foreign"), ("Replace", {"Path": "../local"})):
            row = info()
            row["Deps"][0][key] = value
            cases.append(row)
        row = info()
        row["Main"]["Path"] = "foreign.invalid/project"
        cases.append(row)
        row = info()
        row["GoVersion"] = "go1.26rc1"
        cases.append(row)
        for row in cases:
            with self.subTest(row=row), self.assertRaises(ValueError):
                materials.build_modules(materials.encoded(row), materials.official_source())

    def test_complete_dependency_and_matching_stdlib_coverage_cannot_be_omitted(self):
        for mutate in (lambda value: value["modules"].pop(),
                       lambda value: value["stdlib"].update(version="go1.25.0"),
                       lambda value: value["stdlib"]["materials"].__delitem__(1),
                       lambda value: value["modules"][0]["materials"][0].update(text="changed")):
            value = full_fixture()
            mutate(value)
            with self.assertRaises(ValueError):
                materials.checked(materials.encoded(value), materials.PIN)

    def archive(self, entries):
        raw = io.BytesIO()
        with zipfile.ZipFile(raw, "w") as archive:
            for name, payload, mode in entries:
                row = zipfile.ZipInfo("example.org/public/module@v1.2.3/" + name)
                row.external_attr = mode << 16
                archive.writestr(row, payload)
        return raw.getvalue()

    def test_real_zip_material_bytes_include_nested_notices_without_module_execution(self):
        raw = self.archive([("LICENSE", b"Full synthetic license\n", stat.S_IFREG | 0o644),
                            ("vendor/other/NOTICE", b"Full other notice\n", stat.S_IFREG | 0o644),
                            ("evil.go", b"THIS IS NEVER EXECUTED", stat.S_IFREG | 0o644)])
        actual = materials.zip_materials(raw, "example.org/public/module", "v1.2.3")
        self.assertEqual([row["path"] for row in actual], ["LICENSE", "vendor/other/NOTICE"])
        self.assertEqual(actual[1]["text"], "Full other notice\n")

    def test_empty_authors_is_preserved_byte_exact_with_nonempty_license_coverage(self):
        raw = self.archive([("LICENSE", b"Full synthetic license\n", stat.S_IFREG | 0o644),
                            ("testdata/LICENSE", b"", stat.S_IFREG | 0o644),
                            ("AUTHORS", b"", stat.S_IFREG | 0o644)])
        actual = materials.zip_materials(raw, "example.org/public/module", "v1.2.3")
        empty = next(row for row in actual if row["path"] == "AUTHORS")
        self.assertEqual(empty, dict(path="AUTHORS", text="", sha256=materials.digest(b"")))
        nested = next(row for row in actual if row["path"] == "testdata/LICENSE")
        self.assertEqual(nested, dict(path="testdata/LICENSE", text="", sha256=materials.digest(b"")))
        value = full_fixture()
        value["modules"][0]["materials"] = actual
        self.assertEqual(materials.checked(materials.encoded(value), materials.PIN), value)

    def test_empty_primary_license_cannot_satisfy_module_or_saved_coverage(self):
        for payload in (b"", b" \t\r\n"):
            with self.assertRaises(ValueError) as refused:
                materials.zip_materials(self.archive([("LICENSE", payload, stat.S_IFREG | 0o644),
                                                       ("AUTHORS", b"Public authors\n", stat.S_IFREG | 0o644)]),
                                        "example.org/public/module", "v1.2.3")
            diagnostic = json.loads(str(refused.exception).split(": ", 1)[1])
            self.assertEqual(diagnostic["module"], "example.org/public/module")
            self.assertEqual(diagnostic["version"], "v1.2.3")
            self.assertTrue(all(set(row) == {"path", "bytes", "sha256"} for row in diagnostic["materials"]))
            self.assertNotIn("Public authors", str(refused.exception))
        value = full_fixture()
        value["modules"][0]["materials"] = [dict(path="LICENSE", text="", sha256=materials.digest(b""))]
        with self.assertRaises(ValueError):
            materials.checked(materials.encoded(value), materials.PIN)

    def test_link_escape_missing_full_license_and_oversized_notice_are_not_extracted(self):
        for entries in ([ ("../LICENSE", b"escape", stat.S_IFREG | 0o644) ],
                        [ ("LICENSE", b"target", stat.S_IFLNK | 0o777) ],
                        [ ("README.md", b"license omitted", stat.S_IFREG | 0o644) ],
                        [ ("LICENSE", b"x" * (128 * 1024 + 1), stat.S_IFREG | 0o644) ]):
            with self.subTest(entries=entries[0][0]), self.assertRaises(ValueError):
                materials.zip_materials(self.archive(entries), "example.org/public/module", "v1.2.3")

    def test_real_stdlib_tar_collects_original_full_materials_without_extraction(self):
        raw = io.BytesIO()
        names = ("LICENSE", "PATENTS", "AUTHORS", "CONTRIBUTORS", "src/vendor/public/LICENSE")
        with tarfile.open(fileobj=raw, mode="w:gz") as archive:
            top = tarfile.TarInfo("go/")
            top.type = tarfile.DIRTYPE
            archive.addfile(top)  # The actual parser returns name == "go".
            for name in names:
                payload = ("Synthetic " + name + "\n").encode()
                row = tarfile.TarInfo("go/" + name)
                row.size = len(payload)
                archive.addfile(row, io.BytesIO(payload))
        actual = materials.stdlib_materials(raw.getvalue())
        self.assertEqual({row["path"] for row in actual}, set(names))

    def test_foreign_root_and_exact_go_regular_or_link_still_deny_with_public_metadata(self):
        for name, kind in (("foreign/", tarfile.DIRTYPE), ("go", tarfile.REGTYPE),
                           ("go", tarfile.SYMTYPE), ("go", tarfile.LNKTYPE)):
            raw = io.BytesIO()
            with tarfile.open(fileobj=raw, mode="w:gz") as archive:
                top = tarfile.TarInfo(name)
                top.type = kind
                if kind in (tarfile.SYMTYPE, tarfile.LNKTYPE): top.linkname = "synthetic-public-target"
                archive.addfile(top)
            with self.subTest(name=name, kind=kind), self.assertRaisesRegex(ValueError, "stdlib source root mismatch") as refused:
                materials.stdlib_materials(raw.getvalue())
            diagnostic = json.loads(str(refused.exception).split(": ", 1)[1])
            self.assertEqual(set(diagnostic), {"path", "type", "bytes"})
            self.assertEqual(diagnostic["path"], name.rstrip("/"))
            self.assertNotIn("synthetic-public-target", str(refused.exception))

    def test_modern_go_source_preserves_all_found_and_requires_actual_nonempty_root_materials(self):
        rows = [materials.material("LICENSE", b"Synthetic license\n"), materials.material("PATENTS", b"Synthetic patents\n"),
                materials.material("src/vendor/public/NOTICE", b""), materials.material("testdata/LICENSE", b"")]
        materials.check_stdlib_coverage(rows)
        value = full_fixture()
        value["stdlib"]["materials"] = rows
        self.assertEqual(materials.checked(materials.encoded(value), materials.PIN), value)
        for changed in (rows[1:], [dict(rows[0], text=" ", sha256=materials.digest(b" ")), *rows[1:]]):
            with self.assertRaisesRegex(ValueError, "full Go source license material missing") as refused:
                materials.check_stdlib_coverage(changed)
            diagnostic = json.loads(str(refused.exception).split(": ", 1)[1])
            self.assertEqual(set(diagnostic), {"found", "missing", "empty"})

    def test_self_asserted_source_json_cannot_replace_reviewed_official_pin_or_fetch_network(self):
        with tempfile.TemporaryDirectory(prefix="cc-go-explicit-source-") as directory:
            root = Path(directory).resolve();archive = root / "go1.27.3.src.tar.gz"
            raw = b"SYNTHETIC_PUBLIC_ARCHIVE_BYTES_NO_EXECUTION";archive.write_bytes(raw)
            value = dict(format="cc-official-go-source-input/v1", version="go1.27.3", archiveFile=archive.name,
                archiveUrl="https://go.dev/dl/" + archive.name, archiveFinalUrl="https://dl.google.com/go/" + archive.name,
                listingUrl="https://go.dev/dl/?mode=json&include=all", listingSha256="a" * 64,
                archiveSha256=materials.digest(raw), archiveBytes=len(raw), binaryExecuted=False, legalCertification=False)
            record = root / "source-input.json";record.write_bytes(materials.encoded(value))
            with mock.patch.object(materials, "public_get", side_effect=AssertionError("explicit path must not fetch")):
                # Matching arbitrary JSON+archive SHA is NOT official provenance.
                with self.assertRaisesRegex(ValueError, "reviewed official stdlib source pin required"):
                    materials.explicit_stdlib_source("go1.27.3", record)
                # Even a record quoting the known official pin cannot bless
                # adjacent bytes that are not that original archive.
                value["archiveSha256"] = materials.STDLIB_SOURCE_PIN[1]
                value["archiveBytes"] = materials.STDLIB_SOURCE_PIN[2]
                record.write_bytes(materials.encoded(value))
                with self.assertRaisesRegex(ValueError, "archive bytes changed"):
                    materials.explicit_stdlib_source("go1.27.3", record)
                for key,replacement in (("version", "go1.27.2"), ("archiveFile", "../other"), ("archiveSha256", "b" * 64), ("archiveBytes", len(raw)+1)):
                    changed = dict(value);changed[key] = replacement;record.write_bytes(materials.encoded(changed))
                    with self.subTest(key=key), self.assertRaises(ValueError):materials.explicit_stdlib_source("go1.27.3", record)
                record.write_bytes(materials.encoded(value));archive.write_bytes(raw + b"changed")
                with self.assertRaises(ValueError):materials.explicit_stdlib_source("go1.27.3", record)

    def test_trimmed_tool_receives_selected_root_in_real_child_without_ambient_goroot(self):
        # Synthetic trimmed-tool precondition in a real child process; no Go,
        # collector ELF or network execution. Actual trimmed-Go failure is
        # preserved separately in the owned capture that motivated this case.
        with tempfile.TemporaryDirectory(prefix="cc-trimmed-go-root-") as directory:
            root = Path(directory).resolve()
            selected = root / "official-go"
            (selected / "bin").mkdir(parents=True)
            (selected / "VERSION").write_text("synthetic-official-tool-root\n", encoding="utf-8")
            work = root / "work"
            work.mkdir()
            with mock.patch.dict(os.environ, {"GOROOT": str(root / "foreign"),
                    "GITHUB_TOKEN": "synthetic-never-copy"}):
                env = materials.environment(work, selected / "bin/go")
            check = ("import os,pathlib,sys; "
                     "r=os.environ.get('GOROOT'); "
                     "assert r==sys.argv[1], 'trimmed tool requires selected GOROOT'; "
                     "assert (pathlib.Path(r)/'VERSION').read_text().strip()=='synthetic-official-tool-root'; "
                     "assert 'GITHUB_TOKEN' not in os.environ; print('selected-root-only')")
            actual = materials.command([sys.executable, "-I", "-B", "-c", check, str(selected)],
                                       env, work, time.monotonic() + 10)
            self.assertEqual(actual.strip(), b"selected-root-only")

    def test_relative_or_non_root_bin_tool_is_denied_before_creating_cache(self):
        with tempfile.TemporaryDirectory(prefix="cc-go-root-shape-") as directory:
            root = Path(directory).resolve()
            for selected in (Path("relative/bin/go"), root / "arbitrary-go", root / "bin/foreign"):
                with self.subTest(selected=selected), self.assertRaisesRegex(ValueError, "absolute official Go root/bin/go"):
                    materials.environment(root, selected)
                self.assertEqual(list(root.iterdir()), [])

    def test_missing_materials_refuse_package_and_ambient_go_configuration_is_not_inherited(self):
        with tempfile.TemporaryDirectory(prefix="cc-public-go-materials-") as directory:
            root = Path(directory)
            with self.assertRaisesRegex(ValueError, "complete actual Go"):
                pipeline.bind_collector_materials(root, {"sha256": materials.PIN}, None)
            self.assertEqual(list(root.iterdir()), [])
            with mock.patch.dict(os.environ, {"GOFLAGS": "-toolexec=/private/tool", "GOPROXY": "https://private.invalid",
                    "GITHUB_TOKEN": "synthetic-never-copy", "GOPRIVATE": "private.invalid"}, clear=True):
                actual = materials.environment(root, root / "official-go/bin/go")
            self.assertEqual(actual["GOROOT"], str(root / "official-go"))
            self.assertEqual(actual["GOVCS"], "*:off")
            self.assertEqual(actual["GOTOOLCHAIN"], "local")
            self.assertEqual(actual["GOENV"], "off")
            self.assertEqual(actual["GOPROXY"], "https://proxy.golang.org")
            self.assertEqual(actual["GOSUMDB"], "sum.golang.org")
            self.assertNotIn("GITHUB_TOKEN", actual)
            self.assertEqual(actual["GOFLAGS"], "")


if __name__ == "__main__":
    unittest.main()
