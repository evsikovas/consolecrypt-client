"""DRAFT actual O_EXCL component failures; no Root/Ansible/systemd acceptance."""
import importlib.util
import os
from pathlib import Path
import tempfile
import unittest

SOURCE=Path(__file__).resolve().parents[2]/'delivery/roles/consolecrypt_agent_outbound_control/files/outbound_control_publish.py'
SPEC=importlib.util.spec_from_file_location('owned_outbound_publication_draft',SOURCE)
publisher=importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(publisher)


class OutboundPublicationTests(unittest.TestCase):
    def test_bare_equal_unit_after_absence_check_is_not_adopted_or_receipted(self):
        with tempfile.TemporaryDirectory(prefix='cc-outbound-publication-owned-') as temporary:
            root=Path(temporary);parent=os.open(root,os.O_RDONLY|os.O_DIRECTORY)
            owned=[];effect={'attempted':False}
            try:
                self.assertFalse(publisher._exists(parent,publisher._UNIT_NAME))
                # Another publisher wins AFTER our earlier absence observation.
                unit=root/publisher._UNIT_NAME;unit.write_bytes(b'exact same desired unit')
                with self.assertRaises(FileExistsError):publisher._create_pair(parent,parent,effect,owned)
                self.assertTrue(effect['attempted']);self.assertEqual(owned,[])
                self.assertEqual(unit.read_bytes(),b'exact same desired unit')
                self.assertFalse((root/publisher._OWNER_NAME).exists())
            finally:
                for fd in owned:os.close(fd)
                os.close(parent)

    def test_second_exclusive_create_failure_retains_partial_without_foreign_receipt_write(self):
        with tempfile.TemporaryDirectory(prefix='cc-outbound-publication-owned-') as temporary:
            root=Path(temporary);parent=os.open(root,os.O_RDONLY|os.O_DIRECTORY)
            owner=root/publisher._OWNER_NAME;owner.write_bytes(b'foreign or partial receipt')
            owned=[];effect={'attempted':False}
            try:
                with self.assertRaises(FileExistsError):publisher._create_pair(parent,parent,effect,owned)
                self.assertTrue(effect['attempted']);self.assertEqual(len(owned),1)
                self.assertEqual(owner.read_bytes(),b'foreign or partial receipt')
                self.assertEqual((root/publisher._UNIT_NAME).read_bytes(),b'')
            finally:
                for fd in owned:os.close(fd)
                os.close(parent)

    def test_equal_replacement_unit_cannot_match_original_inode_bound_receipt(self):
        with tempfile.TemporaryDirectory(prefix='cc-outbound-publication-owned-') as temporary:
            root=Path(temporary);path=root/publisher._UNIT_NAME;path.write_bytes(b'equal unit')
            original=os.open(path,os.O_RDONLY)
            try:
                before=os.fstat(original);facts=publisher._facts(991,991,'192.0.2.1',b'equal unit',before)
                path.rename(root/'retained-original');path.write_bytes(b'equal unit')
                current=publisher._facts(991,991,'192.0.2.1',b'equal unit',path.stat())
                self.assertEqual(facts['unitSha256'],current['unitSha256'])
                self.assertNotEqual(facts,current)
            finally:os.close(original)


if __name__=='__main__':unittest.main()
