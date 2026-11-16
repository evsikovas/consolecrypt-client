"""Private phase socket seam. NOT connected to Helper/default dispatch.
Capsules are routing data, never credential authority; Native rechecks each call.
No password caching, project IDs chosen by guest, target resolver or job grants.
"""
import base64,binascii,threading,sys,os
from .broker import _PrivateSocketBroker
from .errors import require
from .protocol import uuid_field

FIELDS={'version','purpose','scope_id','phase','call_index','environment_digest','helper_digest','phase_nonce','revision_digest','reference_sha256'}
def _digest(value):
    return isinstance(value,str) and len(value)==64 and all(c in '0123456789abcdef' for c in value)
def _capsule(request):
    require(isinstance(request,dict) and set(request)==FIELDS and type(request['version']) is int and request['version']==1 and
            request['purpose']=='ansible_vault_validation_password' and request['phase'] in ('syntax','lint') and
            type(request['call_index']) is int and request['call_index'] in (0,1) and
            all(_digest(request[k]) for k in ('environment_digest','helper_digest','revision_digest','reference_sha256')),
            'vault_phase','Invalid fixed Vault child capsule')
    for field in ('scope_id','phase_nonce'):uuid_field(request[field],field)
    return dict(request)

def checked_password_response(result):
    require(isinstance(result,dict) and set(result)=={'password_base64'} and isinstance(result['password_base64'],str) and
            0<len(result['password_base64'])<=5464,'vault_password','Scoped Vault input unavailable')
    raw=bytearray()
    try:
        raw.extend(base64.b64decode(result['password_base64'],validate=True))
        require(2<=len(raw)<=4097 and raw[-1:]==b'\n','vault_password','Scoped Vault input unavailable')
        value=raw[:-1].decode('utf-8')
        require(value and value.strip()==value and all(not(c<' ' or c=='\x7f' or c in '\u0085\u2028\u2029' or '\u0080'<=c<='\u009f') for c in value),
                'vault_password','Scoped Vault input unavailable')
        return result
    except (ValueError,UnicodeError,binascii.Error):
        require(False,'vault_password','Scoped Vault input unavailable')
    finally:raw[:]=b'\x00'*len(raw)

class VaultValidationBroker(_PrivateSocketBroker):
    """Exactly one phase/two Native child capsules; third denied before channel.
    Channel is a dedicated trusted Native control interface, NOT a guest callback.
    Closing this socket revokes only that dedicated validation scope.
    """
    def __init__(self,directory,capsules,channel,cancel,*,keep_owned_directory=False):
        require(sys.platform=='linux' and os.getuid()!=0,'vault_platform','Linux nonroot Vault broker required')
        require(isinstance(capsules,(tuple,list)) and len(capsules)==2,'vault_phase','Two distinct Vault child capsules required')
        first,second=map(_capsule,capsules)
        require(first['call_index']==0 and second['call_index']==1 and first['phase_nonce']!=second['phase_nonce'] and
                all(first[k]==second[k] for k in FIELDS-{'call_index','phase_nonce'}),
                'vault_phase','Vault child capsules do not match one phase')
        self._capsules=(first,second);self._next=0;self._take=threading.Lock()
        def handle(request):
            require(isinstance(request,dict) and set(request)=={'version','operation'} and type(request['version']) is int and
                    request['version']==1 and request['operation']=='ansible_vault_password',
                    'vault_request','Unsupported private Vault client request')
            with self._take:
                require(not cancel.is_set() and self._next<2,'vault_unavailable','Scoped Vault input unavailable')
                capsule=self._capsules[self._next];self._next+=1
            # Consume BEFORE any Native wait; failed/partial lookup never reuses
            # this child. Native does its own independent atomic/context gates.
            return checked_password_response(channel.call(capsule,cancel=cancel,timeout=5))
        super().__init__(directory,handle,channel.revoke,cancel,slots_limit=1,error_message='Scoped Vault input unavailable',frame_limit=1024,handoff_seconds=5,keep_owned_directory=keep_owned_directory)
