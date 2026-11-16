import copy,importlib.util,unittest
from pathlib import Path
spec=importlib.util.spec_from_file_location('service_host_preflight',Path(__file__).with_name('preflight.py'));m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m)
class HostTests(unittest.TestCase):
 def test_arm_observation_must_match_declared_arm_not_amd_or_alias(self):
  expected,observed=self.facts();expected['target']['arch']='aarch64';observed['arch']='aarch64';m.host(expected,observed)
  for architecture in ('x86_64','arm64','riscv64'):
   with self.subTest(architecture=architecture):
    self.assertRaises(ValueError,m.host,expected,dict(observed,arch=architecture))
 def facts(self):
  expected={'target':{'system':'Linux','arch':'x86_64','libc':'glibc','minGlibc':'2.34','distribution':'ubuntu','distributionVersion':'24.04','minSystemd':255},'runtime':{'prefix':'/opt/cc-runtime','python':'3.12.3'}}
  observed={'system':'Linux','arch':'x86_64','libc':'glibc','libcVersion':'2.39','distribution':'ubuntu','distributionVersion':'24.04','systemd':255,'prefix':'/opt/cc-runtime','python':'3.12.3'}
  return expected,observed
 def test_exact_declared_supported_host_and_semantic_glibc_compare(self):
  expected,observed=self.facts();m.host(expected,observed);self.assertGreater(m.version('2.39'),m.version('2.9'));self.assertEqual(m.version('2.34'),m.version('2.34.0'))
 def test_wrong_arch_libc_floor_manager_and_private_interpreter_refused(self):
  expected,observed=self.facts()
  for key,value in [('system','Windows'),('arch','aarch64'),('libc','musl'),('libcVersion','2.33'),('systemd',254),('prefix','/usr'),('python','3.14.0'),('distribution','debian'),('distributionVersion','22.04')]:
   with self.subTest(key=key):bad=dict(observed);bad[key]=value;self.assertRaises(ValueError,m.host,expected,bad)
if __name__=='__main__':unittest.main(verbosity=2)
