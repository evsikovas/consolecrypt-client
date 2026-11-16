"""Actual maintained SOURCES parsing; no packager import or archive discovery."""
import ast
import importlib.util
from pathlib import Path
import unittest

AGENT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("fixed_source_check", AGENT / "scripts/check_source.py")
checker = importlib.util.module_from_spec(spec)
spec.loader.exec_module(checker)


class PackageSourceClosure(unittest.TestCase):
    def test_actual_literal_can_grow_without_stale_numeric_gate(self):
        # This is the actual maintained list, not a parallel hand-written list.
        # Applied tests use the repository directly; a sparse stage may provide
        # the actual packager via the explicit SOURCE_ROOT test variable only.
        import os
        source_root = Path(os.environ.get("CC_TEST_PACKAGE_SOURCE_ROOT", str(AGENT)))
        tree = ast.parse((source_root / "delivery/prototype_package.py").read_text(encoding="utf-8"))
        value = next(n.value for n in tree.body if isinstance(n, ast.Assign)
                     and any(isinstance(t, ast.Name) and t.id == "SOURCES" for t in n.targets))
        original = ast.literal_eval(value)
        files = {name: {} for name in original}
        literal = "SOURCES = " + repr(original) + "\nraise RuntimeError('must not import')\n"
        self.assertEqual(checker.package_sources(literal, files), original)
        name = "delivery/owned_new_helper.py"
        files[name] = {}
        added = original + (name,)
        self.assertEqual(checker.package_sources("SOURCES = " + repr(added), files), added)

    def test_missing_duplicate_and_private_members_refuse_instead_of_discovery(self):
        files = {"runtime/cc_agent/__main__.py": {}, "tests/private.json": {}, "../escape.py": {}}
        for names in (("runtime/cc_agent/missing.py",), ("runtime/cc_agent/__main__.py",) * 2,
                      ("tests/private.json",), ("../escape.py",), ("identity/target/output.elf",), (),
                      tuple("runtime/cc_agent/file%d.py" % n for n in range(129))):
            with self.subTest(names=names[:2]), self.assertRaises(ValueError):
                checker.package_sources("SOURCES = " + repr(names), files)

    def test_computed_reassigned_or_augmented_sources_are_not_fixed_inputs(self):
        for text in ("SOURCES = tuple(['runtime/a.py'])", "SOURCES = ['runtime/a.py']",
                     "SOURCES = ('runtime/a.py',)\nSOURCES += ('runtime/b.py',)",
                     "SOURCES = ('runtime/a.py',)\nSOURCES = ('runtime/b.py',)"):
            with self.subTest(text=text), self.assertRaises(ValueError):
                checker.package_sources(text, {"runtime/a.py": {}, "runtime/b.py": {}})


if __name__ == "__main__":
    unittest.main()
