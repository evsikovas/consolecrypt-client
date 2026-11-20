import unittest
from consolecrypt_runner.diagnostic_mask import KnownJobSecrets
class DiagnosticByteWorkTests(unittest.TestCase):
    def test_long_common_prefix_cost_precedes_any_comparison(self):
        prefix='a'*65535
        mask=KnownJobSecrets({'extra_vars':{'values':[prefix+chr(s) for s in range(ord('b'),ord('q')+1)]}})
        self.assertTrue(mask.live);self.assertEqual(len(mask.values),16)
        work=[65536]
        with self.assertRaises(ValueError):mask.text(prefix+'z',work)
        self.assertEqual(work,[65536],'reject before first long-prefix comparison')
        mask.close()
        exact='s'*65536;mask=KnownJobSecrets({'extra_vars':{'secret':exact}})
        work=[1]
        with self.assertRaises(ValueError):mask.text(exact,work)
        self.assertEqual(work,[1])
        work=[65537];self.assertEqual(mask.text(exact,work),'[REDACTED]');self.assertEqual(work,[0]);mask.close()
