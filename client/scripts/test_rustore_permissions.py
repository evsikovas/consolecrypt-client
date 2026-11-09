"""Reject dependency-added permissions in actual APK/AAB verification."""
import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location('rustore', Path(__file__).with_name('verify-rustore.py'))
rustore = importlib.util.module_from_spec(spec)
spec.loader.exec_module(rustore)


class StorePermissionsTest(unittest.TestCase):
    def test_qr_notifications_and_biometrics_remain_available(self):
        rustore.verify_store_permissions([
            'android.permission.INTERNET', 'android.permission.CAMERA',
            'android.permission.POST_NOTIFICATIONS', 'android.permission.USE_BIOMETRIC',
        ])

    def test_transitive_recording_storage_and_installer_permissions_fail(self):
        for name in ('REQUEST_INSTALL_PACKAGES', 'RECORD_AUDIO',
                     'READ_EXTERNAL_STORAGE', 'WRITE_EXTERNAL_STORAGE'):
            with self.subTest(permission=name), self.assertRaises(AssertionError):
                rustore.verify_store_permissions(iter(['android.permission.CAMERA', 'android.permission.' + name]))


if __name__ == '__main__':
    unittest.main()
