"""Fixed root-role publication; no caller path, command, rollback or adoption.

The role supplies this exact reviewed source to isolated Python -c. It does not
execute a preexisting remote helper or turn package bytes into sudo authority.
"""
import hashlib
import ipaddress
import json
import os
from pathlib import Path
import pwd
import grp
import stat
import sys

_UNIT_PARENT = Path('/etc/systemd/system')
_OWNER_PARENT = Path('/etc/consolecrypt-agent')
_UNIT_NAME = 'consolecrypt-agent-outbound-control.service'
_OWNER_NAME = 'outbound-control-owner.json'
_CAP = 8192
_UNIT = b'''[Unit]
Description=ConsoleCrypt finite outbound pinned SSH carrier
After=network.target ssh.service
Requires=ssh.service
ConditionPathExists=/etc/consolecrypt-agent/tunnel-transport.json
ConditionPathExists=/etc/consolecrypt-agent/tunnel-relay-ca.pem
ConditionPathExists=/var/lib/consolecrypt-agent/tunnel/control/bootstrap.json

[Service]
Type=simple
User=consolecrypt-agent
Group=consolecrypt-agent
WorkingDirectory=/opt/consolecrypt-agent-install/opt/consolecrypt-agent
ExecStart=/usr/bin/python3.12 -I -B /opt/consolecrypt-agent-install/opt/consolecrypt-agent/launch.py outbound-control
Environment=LANG=C.UTF-8
Restart=no
TimeoutStopSec=10s
UMask=0077
NoNewPrivileges=yes
CapabilityBoundingSet=
AmbientCapabilities=
PrivateTmp=yes
PrivateDevices=yes
ProtectSystem=strict
ProtectHome=yes
ProtectKernelTunables=yes
ProtectKernelModules=yes
ProtectControlGroups=yes
RestrictSUIDSGID=yes
LockPersonality=yes
RestrictRealtime=yes
RestrictAddressFamilies=AF_UNIX AF_INET AF_INET6
IPAddressDeny=any
IPAddressAllow=localhost @RELAY_IP@
ReadOnlyPaths=/var/lib/consolecrypt-agent/tunnel/control
InaccessiblePaths=-/run/docker.sock -/var/run/docker.sock -/run/containerd/containerd.sock
MemoryMax=128M
MemorySwapMax=0
CPUQuota=25%
TasksMax=16
LimitNOFILE=64

[Install]
WantedBy=multi-user.target
'''


class Unavailable(OSError):
    def __init__(self): super().__init__('publication_unavailable')


def _need(value):
    if not value: raise Unavailable()


def _identity(m):
    return (m.st_dev,m.st_ino,m.st_mode,m.st_uid,m.st_gid,m.st_nlink,
            m.st_size,m.st_mtime_ns,m.st_ctime_ns)


def _directory(m): return (m.st_dev,m.st_ino,m.st_mode,m.st_uid,m.st_gid)


def _json(raw):
    def pairs(items):
        result={}
        for key,value in items:
            _need(key not in result);result[key]=value
        return result
    def number(_): raise Unavailable()
    value=json.loads(raw,object_pairs_hook=pairs,parse_float=number,parse_constant=number)
    _need(raw==_encode(value));return value


def _encode(value): return (json.dumps(value,sort_keys=True,separators=(',',':'),ensure_ascii=True)+'\n').encode('ascii')


def _parent(path):
    captured=[]
    for p in (path,*path.parents):
        m=p.lstat();_need(stat.S_ISDIR(m.st_mode) and not stat.S_ISLNK(m.st_mode)
            and m.st_uid==0 and not m.st_mode&0o022)
        captured.append((p,_directory(m)))
    fd=os.open(path,os.O_RDONLY|os.O_DIRECTORY|os.O_CLOEXEC|os.O_NOFOLLOW|os.O_NONBLOCK)
    try:_parents_current(fd,captured)
    except BaseException:os.close(fd);raise
    return fd,captured


def _parents_current(fd,captured):
    _need(_directory(os.fstat(fd))==captured[0][1])
    for path,expected in captured:_need(_directory(path.lstat())==expected)


def _regular(m):
    _need(stat.S_ISREG(m.st_mode) and m.st_uid==m.st_gid==0 and m.st_nlink==1
        and stat.S_IMODE(m.st_mode)==0o644 and 0<=m.st_size<=_CAP)


def _read(fd,parent,name):
    before=os.fstat(fd);_regular(before)
    _need(_identity(os.stat(name,dir_fd=parent,follow_symlinks=False))==_identity(before))
    raw=os.pread(fd,_CAP+1,0)
    _need(len(raw)==before.st_size and _identity(os.fstat(fd))==_identity(before)
        and _identity(os.stat(name,dir_fd=parent,follow_symlinks=False))==_identity(before))
    return raw,before


def _existing(parent,name):
    return os.open(name,os.O_RDONLY|os.O_CLOEXEC|os.O_NOFOLLOW|os.O_NONBLOCK,dir_fd=parent)


def _exists(parent,name):
    try:os.stat(name,dir_fd=parent,follow_symlinks=False);return True
    except FileNotFoundError:return False


def _create_pair(unit_parent,owner_parent,effect,owned):
    # Mark uncertain BEFORE the first mutating syscall. An error is not proof
    # of no effect. Never cleanup/unlink partial files or adopt an existing one.
    effect['attempted']=True
    flags=os.O_RDWR|os.O_CREAT|os.O_EXCL|os.O_NOFOLLOW|os.O_NONBLOCK|os.O_CLOEXEC
    unit=os.open(_UNIT_NAME,flags,0o644,dir_fd=unit_parent);owned.append(unit)
    os.fchmod(unit,0o644) # Only our O_EXCL-created FD, never an existing path.
    unit_identity=_identity(os.fstat(unit))
    owner=os.open(_OWNER_NAME,flags,0o644,dir_fd=owner_parent);owned.append(owner)
    os.fchmod(owner,0o644)
    return unit,owner,unit_identity


def _write(fd,raw):
    _need(0<len(raw)<=_CAP)
    remaining=memoryview(raw)
    while remaining:
        count=os.write(fd,remaining);_need(0<count<=len(remaining));remaining=remaining[count:]
    os.fsync(fd)


def _facts(uid,gid,ip,unit,meta):
    return dict(format='cc-agent-outbound-control-install/v2',uid=uid,gid=gid,relayDialIp=ip,
        unitSha256=hashlib.sha256(unit).hexdigest(),unitDevice=meta.st_dev,unitInode=meta.st_ino)


def publish(uid,gid,ip,effect):
    _need(os.getuid()==os.geteuid()==0 and os.getgid()==os.getegid()==0)
    user=pwd.getpwnam('consolecrypt-agent');group=grp.getgrnam('consolecrypt-agent')
    _need(uid>0 and gid>0 and user.pw_uid==uid and user.pw_gid==group.gr_gid==gid
        and user.pw_dir=='/var/lib/consolecrypt-agent' and user.pw_shell in {'/usr/sbin/nologin','/sbin/nologin'})
    address=ipaddress.IPv4Address(ip)
    _need(str(address)==ip and not address.is_unspecified and not address.is_multicast and ip!='255.255.255.255')
    unit=_UNIT.replace(b'@RELAY_IP@',ip.encode('ascii'))
    parents=[];owned=[];facts=None;created=False
    try:
        for path in (_UNIT_PARENT,_OWNER_PARENT):parents.append(_parent(path))
        up,op=parents[0][0],parents[1][0]
        has_unit,has_owner=_exists(up,_UNIT_NAME),_exists(op,_OWNER_NAME)
        _need(has_unit==has_owner)
        if has_unit:
            # An original receipt AND original exclusively-created unit inode
            # are required. Byte equality alone never adopts a foreign asset.
            uf=_existing(up,_UNIT_NAME);owned.append(uf)
            of=_existing(op,_OWNER_NAME);owned.append(of)
            actual,meta=_read(uf,up,_UNIT_NAME);receipt,_=_read(of,op,_OWNER_NAME)
            facts=_facts(uid,gid,ip,unit,meta)
            _need(actual==unit and _json(receipt)==facts)
        else:
            uf,of,initial=_create_pair(up,op,effect,owned)
            um,om=os.fstat(uf),os.fstat(of);_regular(um);_regular(om)
            _need(_identity(um)==initial and um.st_size==om.st_size==0)
            facts=_facts(uid,gid,ip,unit,um)
            _write(uf,unit);_write(of,_encode(facts))
            os.fsync(up);os.fsync(op)
            actual,meta=_read(uf,up,_UNIT_NAME);receipt,_=_read(of,op,_OWNER_NAME)
            _need(actual==unit and (meta.st_dev,meta.st_ino)==(um.st_dev,um.st_ino)
                and _json(receipt)==facts)
            created=True
        for fd,captured in parents:_parents_current(fd,captured)
    finally:
        failed=False
        for fd in reversed(owned+[p[0] for p in parents]):
            try:os.close(fd)
            except OSError:failed=True
        _need(not failed)
    return dict(format='cc-agent-outbound-control-publication-result/v1',uid=uid,gid=gid,
        unitSha256=facts['unitSha256'],publication='created' if created else 'alreadyOwned')


def main():
    effect={'attempted':False}
    try:
        _need(sys.argv[0]=='-c' and len(sys.argv)==4)
        _need(all(value.isascii() and value.isdecimal() and str(int(value))==value for value in sys.argv[1:3]))
        result=publish(int(sys.argv[1]),int(sys.argv[2]),sys.argv[3],effect)
    except Exception:
        print(_encode(dict(format='cc-agent-outbound-control-publication-result/v1',
            publication='unknown' if effect['attempted'] else 'refused')).decode('ascii'),end='',flush=True)
        return 1
    print(_encode(result).decode('ascii'),end='',flush=True);return 0


if __name__=='__main__':raise SystemExit(main())
