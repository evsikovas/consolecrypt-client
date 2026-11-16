"""Fixed tunnel-only coarse audit; a local bounded log, never an authority."""
import os
from pathlib import Path
import stat
import time
import uuid

from .audit import AuditLog
from .history import finite_number


class TunnelAuditLog(AuditLog):
    """Separate private directory; legacy audit schema/behavior is unchanged."""
    def __init__(self,directory,policy):
        directory=Path(directory)
        if not directory.is_absolute() or '..' in directory.parts:
            raise ValueError('tunnel_audit_unavailable')
        for path in [directory,*directory.parents]:
            if path.is_symlink():raise ValueError('tunnel_audit_unavailable')
        info=directory.lstat()
        if (not stat.S_ISDIR(info.st_mode) or info.st_uid!=os.getuid()
                or stat.S_IMODE(info.st_mode)!=0o700):raise ValueError('tunnel_audit_unavailable')
        self._policy=policy
        self._directory=directory
        self._directory_identity=(info.st_dev,info.st_ino,info.st_uid,info.st_mode)
        super().__init__(directory)
        if self._blocked:raise ValueError('tunnel_audit_unavailable')
    @staticmethod
    def _valid(event):
        try:
            return (type(event) is dict and set(event)=={'timestamp','agent_id','operation','resource','reason'}
                and finite_number(event['timestamp']) and event['timestamp']>=0
                and str(uuid.UUID(event['agent_id']))==event['agent_id']
                and str(uuid.UUID(event['resource']))==event['resource']
                and event['operation']=='tunnel.open'
                and event['reason'] in {'admitted','closed','scope_denied','operation_unavailable'})
        except (ValueError,TypeError,AttributeError):return False
    def record(self,reason):
        before=self._directory.lstat()
        if (before.st_dev,before.st_ino,before.st_uid,before.st_mode)!=self._directory_identity:
            return False
        self._policy.check()
        policy=self._policy.value()
        result=self({'timestamp':time.time(),'agent_id':policy['agentId'],'operation':'tunnel.open',
            'resource':policy['routeId'],'reason':reason})
        after=self._directory.lstat()
        return result is True and (after.st_dev,after.st_ino,after.st_uid,after.st_mode)==self._directory_identity
