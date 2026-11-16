"""ONE measured execution-loader call on its own private socket.
No credential ID, host, path, command, generic resolver or material caching.
"""
import threading
import os
import sys
from .broker import _PrivateSocketBroker
from .errors import require
from .protocol import uuid_field
from .vault_validation_broker import checked_password_response, _digest

def job_capsule(value):
    require(isinstance(value,dict) and set(value)=={'version','purpose','job_id','manifest_digest','revision_digest','reference_sha256'}
            and type(value['version']) is int and value['version']==1 and value['purpose']=='ansible_vault_password'
            and all(_digest(value[key]) for key in ('manifest_digest','revision_digest','reference_sha256')),
            'vault_job','Invalid fixed job material capsule')
    uuid_field(value['job_id'],'job ID')
    return dict(value)

class VaultJobBroker(_PrivateSocketBroker):
    def __init__(self,directory,capsule,channel,cancel,*,keep_owned_directory=False):
        require(sys.platform=='linux' and os.getuid()!=0,'vault_platform','Linux nonroot Vault broker required')
        capsule=job_capsule(capsule)
        taken=False
        take=threading.Lock()
        def handle(request):
            nonlocal taken
            require(isinstance(request,dict) and set(request)=={'version','operation'} and type(request['version']) is int
                    and request['version']==1 and request['operation']=='ansible_vault_password',
                    'vault_request','Unsupported private Vault client request')
            with take:
                require(not taken and not cancel.is_set(),'vault_unavailable','Scoped Vault input unavailable')
                taken=True
            # Measured pinned execution count is ONE. Failure never makes a
            # second Native lookup or converts this to a reusable resolver.
            return checked_password_response(channel.call_job(capsule,cancel=cancel,timeout=5))
        super().__init__(directory,handle,channel.revoke,cancel,slots_limit=1,
            error_message='Scoped Vault input unavailable',frame_limit=1024,handoff_seconds=5,keep_owned_directory=keep_owned_directory)
