#!/usr/bin/python3
"""Fixed installed monitoring CONFIG backup and explicit recovery operations.

The manifest is integrity metadata, never approval. Explicit reviewed config,
packaged rules and rendered unit are independently compared before restore.
Without a mode flag this remains a read-only preflight. Explicit create/restore
write only the fixed configuration mapping, never service state or metric data.
"""
import hashlib,json,os,secrets,stat,subprocess,sys,tempfile
from pathlib import Path
MAX_FILE=65536
NAMES=('prometheus.yml','rules.yml','prometheus.service')
TOOL=Path('/opt/consolecrypt-monitoring/prometheus-3.16.0.linux-amd64/promtool')
BACKUP=Path('/srv/consolecrypt-monitoring/config-backup')
LIVE={
    'prometheus.yml':Path('/etc/consolecrypt-monitoring/prometheus.yml'),
    'rules.yml':Path('/etc/consolecrypt-monitoring/rules.yml'),
    'prometheus.service':Path('/etc/systemd/system/consolecrypt-prometheus.service'),
}

class Refused(ValueError):
    def __init__(self,code=65):
        self.code=code
        super().__init__('monitoring configuration refused')

def need(value,code=65):
    if not value:raise Refused(code)

def pairs(values):
    result={}
    for key,value in values:need(key not in result);result[key]=value
    return result

def parse(raw):
    return json.loads(raw,object_pairs_hook=pairs,parse_constant=lambda _:need(False))

def read(path,limit=MAX_FILE,uid=0,*,private=True):
    # Production requires Linux nofollow/nonblock and root-owned ancestry.
    # Tests exercise the same held-file mapping on a disposable local tree.
    path=Path(path);need(path.is_absolute())
    if uid is not None:
        current=path.parent
        while True:
            info=current.lstat();need(stat.S_ISDIR(info.st_mode)and info.st_uid==uid and not info.st_mode&0o022,66)
            if current==current.parent:break
            current=current.parent
    need(not path.is_symlink())
    fd=os.open(path,os.O_RDONLY|getattr(os,'O_NOFOLLOW',0)|getattr(os,'O_NONBLOCK',0)|getattr(os,'O_CLOEXEC',0)|getattr(os,'O_BINARY',0))
    try:
        before=os.fstat(fd);need(stat.S_ISREG(before.st_mode)and before.st_nlink==1 and before.st_size<=limit,66)
        if uid is not None:need(before.st_uid==uid and not before.st_mode&(0o077 if private else 0o022),66)
        raw=os.read(fd,limit+1);after=os.fstat(fd);named=path.lstat()
        need(len(raw)==before.st_size and (before.st_dev,before.st_ino,before.st_size,before.st_mtime_ns,before.st_ctime_ns)==(after.st_dev,after.st_ino,after.st_size,after.st_mtime_ns,after.st_ctime_ns))
        need((named.st_dev,named.st_ino)==(before.st_dev,before.st_ino))
        return raw
    finally:os.close(fd)

def digest(raw):return hashlib.sha256(raw).hexdigest()

def matches(files,value):
    value=expected(value)
    need(parse(files['prometheus.yml'])==value['config'],67)
    need(digest(files['rules.yml'])==value['rulesSha256']and digest(files['prometheus.service'])==value['unitSha256'],67)

def expected(value):
    need(type(value)is dict and set(value)=={'version','config','rulesSha256','unitSha256'}and type(value['version'])is int and value['version']==1)
    need(type(value['config'])is dict and value['config'].get('rule_files')==['/etc/consolecrypt-monitoring/rules.yml'])
    for field in('rulesSha256','unitSha256'):
        item=value[field];need(type(item)is str and len(item)==64 and all(c in'0123456789abcdef'for c in item))
    return value

def validate(files,tool,uid=None):
    # Check both backed-up rules and the backed-up config against THESE rules,
    # not a possibly missing/corrupt live rules file. No endpoint is contacted.
    need(Path(tool).is_absolute()and Path(tool).is_file()and not Path(tool).is_symlink(),68)
    if uid is not None:
        current=Path(tool)
        while True:
            info=current.lstat();need(info.st_uid==uid and not info.st_mode&0o022,68)
            need(stat.S_ISREG(info.st_mode)and info.st_nlink==1 if current==Path(tool)else stat.S_ISDIR(info.st_mode),68)
            if current==current.parent:break
            current=current.parent
    with tempfile.TemporaryDirectory(prefix='cc-monitoring-restore-check-')as directory:
        root=Path(directory);rules=root/'rules.yml';rules.write_bytes(files['rules.yml'])
        config=parse(files['prometheus.yml']);config['rule_files']=[str(rules)]
        path=root/'prometheus.yml';path.write_text(json.dumps(config),encoding='utf-8')
        for args in (['check','rules',str(rules)],['check','config',str(path)]):
            run=subprocess.run([str(tool),*args],stdin=subprocess.DEVNULL,capture_output=True,timeout=20)
            need(run.returncode==0 and len(run.stdout)+len(run.stderr)<=262144,69)

def verify(directory,value,*,uid=0,tool=TOOL):
    value=expected(value);directory=Path(directory)
    manifest=parse(read(directory/'manifest.json',4096,uid))
    need(type(manifest)is dict and set(manifest)=={'version','files'}and type(manifest['version'])is int and manifest['version']==1)
    need(type(manifest['files'])is dict and set(manifest['files'])==set(NAMES))
    files={name:read(directory/name,MAX_FILE,uid)for name in NAMES}
    need(all(type(manifest['files'][name])is str and digest(files[name])==manifest['files'][name]for name in NAMES))
    matches(files,value)
    # Snapshot metadata and ALL intended bytes checked before tool invocation.
    validate(files,tool,uid)
    need(files=={name:read(directory/name,MAX_FILE,uid)for name in NAMES})
    need(manifest==parse(read(directory/'manifest.json',4096,uid)))
    return{'version':1,'backupVerified':True,'reviewedIntentMatches':True,'promtoolValidated':True,'metricsReadOrChanged':False,'servicesChanged':False,
           'files':{name:digest(raw)for name,raw in files.items()}}

def identity(info):
    return(info.st_dev,info.st_ino,info.st_size,info.st_mtime_ns,info.st_ctime_ns)

def leaf(directory,name,uid):
    try:info=os.stat(name,dir_fd=directory,follow_symlinks=False)
    except FileNotFoundError:return None
    need(stat.S_ISREG(info.st_mode)and info.st_nlink==1 and info.st_size<=MAX_FILE,70)
    if uid is not None:need(info.st_uid==uid and not info.st_mode&0o022,70)
    return identity(info)

def write_mapping(mapping,*,uid):
    # Production callers supply only LIVE or BACKUP constants. Hold every parent
    # before mutation and preflight ALL destinations. Publish each complete leaf
    # atomically; a crash across leaves is detected by the manifest, not hidden.
    handles={};stamps={};before={}
    try:
        for path in mapping:
            parent=path.parent
            if parent not in handles:
                current=parent
                while True:
                    info=current.lstat();need(stat.S_ISDIR(info.st_mode),70)
                    if uid is not None:need(info.st_uid==uid and not info.st_mode&0o022,70)
                    if current==current.parent:break
                    current=current.parent
                fd=os.open(parent,os.O_RDONLY|os.O_DIRECTORY|os.O_NOFOLLOW|os.O_CLOEXEC)
                handles[parent]=fd;stamps[parent]=os.fstat(fd)
                named=parent.lstat();need((named.st_dev,named.st_ino)==(stamps[parent].st_dev,stamps[parent].st_ino))
            before[path]=leaf(handles[parent],path.name,uid)
        for path,(raw,mode)in mapping.items():
            need(type(raw)is bytes and len(raw)<=MAX_FILE)
            fd=handles[path.parent];named=path.parent.lstat();held=stamps[path.parent]
            need((named.st_dev,named.st_ino)==(held.st_dev,held.st_ino))
            need(leaf(fd,path.name,uid)==before[path],71)
            temporary='.cc-config-'+secrets.token_hex(16);created=False
            try:
                output=os.open(temporary,os.O_WRONLY|os.O_CREAT|os.O_EXCL|os.O_NOFOLLOW|os.O_CLOEXEC,0o600,dir_fd=fd);created=True
                try:
                    with os.fdopen(output,'wb',closefd=False)as stream:
                        stream.write(raw);stream.flush();os.fchmod(output,mode);os.fsync(output)
                    need(leaf(fd,path.name,uid)==before[path],71)
                    os.replace(temporary,path.name,src_dir_fd=fd,dst_dir_fd=fd);created=False
                    os.fsync(fd)
                finally:os.close(output)
            finally:
                if created:os.unlink(temporary,dir_fd=fd)
    finally:
        for fd in handles.values():os.close(fd)

def create_backup(directory,value,*,live=LIVE,uid=0,tool=TOOL):
    need(set(live)==set(NAMES));directory=Path(directory)
    files={name:read(live[name],MAX_FILE,uid,private=False)for name in NAMES}
    matches(files,value);validate(files,tool,uid)
    need(files=={name:read(live[name],MAX_FILE,uid,private=False)for name in NAMES})
    manifest={'version':1,'files':{name:digest(raw)for name,raw in files.items()}}
    mapping={directory/name:(files[name],0o600)for name in NAMES}
    mapping[directory/'manifest.json']=(json.dumps(manifest,sort_keys=True).encode(),0o600)
    write_mapping(mapping,uid=uid)
    result=verify(directory,value,uid=uid,tool=tool);result['backupCreated']=True
    return result

def restore_backup(directory,value,*,live=LIVE,uid=0,tool=TOOL):
    need(set(live)==set(NAMES));directory=Path(directory)
    result=verify(directory,value,uid=uid,tool=tool)
    files={name:read(directory/name,MAX_FILE,uid)for name in NAMES}
    need(all(digest(raw)==result['files'][name]for name,raw in files.items()))
    write_mapping({live[name]:(files[name],0o644)for name in NAMES},uid=uid)
    need(files=={name:read(live[name],MAX_FILE,uid,private=False)for name in NAMES})
    result['configurationRestored']=True
    return result

def main():
    need(sys.argv[1:]in([],['--create-reviewed-backup'],['--restore-reviewed-backup'])and sys.platform=='linux'and os.getuid()==os.geteuid()==0)
    raw=sys.stdin.buffer.read(131073);need(len(raw)<=131072)
    operation=verify if len(sys.argv)==1 else create_backup if sys.argv[1]=='--create-reviewed-backup'else restore_backup
    print(json.dumps(operation(BACKUP,parse(raw)),sort_keys=True))
if __name__=='__main__':
    try:main()
    except Exception as error:
        # Closed numeric categories only. Never print exception text, paths,
        # config/credentials or tool output, including through Ansible logs.
        code=error.code if isinstance(error,Refused)and error.code in range(65,72)else 72 if isinstance(error,subprocess.TimeoutExpired)else 73 if isinstance(error,OSError)else 65
        sys.stderr.write('monitoring configuration refused; code='+str(code)+'\n');sys.exit(code)
