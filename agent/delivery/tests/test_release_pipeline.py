"""Public bytes/descriptor tests only; no real signing keys or host provisioning."""
import hashlib
import importlib.util
import json
import sys
import types
from pathlib import Path
import tempfile
import time
import unittest
from unittest import mock

DELIVERY = Path(__file__).resolve().parents[1]


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


pipeline = load("candidate_pipeline", DELIVERY / "release_pipeline.py")
bootstrap = load("public_bootstrap", DELIVERY / "roles/consolecrypt_agent_release_trust/files/bootstrap_release.py")
normal_producer = load("current_normal_prometheus_producer", DELIVERY / "prometheus_build.py")


class CargoArchiveCacheRoot(unittest.TestCase):
    def test_actual_package_honors_explicit_cargo_home_and_default_home(self):
        # Execute the package path up to notice generation. The selected actual
        # archive directory must come from CARGO_HOME, not unrelated HOME data.
        import notice_generate
        class ReachedNotices(Exception):
            pass
        with tempfile.TemporaryDirectory(prefix="owned-cargo-cache-")as tmp:
            root=Path(tmp);home=root/"home";explicit=root/"cargo"
            default=home/".cargo/registry/cache/index.crates.io-default"
            selected=explicit/"registry/cache/index.crates.io-explicit"
            for path in(default,selected):path.mkdir(parents=True)
            for number,use_explicit in enumerate((True,False)):
                with self.subTest(explicit=use_explicit):
                    work=root/("work"+str(number));work.mkdir()
                    (work/"source-before.json").write_text(json.dumps(dict(
                        target="x86_64-unknown-linux-gnu",source={},dependencySourceBefore={})))
                    (work/"dependency-metadata-after.json").write_text("{}")
                    (work/"cargo-build.jsonl").write_text("synthetic captured build log\n")
                    artifacts={}
                    for name in("cc-agent-identity","cc-agent-release-sign"):
                        source=root/(name+str(number));source.write_bytes(b"synthetic public build output")
                        artifacts[name]=dict(path=str(source),sha256=pipeline.sha(source),bytes=source.stat().st_size)
                    environment={"CARGO_HOME":str(explicit)}if use_explicit else{}
                    def notices(agent,cache,descriptor,**kwargs):
                        self.assertEqual(cache,selected if use_explicit else default)
                        raise ReachedNotices()
                    with mock.patch.dict(pipeline.os.environ,environment,clear=True), \
                         mock.patch.object(pipeline.Path,"home",return_value=home), \
                         mock.patch.object(pipeline,"source_map",return_value={}), \
                         mock.patch.object(pipeline,"dependency_frame",return_value={}), \
                         mock.patch.object(pipeline,"capture_collector",return_value={}), \
                         mock.patch.object(pipeline,"bind_collector_materials"), \
                         mock.patch.object(pipeline,"artifacts",return_value=artifacts), \
                         mock.patch.object(pipeline.subprocess,"check_output",return_value="synthetic captured metadata"), \
                         mock.patch.object(notice_generate,"generate",side_effect=notices)as generate:
                        with self.assertRaises(ReachedNotices):
                            pipeline.package(work,"1","0.1.0","0",None,None)
                        generate.assert_called_once()


class NormalProducerPlatform(unittest.TestCase):
    def test_native_platforms_reach_explicit_offline_input_guard(self):
        for machine in ("x86_64", "aarch64"):
            with self.subTest(machine=machine), tempfile.TemporaryDirectory(prefix="owned-normal-start-")as tmp:
                output=Path(tmp)/"normal"
                with mock.patch.object(normal_producer.sys,"platform","linux"), \
                     mock.patch.object(normal_producer.os,"uname",return_value=types.SimpleNamespace(machine=machine),create=True), \
                     mock.patch.object(normal_producer.os,"getuid",return_value=10001,create=True), \
                     mock.patch.object(normal_producer.os,"geteuid",return_value=10001,create=True), \
                     mock.patch.object(normal_producer,"fetch")as fetch:
                    with self.assertRaisesRegex(ValueError,"explicit complete offline inputs required"):
                        normal_producer.produce(output,offline=True)
                    fetch.assert_not_called()
                self.assertFalse(output.exists())

    def test_failure_diagnostic_uses_same_closed_native_platform(self):
        for machine in ("x86_64", "aarch64"):
            with self.subTest(machine=machine), \
                 mock.patch.object(normal_producer.os,"uname",return_value=types.SimpleNamespace(machine=machine),create=True), \
                 mock.patch.object(normal_producer,"group_members",return_value={}):
                self.assertEqual(normal_producer.compiler_failure(["/fixed/go/bin/go","build"],123),
                                 dict(records=[],unavailable=0))

    def test_public_import_preserves_checksum_bound_metadata_beyond_root_zip_tuples(self):
        # MVS metadata is not a new ZIP/source whitelist. Exact .mod/.info
        # candidates come from original go.sum, including non-root versions.
        with tempfile.TemporaryDirectory(prefix="owned-normal-metadata-")as tmp:
            root=Path(tmp);source=root/"public";source.mkdir();dest=root/"copied"
            sums={("example.invalid/root","v1.0.0/go.mod"):"h1:root",
                  ("example.invalid/history","v0.9.0/go.mod"):"h1:metadata"}
            expected={}
            for module,version in sums:
                version=version.removesuffix("/go.mod")
                for suffix,body in (("mod",("module "+module+"\n").encode()),("info",json.dumps(dict(Version=version)).encode())):
                    name="cache/download/"+module+"/@v/"+version+"."+suffix
                    path=source/name;path.parent.mkdir(parents=True,exist_ok=True);path.write_bytes(body);expected[name]=body
            normal_producer.import_modules(source,dest,sums,[],time.monotonic()+10)
            self.assertEqual({p.relative_to(dest).as_posix():p.read_bytes()for p in dest.rglob("*")if p.is_file()},expected)
            # An unrelated .info label has no original checksum authority and
            # must never be imported as another go.mod or source dependency.
            extra=source/"cache/download/example.invalid/unbound/@v/v2.0.0.info"
            extra.parent.mkdir(parents=True);extra.write_text('{"Version":"v2.0.0"}')
            again=root/"again";normal_producer.import_modules(source,again,sums,[],time.monotonic()+10)
            self.assertFalse((again/extra.relative_to(source)).exists())

    def test_compile_uses_one_package_and_bounded_soft_heap_while_retaining_own_data(self):
        with tempfile.TemporaryDirectory(prefix="owned-normal-compile-")as tmp:
            output=Path(tmp);cache=output/'cache';cache.mkdir();data=cache/'buildcache';data.mkdir()
            marker=data/'retained';marker.write_bytes(b'owned compiler DATA, never accepted as an ELF')
            artifacts=output/'artifacts';artifacts.mkdir();calls=[]
            def command(argv,label,env,limit):
                calls.append((argv,label,env,limit));raise ValueError('controlled compile refusal')
            row=normal_producer.release_platforms.select('x86_64-unknown-linux-gnu','target')
            with self.assertRaisesRegex(ValueError,'controlled compile refusal'):
                normal_producer.compile_and_finish(output,cache,output/'scratch',output/'source',artifacts,output/'inputs',
                    cache/'go/bin/go',command,{'GOPROXY':'off'},1,row,{}, {},'1'*64,{'sha256':'2'*64},[],{},time.monotonic()+10)
            self.assertEqual(len(calls),1)
            argv,label,env,limit=calls[0]
            self.assertEqual(label,'normal-build');self.assertIn('-p=1',argv);self.assertNotIn('-p=2',argv)
            self.assertEqual((env['GOMAXPROCS'],env['GOMEMLIMIT'],limit),('2','1GiB',6*1024**3))
            self.assertEqual(marker.read_bytes(),b'owned compiler DATA, never accepted as an ELF')
            self.assertFalse((artifacts/'prometheus').exists())
            self.assertFalse(json.loads((output/'compile-failure.json').read_bytes())['compiledELFAccepted'])

    def test_compile_resume_rejects_wrong_origin_before_go_or_mutation(self):
        with tempfile.TemporaryDirectory(prefix="owned-normal-origin-")as tmp:
            output=Path(tmp);origin=output/'compiler-cache-origin.json';origin.write_bytes(b'{}')
            with mock.patch.object(normal_producer.sys,'platform','linux'), \
                 mock.patch.object(normal_producer.os,'getuid',return_value=10001,create=True), \
                 mock.patch.object(normal_producer.os,'geteuid',return_value=10001,create=True), \
                 mock.patch.object(normal_producer,'run')as run:
                with self.assertRaisesRegex(ValueError,'owned compiler origin changed'):
                    normal_producer.resume_compile(output,'0'*64,'x86_64-unknown-linux-gnu')
                run.assert_not_called()
            self.assertEqual(origin.read_bytes(),b'{}')
            self.assertFalse((output/'artifacts-before-compile-resume').exists())

    def test_resume_environment_uses_actual_fresh_helper_with_existing_owned_caches(self):
        with tempfile.TemporaryDirectory(prefix="owned-normal-real-env-")as tmp:
            output=Path(tmp);cache=output/'cache';cache.mkdir()
            for name in('home','gopath','modcache','buildcache','tmp'):
                directory=cache/name;directory.mkdir();(directory/'retained').write_bytes(name.encode())
            artifacts=output/'artifacts';artifacts.mkdir();(artifacts/'original').write_bytes(b'original gates')
            go=cache/'go/bin/go'
            # This is the actual shared materials.environment, with no stub.
            with self.assertRaises(FileExistsError):normal_producer.materials.environment(cache,go)
            env=normal_producer.resume_environment(cache,go,output/'scratch','amd64')
            again=normal_producer.resume_environment(cache,go,output/'scratch','amd64')
            self.assertNotEqual(env['HOME'],again['HOME'])
            self.assertEqual((env['GOMODCACHE'],env['GOCACHE']),(str(cache/'modcache'),str(cache/'buildcache')))
            self.assertEqual((env['GOROOT'],env['GOPROXY'],env['GOSUMDB'],env['GOMEMLIMIT']),
                             (str(cache/'go'),'off','off','1GiB'))
            self.assertTrue(Path(env['HOME']).is_dir())
            self.assertEqual((artifacts/'original').read_bytes(),b'original gates')
            self.assertFalse((output/'artifacts-before-compile-resume').exists())
            for name in('home','gopath','modcache','buildcache','tmp'):
                self.assertEqual((cache/name/'retained').read_bytes(),name.encode())


class CompleteStandardNotices(unittest.TestCase):
    def test_measured_compressed_package_and_component_limits_remain_finite(self):
        from notice_inventory import validate_notice_inventory,INVENTORY
        value=dict(format='cc-agent-notice-inventory/v1',components=[{'notices':[]}for _ in range(512)])
        self.assertEqual(validate_notice_inventory({INVENTORY:json.dumps(value).encode()})['components'],512)
        value['components'].append({'notices':[]})
        with self.assertRaises(ValueError):validate_notice_inventory({INVENTORY:json.dumps(value).encode()})
        with tempfile.TemporaryDirectory(prefix='owned-compressed-bound-')as tmp:
            artifact=Path(tmp)/'candidate.zip'
            with artifact.open('wb')as output:output.truncate(96*1024**2+1)
            with self.assertRaisesRegex(ValueError,'compressed package budget'):pipeline.candidate_budget(artifact)

    def test_exact_shared_cc0_document_and_known_standard_choices_keep_full_text(self):
        import notice_generate as notices
        cache_root=Path(__import__('os').environ.get('CARGO_HOME',str(Path.home()/'.cargo')))/'registry/cache'
        archive=next(cache_root.glob('*/bip39-3.0.0.crate'))
        raw=archive.read_bytes()
        with tempfile.TemporaryDirectory(prefix='owned-cc0-notice-')as tmp:
            cache=Path(tmp);(cache/archive.name).write_bytes(raw);captured=[]
            def notice(name,body):captured.append((name,body));return dict(original_path=name,sha256=hashlib.sha256(body).hexdigest())
            entry,provenance=notices.shared_cc0_notice('bitcoin_hashes','0.14.101',cache,notices.read,notice)
            self.assertEqual(len(captured[0][1]),7049)
            self.assertEqual(entry['sha256'],'7179683e8000e6bdc9bbc60d85edf0a4ac8e76f951857f54fcb775d5886f1309')
            self.assertEqual(provenance['archive_sha256'],hashlib.sha256(raw).hexdigest())
            self.assertIn('omits',captured[0][0])
            (cache/archive.name).write_bytes(raw+b'changed')
            with self.assertRaises(ValueError):notices.shared_cc0_notice('bitcoin_hashes','0.14.101',cache,notices.read,notice)
            with self.assertRaises(ValueError):notices.shared_cc0_notice('unreviewed','1.0.0',cache,notices.read,notice)
        self.assertEqual(notices.chosen('CC0-1.0'),'CC0-1.0')
        self.assertEqual(notices.chosen('Unicode-3.0'),'Unicode-3.0')
        self.assertEqual(notices.chosen('Zlib OR Apache-2.0 OR MIT'),'MIT')
        with self.assertRaises(ValueError):notices.chosen('unreviewed-license')


class RuntimeVersionStamp(unittest.TestCase):
    def test_stamped_module_preserves_source_and_binds_original_and_packaged_hashes(self):
        from prototype_package import stamp_runtime_version,runtime_declared_version,RUNTIME_VERSION_FILE
        source=DELIVERY.parent/RUNTIME_VERSION_FILE;raw=source.read_bytes()
        packaged,record=stamp_runtime_version(raw,"0.2.0")
        self.assertEqual(runtime_declared_version(raw),"0.1.0")
        self.assertEqual(runtime_declared_version(packaged),"0.2.0")
        self.assertEqual(source.read_bytes(),raw)
        self.assertEqual(record["sourceSha256"],hashlib.sha256(raw).hexdigest())
        self.assertEqual(record["packagedSha256"],hashlib.sha256(packaged).hexdigest())
        self.assertEqual(record["generation"],"exact-version-assignment/v1")
        self.assertEqual(raw.splitlines()[4:],packaged.splitlines()[4:]) # imports/__all__ unchanged

    def test_ambiguous_or_dynamic_version_and_invalid_display_do_not_execute_or_stamp(self):
        from prototype_package import stamp_runtime_version
        for raw in[b'__version__ = "0.1.0"\n__version__ = "0.2.0"\n',
                    b'__version__ = dangerous()\n',b'from somewhere import __version__\n__version__ = "0.1.0"\n']:
            with self.assertRaises(ValueError):stamp_runtime_version(raw,"0.2.0")
        with self.assertRaises(ValueError):stamp_runtime_version(b'__version__ = "0.1.0"\n',"0.2.0\ninject")

    def test_old_imported_heartbeat_keeps_old_version_and_new_import_reports_packaged_code(self):
        # Actual heartbeat method, no listener/auth/process claim. The existing
        # service imports the package constant ONCE, so selecting another
        # signed build cannot rewrite an already-running object's observation.
        from prototype_package import stamp_runtime_version,RUNTIME_VERSION_FILE
        raw=(DELIVERY.parent/RUNTIME_VERSION_FILE).read_bytes();packaged,_=stamp_runtime_version(raw,"0.2.0")
        loaded=[]
        def runtime(name,path):
            spec=importlib.util.spec_from_file_location(name,path,submodule_search_locations=[str(DELIVERY.parent/"runtime/cc_agent")])
            module=importlib.util.module_from_spec(spec);sys.modules[name]=module;loaded.append(name);spec.loader.exec_module(module)
            service=sys.modules[name+".service"];value=service.AgentRuntime.__new__(service.AgentRuntime)
            value.manifest=types.SimpleNamespace(agent_id="public-agent",host_id="public-host")
            value.started=0;value.monotonic=lambda:1;value.clock=lambda:1;value.audit_log=None
            value._collection=lambda:{"state":"unknown"}
            return value
        try:
            with tempfile.TemporaryDirectory(prefix="owned-runtime-stamp-")as tmp:
                path=Path(tmp)/"__init__.py";path.write_bytes(raw)
                old=runtime("owned_runtime_version_old",path)
                path.write_bytes(packaged)
                new=runtime("owned_runtime_version_new",path)
                self.assertEqual(old.heartbeat()["version"],"0.1.0")
                self.assertEqual(new.heartbeat()["version"],"0.2.0")
                self.assertEqual(old.heartbeat()["host_binding"],"configured_not_identity_attested")
        finally:
            for key in list(sys.modules):
                if any(key==name or key.startswith(name+".")for name in loaded):sys.modules.pop(key,None)


class PublicReleaseInputs(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="cc-public-release-test-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()
        self.elf = b"\x7fELF\x02\x01" + b"\0" * 12 + bytes([62, 0])
        self.verifier = self.root / "public-verifier"
        self.verifier.write_bytes(self.elf)
        self.key = self.root / "public-key.json"
        # Synthetic public data only; this checker claims no curve/signature proof.
        self.key.write_text(json.dumps({"format": 1, "publicKey": "07" * 32}), encoding="utf-8")
        self.fingerprint = hashlib.sha256(b"consolecrypt.agent.public-key.v1\0" + bytes([7] * 32)).hexdigest()

    def check(self, **overrides):
        args = dict(verifier=self.verifier, public_key=self.key,
            expected_verifier=pipeline.sha(self.verifier), expected_key=pipeline.sha(self.key),
            expected_fingerprint=self.fingerprint, platform="linux-x86_64")
        args.update(overrides)
        return bootstrap.inspect(**args)

    def test_public_checker_binds_actual_files_and_fixed_trust_schema_without_writes(self):
        before = {p.name: p.read_bytes() for p in self.root.iterdir()}
        actual = self.check()
        trust = json.loads(actual["trustJson"])
        self.assertEqual(set(trust), {"version", "verifierSha256", "publicKeySha256", "platform"})
        self.assertFalse(actual["signatureVerificationClaimed"])
        self.assertFalse(actual["hostChanged"])
        self.assertEqual(before, {p.name: p.read_bytes() for p in self.root.iterdir()})

    def test_unknown_signer_or_changed_verifier_cannot_be_bootstrapped(self):
        with self.assertRaises(ValueError):
            self.check(expected_fingerprint="a" * 64)
        original = pipeline.sha(self.verifier)
        self.verifier.write_bytes(self.elf + b"changed")
        with self.assertRaises(ValueError):
            self.check(expected_verifier=original)

    def test_duplicate_or_private_key_fields_are_not_a_release_public_pin(self):
        for value in [b'{"format":1,"format":1,"publicKey":"' + b"07" * 32 + b'"}',
                json.dumps({"format": 1, "publicKey": "07" * 32, "seed": "never-accepted"}).encode()]:
            self.key.write_bytes(value)
            with self.assertRaises(ValueError):
                self.check()

    def test_wrong_elf_architecture_and_placeholder_template_are_refused(self):
        with self.assertRaises(ValueError):
            self.check(platform="linux-aarch64")
        with self.assertRaises(ValueError):
            self.check(expected_verifier="REPLACE_WITH_VERIFIER_SHA256")

    def test_hardlink_public_input_is_not_adopted(self):
        linked = self.root / "linked-key"
        import os
        os.link(self.key, linked)
        with self.assertRaises(ValueError):
            self.check()

    def cargo_log(self, *, extra=False, success=True, machine=62):
        target = "x86_64-unknown-linux-gnu"
        directory = self.root / target / "release"
        directory.mkdir(parents=True, exist_ok=True)
        rows = []
        for name in ("cc-agent-identity", "cc-agent-release-sign"):
            path = directory / name
            path.write_bytes(self.elf[:18] + machine.to_bytes(2, "little"))
            rows.append({"reason": "compiler-artifact", "target": {"name": name}, "profile": {"test": False, "opt_level": "3"}, "executable": str(path)})
        if extra:
            rows.append(rows[0])
        rows.append({"reason": "build-finished", "success": success})
        path = self.root / "cargo.jsonl"
        path.write_text("\n".join(json.dumps(r) for r in rows), encoding="utf-8")
        return path, target

    def test_descriptor_selects_exact_actual_normal_agent_and_separate_signer(self):
        log, target = self.cargo_log()
        actual = pipeline.artifacts(log, target)
        self.assertEqual(set(actual), {"cc-agent-identity", "cc-agent-release-sign"})
        for row in actual.values():
            self.assertEqual(row["sha256"], pipeline.sha(Path(row["path"])))

    def test_failed_duplicate_and_foreign_arch_build_receipts_are_refused(self):
        for options in ({"extra": True}, {"success": False}, {"machine": 183}):
            log, target = self.cargo_log(**options)
            with self.assertRaises(ValueError):
                pipeline.artifacts(log, target)

    def release_outputs(self):
        (self.root / "signed").mkdir()
        unsigned = self.root / "unsigned-release.json"
        unsigned.write_text(json.dumps({"format": 1, "version": 2}), encoding="utf-8")
        (self.root / "build-public.json").write_text("{}", encoding="utf-8")
        (self.root / "package-public.json").write_text(json.dumps({"unsignedManifestSha256": pipeline.sha(unsigned)}), encoding="utf-8")
        return unsigned

    def test_replaced_output_signer_is_denied_before_any_verifier_execution(self):
        self.release_outputs()
        (self.root / "signed/public-key.json").write_text(json.dumps({"format": 1, "publicKey": "08" * 32}), encoding="utf-8")
        with mock.patch.object(pipeline.os, "environ", {"CC_AGENT_RELEASE_EXPECTED_PUBLIC_KEY": "07" * 32}), mock.patch.object(pipeline.subprocess, "run") as execute:
            with self.assertRaisesRegex(ValueError, "independently approved release pin"):
                pipeline.self_verify(self.root, "1")
            execute.assert_not_called()

    def test_changed_original_unsigned_body_is_denied_before_any_verifier_execution(self):
        unsigned = self.release_outputs()
        unsigned.write_text(json.dumps({"format": 1, "version": 3}), encoding="utf-8")
        with mock.patch.object(pipeline.subprocess, "run") as execute:
            with self.assertRaisesRegex(ValueError, "original unsigned release changed"):
                pipeline.self_verify(self.root, "1")
            execute.assert_not_called()


class SupervisorRuntimePinInputs(unittest.TestCase):
    def pin(self,path,executable="/usr/bin/python3.12",digest="a"*64):
        raw=json.dumps(dict(format="cc-tunnel-supervisor-runtime/v1",pythonExecutable=executable,pythonSha256=digest),indent=2).encode()
        path.write_bytes(raw);return raw

    def test_reviewed_runtime_pin_is_copied_exactly_for_both_supported_system_paths(self):
        import prototype_package
        for executable in ("/usr/bin/python3.11","/usr/bin/python3.12"):
            with tempfile.TemporaryDirectory(prefix="owned-system-python-pin-")as tmp:
                root=Path(tmp);work=root/"work";work.mkdir();pin=root/"reviewed.json"
                raw=self.pin(pin,executable)
                captured=pipeline.capture_supervisor_runtime(work,pin)
                self.assertEqual(Path(captured["path"]).read_bytes(),raw)
                self.assertEqual(prototype_package.public_runtime_pin(pin),raw)
                self.assertEqual(captured["sha256"],hashlib.sha256(raw).hexdigest())
                self.assertEqual(captured["pythonExecutable"],executable)
                pipeline.recheck_supervisor_runtime(pin,captured)

    def test_reviewed_pin_mutation_after_capture_denies_without_rebinding(self):
        with tempfile.TemporaryDirectory(prefix="owned-system-python-pin-")as tmp:
            root=Path(tmp);work=root/"work";work.mkdir();pin=root/"reviewed.json"
            original=self.pin(pin);captured=pipeline.capture_supervisor_runtime(work,pin)
            self.pin(pin,digest="b"*64)
            with self.assertRaisesRegex(ValueError,"reviewed supervisor runtime input changed"):
                pipeline.recheck_supervisor_runtime(pin,captured)
            self.assertEqual(Path(captured["path"]).read_bytes(),original)
            # The original input may remain intact while an owned work copy
            # changes after notices/package capture: that must deny too.
            self.pin(pin)
            self.pin(Path(captured["path"]),digest="c"*64)
            with self.assertRaisesRegex(ValueError,"reviewed supervisor runtime copy changed"):
                pipeline.recheck_supervisor_runtime(pin,captured)
            self.assertEqual(pin.read_bytes(),original)

    def test_unknown_runtime_paths_duplicate_fields_and_zero_digest_are_denied(self):
        import prototype_package
        with tempfile.TemporaryDirectory(prefix="owned-system-python-pin-")as tmp:
            pin=Path(tmp)/"reviewed.json"
            for executable in ("python3","/usr/bin/python3","/usr/bin/python3.13","/tmp/python3.12","/usr/bin/python3.12 -c pass"):
                self.pin(pin,executable)
                with self.assertRaisesRegex(ValueError,"unsupported supervisor runtime pin"):
                    prototype_package.public_runtime_pin(pin)
            self.pin(pin,digest="0"*64)
            with self.assertRaisesRegex(ValueError,"unsupported supervisor runtime pin"):
                prototype_package.public_runtime_pin(pin)
            pin.write_bytes(b'{"format":"cc-tunnel-supervisor-runtime/v1","pythonExecutable":"/usr/bin/python3.11","pythonExecutable":"/usr/bin/python3.12","pythonSha256":"a"}')
            with self.assertRaisesRegex(ValueError,"duplicate supervisor runtime pin field"):
                prototype_package.public_runtime_pin(pin)

    def test_absent_runtime_pin_remains_absent_without_host_interpreter_discovery(self):
        with tempfile.TemporaryDirectory(prefix="owned-no-python-pin-")as tmp:
            with mock.patch.object(pipeline.subprocess,"run",side_effect=AssertionError("must not execute or discover interpreter")):
                self.assertIsNone(pipeline.capture_supervisor_runtime(Path(tmp),None))
                pipeline.recheck_supervisor_runtime(None,None)
            self.assertEqual(list(Path(tmp).iterdir()),[])


if __name__ == "__main__":
    unittest.main()
