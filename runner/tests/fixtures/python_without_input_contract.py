#!/opt/cc-runtime/bin/python -I
"""Test-only interpreter shim: emulate an older helper's missing capability."""
import os
import sys
from pathlib import Path

assert os.getuid()==10001 and sys.argv[1:3]==['-I','-c']
sys.path.insert(0,'/opt/consolecrypt/src')
from consolecrypt_runner import probe as probe_module

original_probe=probe_module.probe
missing_capability='locked_dependencies_v1' if Path(sys.argv[0]).name=='python-old-dependencies' else 'declared_inputs_v1'


def older_probe():
    result=original_probe()
    result['capabilities'].pop(missing_capability,None)
    return result


probe_module.probe=older_probe
code=sys.argv[3]
sys.argv=sys.argv[3:]
exec(code,{'__name__':'__main__'})
