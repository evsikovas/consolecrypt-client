"""ONE fixed owned systemd guest observation, real Node+signed runtime reads.

Called by the existing native-controller Phases, never on a user host. No fake
executor, success marker, imported signing key or unsigned install fallback.
"""
from pathlib import Path
import base64,hashlib,http.client,importlib.util,json,os,re,stat,subprocess,sys,time,uuid
HOME=Path('/var/lib/consolecrypt-agent');CONFIG=Path('/etc/consolecrypt-agent')
FIXTURE=Path('/opt/cc-agent-install-fixture');BINARY=Path('/usr/local/libexec/consolecrypt-agent-release/cc-agent-identity')
OWNER=FIXTURE/'private/synthetic_node_authority';OWNER_SHA=json.loads((FIXTURE/'expected-releases.json').read_bytes())['fixtureAuthoritySha256']
CODE=Path('/opt/consolecrypt-agent-install/opt/consolecrypt-agent')
RUNTIME_FILES=('runtime/cc_agent/__init__.py','runtime/cc_agent/service.py','integration/__main__.py','launch.py')
AUTH=HOME/'checkpoints/authority';KEY=HOME/'identity/node-key'
OBS=Path('/cc-persist/software/agent-observation')
sys.path.insert(0,str(FIXTURE));from sqlite_fixture_flow import owner_via_fd,node_credentials,ready_without_proof,OwnedPidObserver,assert_no_owner_fd,owned_workers

def expected_releases():
    path=FIXTURE/'expected-releases.json';info=path.lstat()
    assert stat.S_ISREG(info.st_mode)and info.st_uid==0 and info.st_nlink==1 and stat.S_IMODE(info.st_mode)==0o444 and info.st_size<=16384
    value=json.loads(path.read_bytes());assert value['version']==1 and set(value['phases'])=={'install','update','rollback','disk'}
    return value


def binary_sha():return expected_releases()['normalElfSha256']


def selected_runtime(stage):
    expected=expected_releases()['phases'][stage]
    current=json.loads((CODE/'current.json').read_bytes())
    assert current['version']==expected['version']and current['artifact_sha256']==expected['artifactSha256']
    release=CODE/'releases'/expected['version'];record=json.loads((release/'installed.json').read_bytes())
    assert record['version']==expected['version']and record['artifact_sha256']==expected['artifactSha256']
    hashes={}
    for name in RUNTIME_FILES:
        path=release/name;info=path.lstat()
        assert stat.S_ISREG(info.st_mode)and info.st_uid==0 and info.st_nlink==1 and stat.S_IMODE(info.st_mode)==0o644 and info.st_size<=128*1024
        raw=path.read_bytes();pin=hashlib.sha256(raw).hexdigest()
        assert pin==record['files'][name]==expected['runtimeFiles'][name]
        hashes[name]=pin
    assert hashlib.sha256((CODE/'launch.py').read_bytes()).hexdigest()==hashes['launch.py']
    return {'version':expected['version'],'artifactSha256':expected['artifactSha256'],'runtimeFiles':hashes}


def fixed_python_argv(raw):
    expected=['/usr/bin/python3','-I',str(CODE/'launch.py'),'--manifest',str(CONFIG/'manifest.json'),
              '--identity-config',str(CONFIG/'identity-config.json'),'--state-dir',str(HOME/'history')]
    assert len(raw)<=4096 and raw==b'\0'.join(value.encode()for value in expected)+b'\0'


def bound_listener(raw,owned_inodes,port=18991):
    assert len(raw)<=65536
    found=[]
    for line in raw.decode('ascii').splitlines()[1:]:
        fields=line.split();assert len(fields)>=10
        if fields[1]=='0100007F:%04X'%port and fields[3]=='0A':found.append(fields[9])
    assert len(found)==1 and found[0]in owned_inodes,'heartbeat listener is not owned by actual MainPID'


def running_python_binding(pid,stage):
    selected=selected_runtime(stage);proc=Path('/proc')/str(pid)
    fixed_python_argv((proc/'cmdline').read_bytes())
    status=(proc/'status').read_bytes();assert len(status)<=16384
    credentials={line.split(':',1)[0]:line.split(':',1)[1].split()for line in status.decode().splitlines()if line.startswith(('Uid:','Gid:'))}
    assert credentials=={'Uid':['10002']*4,'Gid':['10002']*4}
    assert(proc/'cwd').resolve()==CODE
    mapped=(proc/'exe').stat();interpreter=Path('/usr/bin/python3').stat()
    assert(mapped.st_dev,mapped.st_ino)==(interpreter.st_dev,interpreter.st_ino)
    with(proc/'exe').open('rb')as stream:python_sha=hashlib.file_digest(stream,'sha256').hexdigest()
    assert python_sha==hashlib.sha256(Path('/usr/bin/python3').read_bytes()).hexdigest()
    sockets=set();fds=list((proc/'fd').iterdir());assert len(fds)<=128
    for fd in fds:
        try:value=os.readlink(fd)
        except FileNotFoundError:continue
        match=re.fullmatch(r'socket:\[([0-9]+)\]',value)
        if match:sockets.add(match.group(1))
    bound_listener((proc/'net/tcp').read_bytes(),sockets)
    return dict(selected,pythonExecutableSha256=python_sha,listenerOwnedByMainPid=True)


def run(argv,raw=None,limit=20,node=False):
    value=subprocess.run([str(v)for v in argv],input=raw,preexec_fn=node_credentials if node else None,capture_output=True,close_fds=True,timeout=limit)
    assert value.returncode==0 and len(value.stdout)+len(value.stderr)<=65536,'actual fixed fixture operation failed'
    return json.loads(value.stdout)if value.stdout else None


def provision_collector_once():
    # ONLY this fixed provision main is called, exactly once. Its CLI catch-all
    # otherwise discards the originating static error and stack location.
    script=r'''import ast,hashlib,importlib.util,json,os,pathlib,re,select,sys,threading,time
path=pathlib.Path('/usr/local/libexec/consolecrypt-agent-install/collector_provision.py')
raw=path.read_bytes()
allowed={n.value for n in ast.walk(ast.parse(raw))if isinstance(n,ast.Constant)and type(n.value)is str and re.fullmatch(r'collector_[a-z_]{1,80}',n.value)}
spec=importlib.util.spec_from_file_location('owned_collector_provision',path)
module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module)
original_command=module.command
namespace_observed={}
journal_state={'stage':'notStarted','expected':(),'snapshot':{},'raw':b'','persisted':False,'queryCount':0}

def save_private(name,retained):
    assert name in('namespace-probe.output','namespace-probe.journal')and len(retained)<=8192
    parent=pathlib.Path('/cc-persist/software/agent-observation');info=parent.lstat()
    assert info.st_uid==0 and info.st_mode&0o777==0o700
    fd=os.open(parent/name,os.O_CREAT|os.O_EXCL|os.O_WRONLY|os.O_NOFOLLOW|os.O_CLOEXEC,0o600)
    try:
        with os.fdopen(os.dup(fd),'wb')as stream:stream.write(retained)
        assert os.fstat(fd).st_size==len(retained)
    finally:os.close(fd)

def namespace_command(argv,**kwargs):
    fixed=(argv[0]=='/usr/bin/systemd-run'and '--unit=cc-agent-collector-prerequisite.service'in argv
           and argv[-6:]==['/usr/bin/python3.12','-I','-B',str(module.PROBE),'10002','10002']
           and kwargs.get('uid')==kwargs.get('gid')==10002)
    journal=(bool(journal_state['expected'])and tuple(argv)==journal_state['expected']
             and argv[0]=='/usr/bin/journalctl'and kwargs.get('uid')==kwargs.get('gid')==0)
    if not fixed and not journal:return original_command(argv,**kwargs)
    original_popen=module.subprocess.Popen;state=[];failed=False;stdout_raw=b'';stdout_total=0;stdout_unknown=False
    def captured_popen(*args,**options):
        assert args[0]is argv and options['stderr']==module.subprocess.DEVNULL and not state
        options['stderr']=module.subprocess.PIPE
        proc=original_popen(*args,**options);raw=bytearray();total=[0];stop=threading.Event()
        def drain():
            fd=proc.stderr.fileno();os.set_blocking(fd,False)
            while True:
                ready=select.select([fd],[],[],.05)[0]
                if ready:
                    block=os.read(fd,1024)
                    if not block:break
                    total[0]+=len(block);raw.extend(block[:max(0,8192-len(raw))])
                elif stop.is_set():break
        thread=threading.Thread(target=drain,daemon=True);thread.start()
        state.append((proc,raw,total,stop,thread));return proc
    module.subprocess.Popen=captured_popen
    try:
        result=original_command(argv,**kwargs)
        assert type(result)is bytes and len(result)<=16384
        stdout_total=len(result);stdout_raw=result[:8192]
        return result
    except BaseException as original_error:
        failed=True
        try:
            tb=original_error.__traceback__;matched=False
            while tb is not None:
                frame=tb.tb_frame
                if frame.f_code is original_command.__code__:
                    assert not matched and frame.f_locals['argv']is argv
                    assert state and frame.f_locals['proc']is state[0][0]
                    original_raw=frame.f_locals['raw']
                    assert type(original_raw)is bytearray and len(original_raw)<=16385
                    stdout_total=len(original_raw);stdout_raw=bytes(original_raw[:8192]);matched=True
                tb=tb.tb_next
            assert matched
        except BaseException:stdout_unknown=True
        raise
    finally:
        module.subprocess.Popen=original_popen
        try:
            if state:
                proc,raw,total,stop,thread=state[0];stop.set();thread.join(timeout=1)
                assert not thread.is_alive()and proc.returncode is not None
                proc.stderr.close()
                stderr_raw=bytes(raw[:max(0,8192-len(stdout_raw))]);retained=stdout_raw+stderr_raw
                snapshot={'exit':proc.returncode,'stdoutBytes':len(stdout_raw),'stdoutObservedBytes':stdout_total,'stdoutCaptureUnknown':stdout_unknown,'stderrBytes':len(stderr_raw),'stderrObservedBytes':total[0],'retainedBytes':len(retained),'streamOrder':['stdout','stderr'],'truncated':stdout_total+total[0]>len(retained),'sha256':hashlib.sha256(retained).hexdigest(),'sameOriginalCommand':True,'secondProbe':False}
                if journal:
                    journal_state['snapshot']=snapshot;journal_state['raw']=retained
                elif failed:
                    namespace_observed['exit']=proc.returncode
                    save_private('namespace-probe.output',retained)
                    code=('probeGuardRejected'if b'collector_namespace_unsupported'in retained else
                          'namespaceSetupFailed'if b'NAMESPACE'in retained or b'namespacing'in retained else
                          'transientStartFailed'if b'Failed to start'in retained else 'unclassified')
                    print('CC_AGENT_NAMESPACE_PROCESS '+json.dumps(dict(snapshot,code=code)),file=sys.stderr,flush=True)
        except BaseException:
            if journal:journal_state['captureUnknown']=True
            else:print('CC_AGENT_NAMESPACE_PROCESS {"code":"captureUnknown","sameOriginalCommand":true,"secondProbe":false}',file=sys.stderr,flush=True)

def persist_journal_query():
    if not journal_state['persisted']:
        journal_state['stage']='privateWrite'
        save_private('namespace-probe.journal',journal_state['raw'])
        journal_state['persisted']=True

def capture_namespace_journal():
    if not namespace_observed:return
    unit='cc-agent-collector-prerequisite.service'
    unit_fields=('_SYSTEMD_USER_UNIT','OBJECT_SYSTEMD_USER_UNIT','USER_UNIT')
    fields=['_UID',*unit_fields,'_BOOT_ID','MESSAGE','ERRNO','CODE_FUNC','CODE_FILE','CODE_LINE','MESSAGE_ID','__MONOTONIC_TIMESTAMP']
    argv=['/usr/bin/journalctl','--no-pager','--quiet','--boot=0','--lines=16','--output=json','--output-fields='+','.join(fields)]
    for index,field in enumerate(unit_fields):
        if index:argv.append('+')
        argv.extend(['_UID=10002',field+'='+unit])
    journal_state['expected']=tuple(argv)
    until=min(module.END,time.monotonic()+3)
    boot=pathlib.Path('/proc/sys/kernel/random/boot_id').read_text().strip().replace('-','')
    raw=b''
    while time.monotonic()<until:
        remaining=until-time.monotonic()
        if remaining<=0:break
        journal_state['stage']='query';journal_state['queryCount']+=1
        try:raw=namespace_command(argv,uid=0,gid=0,seconds=remaining,cap=16384)
        except BaseException:
            persist_journal_query();journal_state['stage']='queryExit';raise
        if raw:break
        remaining=until-time.monotonic()
        if remaining>0:time.sleep(min(.05,remaining))
    # Retain this exact filtered command output BEFORE any JSON/scope guard.
    persist_journal_query()
    selected=[];errnos=[]
    journal_state['stage']='jsonDecode'
    for line in raw.splitlines(keepends=True):
        value=json.loads(line)
        journal_state['stage']='uidGuard';assert value.get('_UID')=='10002'
        journal_state['stage']='bootGuard';assert value.get('_BOOT_ID')==boot
        journal_state['stage']='unitGuard';assert any(value.get(field)==unit for field in unit_fields)
        journal_state['stage']='fieldsGuard'
        sequence_fields={'__SEQNUM','__SEQNUM_ID'};present=set(value)&sequence_fields
        assert present in(set(),sequence_fields)
        assert set(value)<=(set(fields)|{'__CURSOR','__REALTIME_TIMESTAMP'}|sequence_fields)
        if present:
            assert type(value['__SEQNUM'])is str and re.fullmatch('[0-9]{1,20}',value['__SEQNUM'])and int(value['__SEQNUM'])<2**64
            assert type(value['__SEQNUM_ID'])is str and re.fullmatch('[0-9a-f]{32}',value['__SEQNUM_ID'])
        selected.append(line)
        errno=value.get('ERRNO')
        if type(errno)is str and errno.isdecimal()and 0<int(errno)<4096:errnos.append(int(errno))
    journal_state['stage']='complete'
    print('CC_AGENT_NAMESPACE_JOURNAL '+json.dumps({'code':'recordsCaptured'if selected else 'noMatchingRecordsUnknown','validated':bool(selected),'uid':10002,'unit':unit,'records':len(selected),'queryCount':journal_state['queryCount'],'query':journal_state['snapshot'],'privateRetained':journal_state['persisted'],'errnoValues':sorted(set(errnos))if selected else [],'originalProcessExit':namespace_observed['exit'],'readOnly':True}),file=sys.stderr,flush=True)
module.command=namespace_command
sys.argv=[str(path),'--provision']
try:
    module.main()
except BaseException as error:
    try:capture_namespace_journal()
    except BaseException as journal_error:
        kind=type(journal_error).__name__
        if kind not in('ValueError','AssertionError','JSONDecodeError','TimeoutExpired','FileNotFoundError','PermissionError','TypeError','NameError','OSError'):kind='Other'
        code=journal_error.args[0]if type(journal_error)is ValueError and len(journal_error.args)==1 and type(journal_error.args[0])is str and journal_error.args[0]in allowed else 'unclassified'
        frames=[];tb=journal_error.__traceback__
        closed={capture_namespace_journal.__code__:'capture_namespace_journal',persist_journal_query.__code__:'persist_journal_query',namespace_command.__code__:'namespace_command',original_command.__code__:'original_command'}
        while tb is not None:
            if tb.tb_frame.f_code in closed:frames.append({'function':closed[tb.tb_frame.f_code],'line':tb.tb_lineno})
            tb=tb.tb_next
        print('CC_AGENT_NAMESPACE_JOURNAL '+json.dumps({'code':'captureRejected','validated':False,'stage':journal_state['stage'],'exception':kind,'errorCode':code,'frames':frames[-8:],'queryCount':journal_state['queryCount'],'query':journal_state['snapshot'],'privateRetained':journal_state['persisted'],'errnoValues':[],'readOnly':True}),file=sys.stderr,flush=True)
    code=error.args[0]if type(error)is ValueError and len(error.args)==1 and type(error.args[0])is str and error.args[0]in allowed else 'unclassified'
    frames=[];tb=error.__traceback__
    while tb is not None:
        filename=pathlib.Path(tb.tb_frame.f_code.co_filename).name
        if filename=='collector_provision.py':frames.append({'module':filename,'function':tb.tb_frame.f_code.co_name,'line':tb.tb_lineno})
        tb=tb.tb_next
    kind=type(error).__name__
    if kind not in('ValueError','FileNotFoundError','PermissionError','TimeoutExpired','CalledProcessError','AssertionError'):kind='Other'
    print('CC_AGENT_COLLECTOR_PROVISION_FAILURE '+json.dumps({'exception':kind,'code':code,'frames':frames[-8:],'originalMainCalls':1}),file=sys.stderr,flush=True)
    print('collector_provision_unavailable',file=sys.stderr,flush=True)
    raise SystemExit(1)
'''
    value=subprocess.run(['/usr/bin/python3','-I','-B','-c',script],capture_output=True,close_fds=True,timeout=65)
    assert len(value.stdout)+len(value.stderr)<=65536,'bounded fixed provision output'
    if value.returncode:
        lines=[line for line in value.stderr.decode('utf-8',errors='replace').splitlines()if line.startswith(('CC_AGENT_COLLECTOR_PROVISION_FAILURE ','CC_AGENT_NAMESPACE_PROCESS ','CC_AGENT_NAMESPACE_JOURNAL '))]
        assert len(lines)<=3
        for line in lines:
            assert len(line)<=4096
            print(line,flush=True)
        print('CC_AGENT_COLLECTOR_PROVISION_EXIT '+json.dumps({'exit':value.returncode,'stdoutBytes':len(value.stdout),'stderrBytes':len(value.stderr),'diagnosticObserved':any(line.startswith('CC_AGENT_COLLECTOR_PROVISION_FAILURE ')for line in lines)}),flush=True)
        raise AssertionError('actual fixed collector provision failed; original exit retained')
    return json.loads(value.stdout)


def node(command,*args,value=None):
    assert hashlib.sha256(BINARY.read_bytes()).hexdigest()==binary_sha()
    return run([BINARY,command,'--state-dir',KEY,'--agent-id',agent(),'--owner-pin',CONFIG/'node-owner-pin.json',*args],json.dumps(value).encode()if value is not None else None,node=True)

def agent():return str(uuid.uuid5(uuid.UUID(token()),'agent-install-acceptance'))
def token():
    values=[v.split('=',1)[1]for v in Path('/proc/cmdline').read_text().split()if v.startswith('cc_svc_token=')]
    assert len(values)==1 and str(uuid.UUID(values[0]))==values[0]
    return values[0]

def owner(command,*args):
    return owner_via_fd(OWNER,OWNER_SHA,[command,'--provider','sqlite','--out',AUTH,'--now',int(time.time()),*args])

def bootstrap():
    # Fixture provisioning is explicit and happens ONCE before the installer.
    # It is not an installer capability or a production owner-key design.
    assert HOME.is_dir()and HOME.stat().st_uid==0 and HOME.stat().st_mode&0o777==0o755 and not list(HOME.iterdir())
    assert CONFIG.is_dir()and CONFIG.stat().st_uid==0 and not list(CONFIG.iterdir())
    for name in('identity','checkpoints','history'):
        path=HOME/name;path.mkdir(mode=0o700);os.chown(path,10002,10002)
    AUTH.mkdir(mode=0o700);os.chown(AUTH,10002,10002)
    hello=run([BINARY,'provision-node','--state-dir',KEY,'--agent-id',agent()],node=True)
    public=AUTH/'node-public.json';public.write_text(json.dumps(hello));public.chmod(0o644)
    owner('offer','--node-public',public,'--profile-id',str(uuid.uuid5(uuid.UUID(token()),'profile')),'--deployment-id',str(uuid.uuid5(uuid.UUID(token()),'deployment')))
    for name in('node-owner-pin.json',):
        (CONFIG/name).write_bytes((AUTH/name).read_bytes());(CONFIG/name).chmod(0o644)
    claim=node('claim-offer',value=json.loads((AUTH/'claim-input.json').read_bytes()))
    assert claim['body']['agentKey']==hello['publicKey']and not(AUTH/'certificate.json').exists()
    claimfile=AUTH/'claim.json';claimfile.write_text(json.dumps(claim));claimfile.chmod(0o644);owner('approve','--claim',claimfile)
    assert json.loads((AUTH/'certificate.json').read_bytes())['body']['agentKey']==hello['publicKey']
    for name in('node-owner-pin.json','owner-pin.json'):(CONFIG/name).write_bytes((AUTH/name).read_bytes());(CONFIG/name).chmod(0o644)
    manifest={'schema_version':1,'agent_id':agent(),'host_id':str(uuid.uuid5(uuid.UUID(token()),'host')),
      'heartbeat_resource_id':str(uuid.uuid5(uuid.UUID(agent()),'heartbeat-v1')),'history_resource_id':str(uuid.uuid5(uuid.UUID(agent()),'history-v1')),
      'port':18991,'sample_interval_seconds':1,'history':{'max_bytes':1048576,'max_points':1000,'max_age_seconds':3600}}
    (CONFIG/'manifest.json').write_text(json.dumps(manifest));(CONFIG/'manifest.json').chmod(0o644)
    (CONFIG/'identity-config.json').write_text(json.dumps({'provider':'sqlite-worker','executable':str(BINARY),'executable_sha256':binary_sha(),
      'store':str(AUTH/'authority-db'),'anchor':str(AUTH/'authority-anchor'),'owner_pin':str(CONFIG/'owner-pin.json')}));(CONFIG/'identity-config.json').chmod(0o644)
    info=(KEY/'node-identity.key').stat()
    value={'agentId':agent(),'keyFingerprint':hello['keyFingerprint'],'keyInode':[info.st_dev,info.st_ino],'privateKeySha256':hashlib.sha256((KEY/'node-identity.key').read_bytes()).hexdigest()}
    # Private fixture comparison only; never print/copy private key digest or key.
    (OBS/'initial-key.json').write_text(json.dumps(value));(OBS/'initial-key.json').chmod(0o600)
    return{'version':1,'agentId':agent(),'keyFingerprint':hello['keyFingerprint'],'nodeKeyGeneratedOnGuest':True,'actualClaimAndOwnerApproval':True,'vaultKeyUsed':False}

def show():
    out=subprocess.run(['/usr/bin/systemctl','--no-pager','show','--property=MainPID,ActiveState,SubState,ControlGroup','--','consolecrypt-agent.service'],capture_output=True,timeout=5,check=True)
    assert len(out.stdout)+len(out.stderr)<=4096
    return dict(line.split('=',1)for line in out.stdout.decode().splitlines())

def runtime(stage):
    assert stage in('install','update','rollback');expected={'install':'0.1.0','update':'0.1.1','rollback':'0.1.0'}[stage]
    original=json.loads((OBS/'initial-key.json').read_bytes());assert original['agentId']==agent()
    previous=OBS/'last-runtime.json'
    if stage!='install':
        prior=json.loads(previous.read_bytes())
        for child in[prior['main'],*prior['workers']]:
            try:
                observed=OwnedPidObserver(child['pid'])
            except(FileNotFoundError,ProcessLookupError):continue
            with observed:assert observed.starttime!=child['starttime']or not observed.alive(),'Original process still alive after actual role restart'
    info=show();pid=int(info['MainPID']);assert pid>1 and info['ActiveState']=='active'and info['ControlGroup']=='/system.slice/consolecrypt-agent.service'
    with OwnedPidObserver(pid)as observed:
        assert observed.fd is not None
        ready_without_proof(18991,timeout=60,alive=observed.alive)
        assert observed.alive()and show()['MainPID']==str(pid)
        assert_no_owner_fd(pid,OWNER)
        source_binding=running_python_binding(pid,stage)
        resource=json.loads((CONFIG/'manifest.json').read_bytes())['heartbeat_resource_id']
        ordinal={'install':1,'update':2,'rollback':3}[stage]
        owner('lease','--resource',resource,'--lease-id',str(uuid.uuid5(uuid.UUID(token()),'lease-'+str(ordinal))))
        binding=json.loads((AUTH/'request-input.json').read_bytes())
        (CONFIG/'node-owner-pin.json').write_bytes((AUTH/'node-owner-pin.json').read_bytes());(CONFIG/'node-owner-pin.json').chmod(0o644)
        proof=node('sign-request',value=binding)
        bundle=base64.urlsafe_b64encode(json.dumps({'lease':binding['lease'],'freshness':binding['freshness'],'proof':proof}).encode()).decode().rstrip('=')
        c=http.client.HTTPConnection('127.0.0.1',18991,timeout=10)
        try:
            c.request('GET','/v1/heartbeat',headers={'x-consolecrypt-proof':bundle});response=c.getresponse();raw=response.read(65537)
            assert len(raw)<=65536 and response.status==200
            actual=json.loads(raw);assert actual['agent_id']==agent()and actual['version']==expected
        finally:c.close()
        assert observed.alive()and show()['MainPID']==str(pid)
        assert running_python_binding(pid,stage)==source_binding
        workers=owned_workers(pid,BINARY,binary_sha(),OWNER)
        worker_facts=[]
        for worker in workers:
            with OwnedPidObserver(worker)as child:
                assert child.fd is not None and child.alive()
                worker_facts.append({'pid':worker,'starttime':child.starttime})
        process_facts={'main':{'pid':pid,'starttime':observed.starttime},'workers':worker_facts}
    previous.write_text(json.dumps(process_facts));previous.chmod(0o600)
    key=KEY/'node-identity.key';meta=key.stat()
    assert[meta.st_dev,meta.st_ino]==original['keyInode']and hashlib.sha256(key.read_bytes()).hexdigest()==original['privateKeySha256']
    rows=(HOME/'history/history.jsonl').read_bytes().splitlines();assert rows and len(rows)<=1000
    if stage=='install':(OBS/'first-history.json').write_bytes(rows[0])
    else:assert(OBS/'first-history.json').read_bytes()in rows
    return{'version':1,'phase':stage,'actualRuntimeVersion':actual['version'],'agentId':agent(),'keyFingerprint':original['keyFingerprint'],'mainPid':pid,'realWorkerCount':len(workers),'originalKeyPreserved':True,'originalHistoryPreserved':True,'actualSignedRead':True,'ownerDescriptorInherited':False,'actualRuntimeVersionSource':'owned_python_heartbeat','pythonCodeAndListenerBound':True,'selectedRuntimeSource':source_binding,'rustVersionIsNotPythonRuntimeVersion':True}

def removed():
    info=show();assert info['MainPID']=='0'and info['ActiveState']in('inactive','failed')
    group=Path('/sys/fs/cgroup/system.slice/consolecrypt-agent.service');assert not group.exists()or not(group/'cgroup.procs').read_text().strip()
    prior=json.loads((OBS/'last-runtime.json').read_bytes())
    for child in[prior['main'],*prior['workers']]:
        try:observed=OwnedPidObserver(child['pid'])
        except(FileNotFoundError,ProcessLookupError):continue
        with observed:assert observed.starttime!=child['starttime']or not observed.alive(),'Original process remains after remove'
    original=json.loads((OBS/'initial-key.json').read_bytes());key=KEY/'node-identity.key';meta=key.stat()
    assert[meta.st_dev,meta.st_ino]==original['keyInode']and hashlib.sha256(key.read_bytes()).hexdigest()==original['privateKeySha256']
    assert(OBS/'first-history.json').read_bytes()in(HOME/'history/history.jsonl').read_bytes().splitlines()
    floor=json.loads(Path('/opt/consolecrypt-agent-install/var/lib/consolecrypt-agent/release-anchor.json').read_bytes());assert floor['version']==3
    assert not Path('/opt/consolecrypt-agent-install/opt/consolecrypt-agent/current.json').exists()
    assert(AUTH/'authority-db').exists()and(AUTH/'authority-anchor').exists()
    c_original=json.loads((OBS/'metrics-c-key.json').read_bytes());c_key=C_KEY/'node-identity.key';c_meta=c_key.stat()
    assert[c_meta.st_dev,c_meta.st_ino]==c_original['identity']and hashlib.sha256(c_key.read_bytes()).hexdigest()==c_original['sha256']
    assert C_DB.is_dir()and C_ANCHOR.is_dir()and any(C_DB.iterdir())and any(C_ANCHOR.iterdir())
    assert a_state()==json.loads((OBS/'a-before-metrics-c.json').read_bytes())
    service=subprocess.run(['/usr/bin/systemctl','--no-pager','show','--property=MainPID,ActiveState','consolecrypt-agent-collector.service'],capture_output=True,timeout=5,check=True)
    assert len(service.stdout)<=4096 and b'MainPID=0\n'in service.stdout and b'ActiveState=inactive\n'in service.stdout
    service_group=Path('/sys/fs/cgroup/system.slice/consolecrypt-agent-collector.service')
    assert not service_group.exists()or not(service_group/'cgroup.procs').read_text().strip()
    assert any((HOME/'metrics-disk').iterdir()),'software removal erased collector data'
    return{'version':1,'phase':'remove','originalKeyHistoryAndReplayStatePreserved':True,'serviceMainPidZero':True,'cgroupEmptyOrRemoved':True,'releaseFloor':3,'dataErased':False,'runtimeVersion':None,'separateCIdentityJournalAndWalPreserved':True,'collectorServiceStopped':True}

# C is a different MetricsOnly association; A's certificate and data stay intact.
C_KEY=HOME/'identity/metrics-c-key'
C_DB=HOME/'checkpoints/metrics-c-db'
C_ANCHOR=HOME/'checkpoints/metrics-c-anchor'
C_OWNER=HOME/'checkpoints/metrics-c-owner'
C_PIN=CONFIG/'metrics-c-genesis.json'


def canonical(value):
    return json.dumps(value,sort_keys=True,separators=(',',':'),ensure_ascii=True).encode()+b'\n'


def save(path,value,mode=0o600):
    raw=canonical(value);assert len(raw)<=65536
    fd=os.open(path,os.O_WRONLY|os.O_CREAT|os.O_EXCL|os.O_NOFOLLOW|os.O_CLOEXEC,mode)
    try:assert os.write(fd,raw)==len(raw);os.fsync(fd)
    finally:os.close(fd)


def a_state():
    rows={}
    for root in(KEY,AUTH,HOME/'history'):
        for path in sorted(root.rglob('*')):
            info=path.lstat();assert not stat.S_ISLNK(info.st_mode)
            if stat.S_ISDIR(info.st_mode):continue
            assert stat.S_ISREG(info.st_mode)and info.st_nlink==1 and info.st_size<=16*1024*1024 and len(rows)<512
            rows[str(path.relative_to(HOME))]={'identity':[info.st_dev,info.st_ino],
                'sha256':hashlib.sha256(path.read_bytes()).hexdigest()}
    return rows


def disk_stop_a():
    assert selected_runtime('rollback')['version']=='0.1.0'
    run(['/usr/bin/systemctl','--no-pager','stop','consolecrypt-agent.service'])
    info=show();assert info['MainPID']=='0'and info['ActiveState']in('inactive','failed')
    prior=json.loads((OBS/'last-runtime.json').read_bytes())
    for child in[prior['main'],*prior['workers']]:
        try:observer=OwnedPidObserver(child['pid'])
        except(FileNotFoundError,ProcessLookupError):continue
        with observer:assert observer.starttime!=child['starttime']or not observer.alive()
    # Private key hashes stay in the private observation directory, never output.
    save(OBS/'a-before-metrics-c.json',a_state())
    return{'originalAStoppedAndJoined':True,'freshMetricsActorRequired':True}


def c_agent():return str(uuid.uuid5(uuid.UUID(token()),'agent-install-metrics-c'))


def c_owner(command,*args):
    return owner_via_fd(OWNER,OWNER_SHA,[command,'--provider','sqlite','--out',C_OWNER,'--now',int(time.time()),*args])


def metrics_node_credentials():
    # Only Popen's two freshly created anonymous stdio pipes in this fixed
    # MetricsPeer child; never repair a named file or an existing transport.
    assert os.getuid()==os.geteuid()==0
    before=[os.fstat(fd)for fd in(0,1)]
    identity=lambda value:(value.st_dev,value.st_ino,stat.S_IFMT(value.st_mode),value.st_nlink)
    assert identity(before[0])!=identity(before[1])
    for value in before:
        assert stat.S_ISFIFO(value.st_mode)and value.st_nlink==1
        assert value.st_uid==value.st_gid==0 and stat.S_IMODE(value.st_mode)==0o600
    for fd in(0,1):os.fchown(fd,10002,10002)
    for fd,original in zip((0,1),before):
        value=os.fstat(fd)
        assert identity(value)==identity(original)and value.st_uid==value.st_gid==10002
        assert stat.S_IMODE(value.st_mode)==0o600
    node_credentials()
    assert os.getuid()==os.geteuid()==os.getgid()==os.getegid()==10002


class MetricsPeer:
    """Original normal Node protocol; bounded pipe read and real original join."""
    def __init__(self):
        import selectors
        assert hashlib.sha256(BINARY.read_bytes()).hexdigest()==binary_sha()
        self.child=subprocess.Popen([str(BINARY),'worker-node-enroll-sqlite'],stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,stderr=subprocess.PIPE,preexec_fn=metrics_node_credentials,close_fds=True)
        self.buffer=b'';self.selector=selectors.DefaultSelector();self.selector.register(self.child.stdout,selectors.EVENT_READ)
        self.end=time.monotonic()+55;self.allow_denial=False;assert_no_owner_fd(self.child.pid,OWNER)

    def request(self,kind,payload):
        request_id=str(uuid.uuid4());frame={'format':'cc-native-node-enroll/v1','requestId':request_id,'kind':kind,'payload':payload}
        raw=canonical(frame);assert len(raw)<=16384 and self.child.poll()is None
        self.child.stdin.write(raw);self.child.stdin.flush()
        while b'\n'not in self.buffer:
            remaining=self.end-time.monotonic();assert remaining>0
            assert self.selector.select(remaining),'normal Metrics peer response deadline'
            block=os.read(self.child.stdout.fileno(),16385-len(self.buffer));assert block
            self.buffer+=block;assert len(self.buffer)<=16384
        raw,self.buffer=self.buffer.split(b'\n',1);value=json.loads(raw)
        assert set(value)=={'format','requestId','kind','payload'}and value['format']==frame['format']and value['requestId']==request_id
        assert canonical(value)==raw+b'\n'and value['kind']in(kind,'error')
        return value

    def close(self,success=True):
        try:
            self.child.stdin.close()
            try:code=self.child.wait(timeout=5)
            except subprocess.TimeoutExpired:
                self.child.kill();self.child.wait(timeout=2);raise AssertionError('normal Metrics peer join failed')
            raw=self.child.stderr.read(4097)
            if not success or(code!=0 and not self.allow_denial):
                # SAME original worker's bounded stderr, after its original
                # wait/join and before pipe/guest cleanup. No second worker.
                try:
                    path=OBS/'metrics-peer.stderr';retained=raw[:4096]
                    with path.open('xb')as stream:stream.write(retained)
                    path.chmod(0o600);info=path.lstat()
                    assert stat.S_ISREG(info.st_mode)and info.st_uid==0 and info.st_nlink==1
                    assert stat.S_IMODE(info.st_mode)==0o600 and info.st_size==len(retained)
                    print('CC_AGENT_METRICS_PEER_EXIT '+json.dumps({'exit':code,'stderrBytesObserved':len(raw),
                        'privateBytes':len(retained),'truncated':len(raw)>4096,'sha256':hashlib.sha256(retained).hexdigest(),
                        'privateRetained':True,'sameOriginalProcess':True,'secondWorker':False}),flush=True)
                except Exception as capture:
                    print('CC_AGENT_METRICS_PEER_EXIT '+json.dumps({'exit':code,'stderrBytesObserved':len(raw),
                        'privateRetained':False,'captureErrorType':type(capture).__name__,'secondWorker':False}),flush=True)
            assert len(raw)<=4096
            if success and not self.allow_denial:assert code==0 and self.buffer==b''
        finally:self.selector.close();self.child.stdout.close();self.child.stderr.close()

    def __enter__(self):return self
    def __exit__(self,kind,*_):self.close(kind is None)


def reply(peer,kind,payload):
    value=peer.request(kind,payload);assert value['kind']==kind,'actual Metrics Node denied fixed typed request'
    return value['payload']


def disk_prepare():
    assert a_state()==json.loads((OBS/'a-before-metrics-c.json').read_bytes())
    selected=selected_runtime('disk');assert selected['version']=='0.1.2'and show()['MainPID']=='0'
    expected=expected_releases()['phases']['disk'];release=CODE/'releases/0.1.2'
    record=json.loads((release/'installed.json').read_bytes())
    for name,digest in expected['diskFiles'].items():
        for path in(release/name,CODE/name):
            info=path.lstat();assert stat.S_ISREG(info.st_mode)and info.st_uid==0 and info.st_nlink==1
            assert stat.S_IMODE(info.st_mode)==(0o755 if name.endswith('prometheus-hardmode')else 0o644)
            assert hashlib.sha256(path.read_bytes()).hexdigest()==record['files'][name]==digest
    binary=CODE/'collector/prometheus-hardmode';assert binary.stat().st_size==expected['prometheusBytes']==144399412
    manifest={'schema_version':2,'mode':'remote_write','targets':[{'id':'owned','url':'http://127.0.0.1:18992/metrics',
        'kind':'application','sample_limit':256,'series_limit':256,'response_bytes':65536,'label_limit':16}],
        'labels':{'cc_host_id':str(uuid.uuid5(uuid.UUID(token()),'metrics-c-host')),'site':'owned','environment':'test'},
        'scrape_interval_seconds':1,'receiver':{'url':'http://127.0.0.1:18993/api/v1/write'},
        'buffer':{'max_bytes':536870912,'max_age_seconds':7200},'profile':'maintained-prometheus-disk-512mib-2h-v1',
        'bufferMode':'persistentNativeWal','runtime':{'kind':'prometheusHardmode','upstreamTag':'3.16.0',
        'platform':'linux-x86_64','binarySha256':expected['diskFiles']['collector/prometheus-hardmode'],'binaryBytes':144399412}}
    digest=hashlib.sha256(canonical(manifest)[:-1]).digest();resource=bytearray(digest[:16]);resource[6]=resource[6]&15|128;resource[8]=resource[8]&63|128
    catalog={'format':'cc-agent-disk-collector/v2','collectorId':str(uuid.UUID(bytes=bytes(resource))),'manifest':manifest,
        'prometheusSha256':manifest['runtime']['binarySha256'],'interpreterSha256':hashlib.sha256(Path('/usr/bin/python3.12').read_bytes()).hexdigest()}
    for field,name in(('adapterSha256','disk_unit_adapter.py'),('entrySha256','disk_unit_entry.py'),('specSha256','disk_unit_spec.py'),('bindingSha256','disk_binding.py')):
        catalog[field]=expected['diskFiles']['collector/'+name]
    save(CONFIG/'collector-disk.json',catalog,0o644)
    save(CONFIG/'collector-runtime.json',{'format':'cc-agent-installed-disk-collector-profile/v1','uid':10002,'gid':10002})
    provision=provision_collector_once()
    assert provision['profile']=='ubuntu24-systemd255-disk-v1'and provision['namespaceProbeExit']==0
    C_OWNER.mkdir(mode=0o700);os.chown(C_OWNER,10002,10002)
    public=run([BINARY,'provision-node','--state-dir',C_KEY,'--agent-id',c_agent()],node=True)
    key=C_KEY/'node-identity.key';info=key.stat()
    save(OBS/'metrics-c-key.json',{'identity':[info.st_dev,info.st_ino],'sha256':hashlib.sha256(key.read_bytes()).hexdigest()})
    save(C_OWNER/'node-public.json',public,0o644)
    c_owner('metrics-offer','--node-public',C_OWNER/'node-public.json','--profile-id',str(uuid.uuid5(uuid.UUID(token()),'metrics-c-profile')),
        '--deployment-id',str(uuid.uuid5(uuid.UUID(token()),'metrics-c-deployment')))
    genesis=json.loads((C_OWNER/'node-owner-pin.json').read_bytes());save(C_PIN,genesis,0o644)
    initialized=run([BINARY,'init-sqlite','--db-dir',C_DB,'--anchor-dir',C_ANCHOR,'--owner-pin',C_PIN],node=True)
    assert initialized=={'initialized':True,'provider':'sqlite-journal-v1'}
    save(CONFIG/'enrollment-control.json',{'format':'cc-agent-enroll-config/v1','agentId':c_agent(),
        'nodeDir':str(C_KEY),'dbDir':str(C_DB),'anchorDir':str(C_ANCHOR),'ownerPin':str(C_PIN)},0o644)
    with MetricsPeer()as peer:
        probe=reply(peer,'probeCollectorService',{});assert probe['certificate']is None and not probe['revoked']
        assert probe['node']==public and probe['advertisedScopes']==['metrics.collect']
        assert probe['collector']=={'collectorId':catalog['collectorId'],'manifestSha256':digest.hex(),'manifest':manifest}
        claim=reply(peer,'stageOffer',{'expectedActorRevision':probe['actorRevision'],'expectedOwnerPin':probe['ownerPin'],
            'offer':json.loads((C_OWNER/'offer.json').read_bytes()),'token':'21'*32})
        save(C_OWNER/'claim.json',claim['claim'],0o644);c_owner('metrics-approve','--claim',C_OWNER/'claim.json')
        applied=reply(peer,'applyCertificate',{'expectedRev':claim['actorRevision'],'expectedPin':claim['ownerPin'],
            'claimDigest':claim['claimDigest'],'certificate':json.loads((C_OWNER/'certificate.json').read_bytes())})
        observed=reply(peer,'observeCertificate',{'expectedRev':applied['actorRevision'],'expectedPin':applied['ownerPin'],
            'agentId':c_agent(),'certificateDigest':applied['certificateDigest']})
        assert observed['certificate']['scopes']==['metrics.collect']and observed['revoked']is False
    assert a_state()==json.loads((OBS/'a-before-metrics-c.json').read_bytes())
    return{'separateMetricsActor':True,'agentId':c_agent(),'originalAPreserved':True,'actualMetricsCertificate':True,
        'collectorId':catalog['collectorId'],'fullManifestSha256':digest.hex(),'actualRuntimeVersion':None,
        'selectedRelease':selected,'installedPrometheusSha256':manifest['runtime']['binarySha256'],'installedPrometheusBytes':144399412}


def probe_file(value):
    path=C_OWNER/'metrics-probe.json';raw=canonical(value);assert len(raw)<=16384
    if not path.exists():save(path,value,0o644);return
    fd=os.open(path,os.O_WRONLY|os.O_NOFOLLOW|os.O_CLOEXEC)
    try:
        info=os.fstat(fd);assert stat.S_ISREG(info.st_mode)and info.st_uid==0 and info.st_nlink==1
        os.ftruncate(fd,0);assert os.write(fd,raw)==len(raw);os.fsync(fd)
    finally:os.close(fd)


def service_request(command,kind,filename):
    with MetricsPeer()as peer:
        probe=reply(peer,'probeCollectorServiceStatus'if command!='collector-service-grant'else'probeCollectorService',{})
        assert probe['node']['agentId']==c_agent()and probe['advertisedScopes']==['metrics.collect']
        if command!='collector-service-grant':assert probe['collector']is None
        probe_file(probe);c_owner(command)
        value=reply(peer,kind,json.loads((C_OWNER/filename).read_bytes()))
        assert value['receipt']['body']['agentId']==c_agent()
        return value


def metric_values(port):
    import math
    c=http.client.HTTPConnection('127.0.0.1',port,timeout=3)
    try:
        c.request('GET','/metrics');response=c.getresponse();raw=response.read(2*1024*1024+1)
        assert response.status==200 and len(raw)<=2*1024*1024
    finally:c.close()
    wanted={'prometheus_agent_samples_appended_total','prometheus_agent_hard_charged_bytes',
        'prometheus_agent_hard_loss_unknown','prometheus_agent_hard_unavailable',
        'prometheus_agent_hard_byte_evicted_segments_total','prometheus_agent_hard_age_evicted_segments_total'}
    values={}
    for line in raw.decode('ascii').splitlines():
        if line.startswith('#'):continue
        fields=line.split()
        if fields and fields[0]in wanted:
            assert len(fields)==2 and fields[0]not in values
            number=float(fields[1]);assert math.isfinite(number)and number>=0;values[fields[0]]=number
    assert set(values)==wanted
    assert values['prometheus_agent_hard_loss_unknown']in(0,1)and values['prometheus_agent_hard_unavailable']in(0,1)
    return values


def node_record(path):
    fd=os.open(path,os.O_RDONLY|os.O_NOFOLLOW|os.O_NONBLOCK|os.O_CLOEXEC)
    try:
        info=os.fstat(fd);assert stat.S_ISREG(info.st_mode)and info.st_uid==10002 and info.st_nlink==1 and 0<info.st_size<=16384
        identity=lambda m:(m.st_dev,m.st_ino,m.st_mode,m.st_uid,m.st_gid,m.st_nlink,m.st_size,m.st_mtime_ns,m.st_ctime_ns)
        raw=os.read(fd,16385);assert len(raw)==info.st_size and identity(os.fstat(fd))==identity(info)==identity(path.lstat())
        return json.loads(raw)
    finally:os.close(fd)


def _collector_failure_facts(end):
    # Read-only bounded facts from the SAME failed service before guest cleanup.
    # No worker, signed request, manager restart or raw journal persistence.
    fields=('MainPID','ExecMainPID','ExecMainCode','ExecMainStatus','ActiveState','SubState','Result',
            'MemoryCurrent','MemoryPeak','MemoryMax','TasksCurrent','ControlGroup')
    groups={'consolecrypt-agent-collector.service':'/system.slice/consolecrypt-agent-collector.service',
            'user@10002.service':'/user.slice/user-10002.slice/user@10002.service',
            'dbus.service':'/system.slice/dbus.service'}
    states={'active','inactive','failed','activating','deactivating','running','dead','exited','stop-sigterm',
            'success','exit-code','signal','core-dump','timeout','oom-kill','start-limit-hit','resources','unknown'}
    facts={'readOnly':True,'units':{},'workerExits':[],'journalObserved':False}
    for unit,expected_group in groups.items():
        item={'statusObserved':False,'memoryEventsObserved':False};facts['units'][unit]=item
        remaining=end-time.monotonic()
        if remaining<=0:break
        try:
            value=subprocess.run(['/usr/bin/systemctl','--no-pager','show',unit,'--property='+','.join(fields)],
                capture_output=True,close_fds=True,timeout=min(1,remaining))
            if value.returncode!=0 or len(value.stdout)+len(value.stderr)>8192:continue
            rows=[line.split('=',1)for line in value.stdout.decode('ascii').splitlines()]
            if any(len(row)!=2 for row in rows)or len({row[0]for row in rows})!=len(rows):continue
            row=dict(rows)
            if set(row)!=set(fields):continue
            safe={}
            for key,entry in row.items():
                if key=='ControlGroup':continue
                if entry.isascii()and entry.isdecimal():safe[key]=int(entry)
                elif entry in states:safe[key]=entry
                elif entry in('','[not set]','infinity'):safe[key]='unknown'
                else:raise ValueError('closed_service_status')
            item.update(statusObserved=True,status=safe)
            if row['ControlGroup']!=expected_group:continue
            group=Path('/sys/fs/cgroup'+expected_group)
            if not group.is_dir()or group.is_symlink():continue
            events=(group/'memory.events').read_bytes()
            if len(events)>4096:continue
            observed={}
            for line in events.decode('ascii').splitlines():
                key,count=line.split()
                if key not in('low','high','max','oom','oom_kill','oom_group_kill')or not count.isdecimal()or key in observed:
                    raise ValueError('closed_memory_events')
                observed[key]=int(count)
            item.update(memoryEventsObserved=True,memoryEvents=observed)
        except Exception:pass # Absence/unknown is retained, never changed to zero.
    # Fixed stage booleans only: no namespace name, private file body or grant.
    facts['namespace']={'observed':False}
    try:
        import itertools
        root=Path('/run/consolecrypt-agent/disk')
        names=[entry for entry in itertools.islice(root.iterdir(),3)if re.fullmatch('[0-9a-f]{32}',entry.name)]
        if len(names)==1:
            directory=names[0];meta=directory.lstat()
            if stat.S_ISDIR(meta.st_mode)and not directory.is_symlink()and meta.st_uid==10002 and stat.S_IMODE(meta.st_mode)==0o700:
                stages={}
                for name in('session.json','launch-owner.json','preexec.json','prometheus-exec.json','prometheus-exit.json'):
                    try:
                        item=(directory/name).lstat()
                        stages[name]=stat.S_ISREG(item.st_mode)and item.st_uid==10002 and item.st_nlink==1
                    except FileNotFoundError:stages[name]=False
                facts['namespace']={'observed':True,'stageFiles':stages}
        elif not names:facts['namespace']={'observed':True,'directoryPresent':False}
    except Exception:pass
    remaining=end-time.monotonic()
    if remaining>0:
        try:
            value=subprocess.run(['/usr/bin/journalctl','--no-pager','--unit=consolecrypt-agent-collector.service',
                '--lines=32','--output=cat'],capture_output=True,close_fds=True,timeout=min(1,remaining))
            if value.returncode==0 and len(value.stdout)+len(value.stderr)<=16384:
                facts['journalObserved']=True
                allowed={'InvalidWire','InvalidKey','InvalidSignature','InvalidTime','Expired','WrongContext','WrongEpoch',
                    'WrongCheckpoint','UnsupportedTeam','Replay','NotFound','WrongState','InvalidToken','WrongClaim',
                    'ScopeDenied','CapabilityDenied','MissingFreshness','Revoked','Quota','Conflict','Storage','Entropy',
                    'KeyReuse','UncertainCommit','unknown'}
                for line in value.stdout.decode('utf-8',errors='replace').splitlines():
                    prefix='CC_AGENT_COLLECTOR_WORKER_EXIT '
                    if not line.startswith(prefix):continue
                    raw=line[len(prefix):];row=json.loads(raw)
                    if (type(row)is dict and set(row)=={'errorCode','workerExit','workerPid','parentPid'}
                            and json.dumps(row,sort_keys=True,separators=(',',':'))==raw
                            and row['errorCode']in allowed and type(row['workerExit'])is int and -255<=row['workerExit']<=255
                            and all(type(row[key])is int and row[key]>1 for key in('workerPid','parentPid'))):
                        facts['workerExits'].append(row)
        except Exception:pass
    return facts


def disk_service():
    from http.server import BaseHTTPRequestHandler,ThreadingHTTPServer
    import threading
    # This is an owned public scrape/HTTP observation, NOT an ACK journal or a
    # replacement decoder. An HTTP204 is never reported as exact sample delivery.
    state={'scrapes':0,'failedRequests':0,'acceptedRequests':0,'receiverReady':False}
    class Handler(BaseHTTPRequestHandler):
        def log_message(self,*_):pass
        def do_GET(self):
            assert self.path=='/metrics'and self.server.server_port==18992
            state['scrapes']+=1
            body=''.join('cc_owned_wal_probe{slot="%d"} %d\n'%(i,int.from_bytes(hashlib.sha256(('%d/%d'%(i,state['scrapes'])).encode()).digest()[:6],'big'))for i in range(192)).encode()
            self.send_response(200);self.send_header('Content-Length',str(len(body)));self.end_headers();self.wfile.write(body)
        def do_POST(self):
            assert self.path=='/api/v1/write'and self.server.server_port==18993
            self.connection.settimeout(3);count=int(self.headers['Content-Length']);assert 0<count<=1048576
            raw=self.rfile.read(count);assert len(raw)==count
            key='acceptedRequests'if state['receiverReady']else'failedRequests';state[key]+=1
            self.send_response(204 if state['receiverReady']else 503);self.send_header('Content-Length','0');self.end_headers()
    servers=[];threads=[];observed=None;failure_capture_end=time.monotonic()+110
    try:
        for port in(18992,18993):
            server=ThreadingHTTPServer(('127.0.0.1',port),Handler);server.daemon_threads=False
            thread=threading.Thread(target=server.serve_forever);thread.start();servers.append(server);threads.append(thread)
        configured=service_request('collector-service-grant','configureCollectorService','collector-service-configure.json')
        assert configured['receipt']['body']['effect']=='configured'
        save(OBS/'metrics-c-configure.json',configured)
        end=time.monotonic()+20;root=Path('/run/consolecrypt-agent/disk');session=None
        while time.monotonic()<end:
            candidates=[p for p in root.iterdir()if re.fullmatch('[0-9a-f]{32}',p.name)]
            assert len(candidates)<=1
            if candidates and(candidates[0]/'prometheus-exec.json').exists():session=candidates[0];break
            time.sleep(.05)
        if session is None:
            try:facts=_collector_failure_facts(min(failure_capture_end,time.monotonic()+5))
            except Exception:facts={'readOnly':True,'captureAvailable':False}
            try:save(OBS/'metrics-service-failure.json',facts)
            except Exception:pass
            try:print('CC_AGENT_COLLECTOR_SERVICE_FAILURE_FACTS '+json.dumps(facts,sort_keys=True,separators=(',',':')),flush=True)
            except OSError:pass
        assert session is not None,'installed normal Prometheus never reached actual exec'
        handoff=node_record(session/'session.json');executed=node_record(session/'prometheus-exec.json');preexec=node_record(session/'preexec.json')
        expected=expected_releases()['phases']['disk'];assert handoff['format']=='cc-collector-service-session/v2'
        assert executed['format']=='cc-disk-prometheus-exec/v1'and executed['session']==session.name
        assert handoff['binarySha256']==executed['prometheusSha256']==expected['diskFiles']['collector/prometheus-hardmode']
        assert handoff['runtime']['binaryBytes']==144399412
        pid=executed['childPid'];observed=OwnedPidObserver(pid)
        assert observed.fd is not None and observed.alive()and int(observed.starttime)==executed['childStartTicks']
        proc=Path('/proc')/str(pid);mapped=(proc/'exe').stat()
        assert(mapped.st_dev,mapped.st_ino)==(executed['childExeDev'],executed['childExeIno'])
        with(proc/'exe').open('rb')as image:assert hashlib.file_digest(image,'sha256').hexdigest()==handoff['binarySha256']
        assert_no_owner_fd(pid,OWNER)
        listener_end=time.monotonic()+10
        while True:
            assert observed.alive()
            sockets=set()
            for entry in(proc/'fd').iterdir():
                try:link=os.readlink(entry)
                except FileNotFoundError:continue
                match=re.fullmatch(r'socket:\[([0-9]+)\]',link)
                if match:sockets.add(match.group(1))
            try:bound_listener((proc/'net/tcp').read_bytes(),sockets,handoff['statusPort']);break
            except AssertionError:
                assert time.monotonic()<listener_end,'normal Prometheus owned listener did not become ready'
                time.sleep(.05)
        assert preexec['session']==session.name and preexec['binarySha256']==handoff['binarySha256']
        assert preexec['kernelLimits']=={'memory.max':'536870912','memory.swap.max':'0','pids.max':'64','cpu.max':'100000 100000'}
        cpath=preexec['cgroup'];assert cpath.startswith('/user.slice/user-10002.slice/user@10002.service/')and'..'not in cpath.split('/')
        assert cpath.rsplit('/',1)[1]=='cc-agent-disk-'+session.name+'.service'
        cgroup=Path('/sys/fs/cgroup')/cpath.lstrip('/')
        members={int(p)for p in(cgroup/'cgroup.procs').read_text().split()}
        assert{pid,executed['entryPid']}<=members
        argv=(proc/'cmdline').read_bytes().split(b'\0');assert b'--storage.agent.hard-budget'in argv
        assert b'--storage.agent.path=/var/lib/consolecrypt-agent/metrics-disk/'+json.loads((CONFIG/'collector-disk.json').read_bytes())['collectorId'].encode()+b'/wal'in argv
        before=None;end=time.monotonic()+25
        while time.monotonic()<end:
            assert observed.alive()
            try:values=metric_values(handoff['statusPort'])
            except(ConnectionError,OSError):time.sleep(.1);continue
            if state['failedRequests']>0 and values['prometheus_agent_samples_appended_total']>=192 and values['prometheus_agent_hard_charged_bytes']>0:
                before=values;break
            time.sleep(.1)
        assert before is not None and before['prometheus_agent_hard_unavailable']==0
        catalog=json.loads((CONFIG/'collector-disk.json').read_bytes());wal=HOME/'metrics-disk'/catalog['collectorId']/'wal'
        payload=[p for p in wal.iterdir()if re.fullmatch('[0-9]{8}',p.name)]
        wal_bytes=sum(p.stat().st_size for p in payload);assert payload and wal_bytes>0
        state['receiverReady']=True;end=time.monotonic()+15
        while state['acceptedRequests']==0 and time.monotonic()<end:assert observed.alive();time.sleep(.1)
        assert state['acceptedRequests']>0
        after=metric_values(handoff['statusPort'])
        status=service_request('collector-service-status','observeCollectorService','collector-service-observe.json')
        assert status['receipt']['body']['effect']=='configured';save(OBS/'metrics-c-status.json',status)
        revoked=service_request('collector-service-revoke','revokeCollectorService','collector-service-revoke.json')
        assert revoked['receipt']['body']['effect']=='revoked';save(OBS/'metrics-c-revoke.json',revoked)
        end=time.monotonic()+18
        while time.monotonic()<end and(observed.alive()or cgroup.exists()and(cgroup/'cgroup.procs').read_text().strip()):time.sleep(.05)
        assert not observed.alive(),'original actual normal Prometheus did not stop after signed revoke'
        assert not cgroup.exists()or not(cgroup/'cgroup.procs').read_text().strip(),'original collector cgroup not quiescent'
        while any(re.fullmatch('[0-9a-f]{32}',p.name)for p in root.iterdir())and time.monotonic()<end:time.sleep(.05)
        assert not any(re.fullmatch('[0-9a-f]{32}',p.name)for p in root.iterdir())
        with MetricsPeer()as peer:
            current=reply(peer,'probeCollectorService',{})
            replay=json.loads((C_OWNER/'collector-service-configure.json').read_bytes())
            # Fresh CAS expectations remove stale-revision rejection as an
            # alternative explanation. Only the ORIGINAL signed policy is reused.
            replay['expectedRev']=current['actorRevision'];replay['expectedPin']=current['ownerPin']
            denied=peer.request('configureCollectorService',replay)
            assert denied['kind']=='error'and denied['payload']['code']=='denied'and denied['payload']['effect']in('notCommitted','unknown')
            peer.allow_denial=True
        final_status=service_request('collector-service-status','observeCollectorService','collector-service-observe.json')
        assert final_status['receipt']['body']['effect']=='revoked'and not observed.alive()
        assert not any(re.fullmatch('[0-9a-f]{32}',p.name)for p in root.iterdir())
        assert a_state()==json.loads((OBS/'a-before-metrics-c.json').read_bytes())
        return{'version':1,'separateMetricsActor':True,'agentId':c_agent(),'actualRuntimeVersion':None,
            'installedNormalPrometheusSha256':handoff['binarySha256'],'installedNormalPrometheusBytes':144399412,
            'actualPidFdAndMappedElf':True,'actualScrapeWalAndUnavailableReceiver':True,'walBytesObserved':wal_bytes,
            'nativeBefore':before,'nativeAfter':after,'receiverRequests':{'failed':state['failedRequests'],'http204':state['acceptedRequests']},
            'confirmedAcceptedSamples':None,'exactUnacknowledgedLoss':None,'signedServiceConfiguredObservedRevoked':True,
            'originalPrometheusStoppedAndPrivateSessionRemoved':True,'originalPolicyReplayDenied':True,'replayResultEffect':denied['payload']['effect'],'originalAPreserved':True,
            'newInstalledThreeHourClaim':False,'newInstalledByteEvictionPressureClaim':False}
    finally:
        if observed is not None:observed.close()
        for server in servers:server.shutdown();server.server_close()
        for thread in threads:thread.join(timeout=3);assert not thread.is_alive()


def main():
    assert os.getuid()==os.geteuid()==0 and Path('/proc/1/comm').read_text().strip()=='systemd'and len(sys.argv)==2
    assert(FIXTURE/'token').read_text().strip()==token()
    action=sys.argv[1]
    value=bootstrap()if action=='bootstrap'else removed()if action=='removed'else runtime(action)
    print(json.dumps(value,sort_keys=True))
if __name__=='__main__':main()
