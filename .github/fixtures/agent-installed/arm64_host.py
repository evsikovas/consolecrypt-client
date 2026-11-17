"""Native ARM host transport for the EXISTING installed Agent acceptance.

Source only until reviewed and explicitly invoked on a disposable hosted VM.
Consumes the existing canonical packet; never builds, signs, downloads, creates
an authority implementation, or treats preparation as installed acceptance.
"""
import argparse
import ctypes
import grp
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import platform
import pwd
import re
import signal
import stat
import subprocess
import sys
import time
import uuid
import zipfile

HERE = Path(__file__).resolve().parent
FIXTURE = Path('/opt/cc-agent-install-fixture')
OBS = Path('/cc-persist/software/agent-observation')
AUTHORITY_SOURCE = '7f403db17f36d26ad4c084b2dda0e794f7b7b6b81cb010f2de500241c766da02'
TARGET = 'aarch64-unknown-linux-gnu'
ANSIBLE_CATALOG_SHA = '34d44603cda4e8ccf3ee29a21b24241f834a0f694a46ddb0664401ee89c8ceaf'


def need(value, message):
    if not value:
        raise ValueError(message)


def digest(raw):
    return hashlib.sha256(raw).hexdigest()


def read(path, cap=16*1024**2):
    path = Path(path)
    need(path.is_absolute(), 'absolute input required')
    for ancestor in (path, *path.parents):
        need(not stat.S_ISLNK(ancestor.lstat().st_mode), 'physical inputs required')
    with path.open('rb') as stream:
        before = os.fstat(stream.fileno())
        need(stat.S_ISREG(before.st_mode) and before.st_nlink == 1 and 0 < before.st_size <= cap,
             'bounded regular singlelink input required')
        raw = stream.read(cap+1)
        identity = lambda m: (m.st_dev,m.st_ino,m.st_size,m.st_mtime_ns,m.st_ctime_ns)
        need(len(raw) == before.st_size and identity(before) == identity(os.fstat(stream.fileno()))
             == identity(path.lstat()), 'stable complete input required')
    return raw


def load(path, cap=16*1024**2):
    return json.loads(read(path,cap))


def elf(raw):
    need(len(raw) >= 64 and raw[:7] == b'\x7fELF\x02\x01\x01'
         and int.from_bytes(raw[18:20],'little') == 183, 'actual AArch64 ELF required')


def command(argv, log, timeout=240, env=None):
    # One finite command under the GitHub job deadline, not a new VM executor.
    from prometheus_build import group_members
    with log.open('xb') as stream:
        child = subprocess.Popen([str(v) for v in argv],stdin=subprocess.DEVNULL,
            stdout=stream,stderr=subprocess.STDOUT,start_new_session=True,env=env)
        try:
            need(child.wait(timeout=timeout) == 0, 'existing role/host command failed')
        finally:
            # Reuse the existing native producer's group census/retire/reap
            # policy, including an exited leader with surviving descendants.
            child.poll()
            late_children = child.returncode is not None and bool(group_members(child.pid))
            for sig in (signal.SIGTERM,signal.SIGKILL):
                if not group_members(child.pid):
                    break
                try:
                    os.killpg(child.pid,sig)
                except ProcessLookupError:
                    pass
                end = time.monotonic()+3
                while time.monotonic() < end:
                    child.poll()
                    try:
                        while os.waitpid(-child.pid,os.WNOHANG)[0]:
                            pass
                    except ChildProcessError:
                        pass
                    if not group_members(child.pid):
                        break
                    time.sleep(.05)
            child.wait(timeout=3)
            need(not group_members(child.pid), 'owned command process group not joined')
        need(not late_children, 'command leader exited with descendants')
        need(log.stat().st_size <= 1024**2, 'bounded role log required')


def ansible_inputs(args, pin):
    catalog_raw = read(args.ansible_wheel_catalog.absolute(),65536)
    need(digest(catalog_raw) == ANSIBLE_CATALOG_SHA,'reviewed exact native Ansible wheel catalog required')
    catalog = json.loads(catalog_raw)
    packages = catalog['packages']
    need(len(packages) == 9 and catalog['architecture'] == 'aarch64' and catalog['python'] == 'CPython3.12',
         'fixed minimal installed-controller dependencies required')
    lock_raw = read(args.ansible_lock.absolute(),16384)
    lock = ''.join(row['name']+' @ '+row['url']+' --hash=sha256:'+row['sha256']+'\n' for row in packages).encode()
    need(lock_raw == lock and digest(lock_raw) == catalog['lockSha256'],'exact ARM direct-wheel hash lock required')
    report = load(args.ansible_install_report.absolute(),262144)
    environment = report['environment']
    need(report['version'] == '1' and environment['implementation_name'] == 'cpython'
         and environment['platform_machine'] == 'aarch64' and environment['sys_platform'] == 'linux'
         and environment['python_version'] == '3.12','actual native ARM pip install report required')
    canonical = lambda name: name.lower().replace('_','-').replace('.','-')
    wanted = {canonical(row['name']):row for row in packages}
    observed = {}
    for row in report['install']:
        name = canonical(row['metadata']['name'])
        need(name in wanted and name not in observed,'exact native dependency install closure required')
        expected = wanted[name]
        need(row['metadata']['version'] == expected['version'] and row['is_direct'] is True
             and row['download_info']['url'] == expected['url']
             and row['download_info']['archive_info']['hashes']['sha256'] == expected['sha256'],
             'actual installed wheel must match its original ARM hash lock')
        observed[name] = row['metadata']['version']
    need(set(observed) == set(wanted),'no missing ARM controller dependency')
    interpreter = args.ansible_python.absolute().resolve(strict=True)
    need(digest(read(interpreter,32*1024**2)) == pin['pythonSha256'],
         'Ansible venv must execute the independently reviewed ARM interpreter')
    # Bounded native metadata readback only; code runs solely after explicit
    # host execution is authorized, never during source preparation/import.
    code = "import importlib.metadata,json,platform,sys;names=json.loads(sys.argv[1]);print(json.dumps({'machine':platform.machine(),'python':list(sys.version_info[:2]),'versions':{n:importlib.metadata.version(n) for n in names}},sort_keys=True))"
    result = subprocess.run([str(args.ansible_python.absolute()),'-I','-B','-c',code,json.dumps(list(wanted))],
        capture_output=True,timeout=10,check=True)
    need(len(result.stdout)+len(result.stderr) <= 16384,'bounded actual controller metadata')
    actual = json.loads(result.stdout)
    need(actual == dict(machine='aarch64',python=[3,12],versions=observed),'actual installed native Ansible versions changed')


def validate(args):
    need(sys.platform == 'linux' and platform.machine() == 'aarch64'
         and os.getuid() == os.geteuid() == 0, 'native root AArch64 Linux host required')
    need(os.environ.get('GITHUB_ACTIONS') == 'true' and os.environ.get('RUNNER_ARCH') == 'ARM64'
         and os.environ.get('RUNNER_ENVIRONMENT') == 'github-hosted', 'disposable hosted ARM VM only')
    need(Path('/proc/1/comm').read_text().strip() == 'systemd', 'actual host systemd required')
    source, packet = args.agent.absolute(), args.packet.absolute()
    manifest_raw = read(packet/'fixture-packages.json',1024**2)
    need(digest(manifest_raw) == args.packet_sha256, 'explicit reviewed actual ARM packet required')
    manifest = json.loads(manifest_raw)
    need(manifest['format'] == 'cc-canonical-agent-ram-fixture/v1' and manifest['fixtureOnly'] is True
         and manifest['runtimeExecuted'] is manifest['installed'] is False
         and manifest['productionReleaseAuthorityClaimed'] is False, 'uninstalled fixture packet required')
    normal = read(packet/'a/cc-agent-identity',64*1024**2)
    authority = read(packet/'actor-authority/synthetic_node_authority',64*1024**2)
    elf(normal); elf(authority)
    need(digest(normal) == manifest['normalElfSha256'] and digest(authority) == manifest['fixtureAuthoritySha256'],
         'actual normal/helper bytes must match packet')
    normal_receipt = load(packet/'NORMAL_AS_BUILT.json')
    cargo_raw = read(packet/'a/cargo-build.jsonl')
    need(normal_receipt['target'] == TARGET and normal_receipt['sha256'] == digest(normal)
         and normal_receipt['bytes'] == len(normal) and normal_receipt['features'] == []
         and normal_receipt['fresh'] is False and normal_receipt['profile']['opt_level'] == '3'
         and normal_receipt['profile']['test'] is False and normal_receipt['cargoLogSha256'] == digest(cargo_raw),
         'actual normal native release3 empty-feature Cargo provenance required')
    receipt = load(packet/'actor-authority/receipt.json')
    need(receipt['format'] == 'cc-agent-install-actor-authority/v1' and receipt['compiled'] is True
         and receipt['sourceUnchanged'] is True and receipt['sourceSha256'] == AUTHORITY_SOURCE
         and receipt['sha256'] == digest(authority) and receipt['bytes'] == len(authority)
         and receipt['features'] == [] and receipt['profile']['opt_level'] == '3'
         and receipt['profile']['test'] is False and receipt['fresh'] is False
         and receipt['normalBundleBuildSha256'] == digest(read(packet/'a/build-public.json')),
         'same fixed-purpose actor source and real native release compilation required')
    need(receipt['normalSha256'] == digest(normal)
         and receipt['normalReceiptSha256'] == digest(read(packet/'NORMAL_AS_BUILT.json'))
         and receipt['normalCargoLogSha256'] == digest(cargo_raw)
         and receipt['sourceFiles'] == normal_receipt['sourceFiles'],
         'original native normal/helper source frame binding required')
    same_cargo = receipt['sameCargoCommandAsNormal']
    need(type(same_cargo) is bool, 'explicit normal/helper Cargo relationship required')
    helper_raw = cargo_raw if same_cargo else read(packet/'helpers-cargo-build.jsonl')
    need(digest(helper_raw) == receipt['cargoLogSha256'], 'actual helper Cargo log bytes required')
    for raw,target,kind,metadata in ((cargo_raw,'cc-agent-identity','bin',normal_receipt),
                                     (helper_raw,'synthetic_node_authority','example',receipt)):
        cargo = [json.loads(line) for line in raw.splitlines() if line.strip().startswith(b'{')]
        need([row['success'] for row in cargo if row.get('reason') == 'build-finished'] == [True],
             'actual successful Cargo command required')
        artifacts = [row for row in cargo if row.get('reason') == 'compiler-artifact'
                     and row.get('executable') and row['target']['name'] == target]
        need(len(artifacts) == 1 and artifacts[0]['features'] == [] and artifacts[0]['fresh'] is False
             and artifacts[0]['profile'] == metadata['profile'] and artifacts[0]['target']['kind'] == [kind],
             'actual matching release compiler artifact required')
    pin_raw = read(args.runtime_pin.absolute(),16384)
    need(digest(pin_raw) == args.runtime_pin_sha256, 'independently reviewed ARM interpreter pin required')
    pin = json.loads(pin_raw)
    need(set(pin) == {'format','pythonExecutable','pythonSha256'}
         and pin['format'] == 'cc-tunnel-supervisor-runtime/v1'
         and pin['pythonExecutable'] == '/usr/bin/python3.12'
         and digest(read(Path(pin['pythonExecutable']),32*1024**2)) == pin['pythonSha256'],
         'actual host Python must match reviewed Ubuntu ARM pin')
    elf(read(Path(pin['pythonExecutable']),32*1024**2))
    ansible_inputs(args,pin)
    sys.path.insert(0,str(source/'delivery'))
    from release_verifier import ReleaseVerifier
    rows = manifest['packages']
    need(len(rows) == 2, 'same two RAM releases required')
    rows = [*rows,manifest['optionalDiskPackage']]
    phases = {}
    first = None
    for number,letter in enumerate(('a','b','disk'),1):
        base = packet/letter
        build, public = load(base/'build-public.json'), load(base/'package-public.json')
        need(build['target'] == TARGET and build['actualRuntimeVersion'] is None
             and build['sourceBefore'] == build['sourceAfter']
             and build['dependencySourceBefore'] == build['dependencySourceAfter']
             and build['binary']['sha256'] == digest(normal) and build['binary']['bytes'] == len(normal),
             'same actual native ARM package/dependency frame required')
        need(build['cargoLogSha256'] == digest(cargo_raw)
             and build['sourceBefore'] == normal_receipt['shippingSourceBefore'] == normal_receipt['shippingSourceAfter']
             and build['dependencySourceBefore'] == normal_receipt['compiledDependencySourceBefore']
                 == normal_receipt['compiledDependencySourceAfter'], 'same actual normal build receipt frame required')
        if first is None:
            first = build
        need(build['sourceAfter'] == first['sourceAfter']
             and build['dependencySourceAfter'] == first['dependencySourceAfter'], 'one canonical current source frame required')
        for name,wanted in build['sourceAfter'].items():
            need(not Path(name).is_absolute() and '..' not in Path(name).parts and '\\' not in name,
                 'closed Agent source path required')
            need(digest(read(source/name,32*1024**2)) == wanted, 'current Agent source changed')
        row = rows[number-1]
        need(row['sequence'] == number and row['version'] == '0.1.'+str(number-1)
             and row['sourceSha256'] == public['inputSha256']
             and digest(read(base/'candidate.zip',96*1024**2)) == row['artifactSha256']
             and digest(read(base/'signed/release-manifest.json',65536)) == row['manifestSha256']
             and digest(read(base/'signed/release-public-key.json',4096)) == manifest['publicKeySha256'],
             'exact same actual ARM release artifacts required')
        verifier = ReleaseVerifier(packet/'a/cc-agent-identity',digest(normal),base/'signed/release-public-key.json',
            base/'signed/release-manifest.json',platform='linux-aarch64',current_version=number-1)
        need(verifier(base/'candidate.zip') is True, 'real native normal release verification required')
        with zipfile.ZipFile(base/'candidate.zip') as archive:
            need(json.loads(archive.read('python/supervisor-runtime.json')) == pin,
                 'all packages must retain the separately reviewed ARM runtime pin')
        phases[('install','update','disk')[number-1]] = dict(version=row['version'],artifactSha256=row['artifactSha256'],
            runtimeFiles={name:row['sourceSha256'][name] for name in
                ('runtime/cc_agent/__init__.py','runtime/cc_agent/service.py','integration/__main__.py','launch.py')})
        if number == 3:
            prom = build['prometheus']
            need(64 <= prom['bytes'] <= 192*1024**2, 'bounded actual ARM hard-budget Prometheus required')
            phases['disk']['prometheusBytes'] = prom['bytes']
            phases['disk']['diskFiles'] = {name:row['sourceSha256'][name] for name in
                ('collector/prometheus-hardmode','collector/disk_unit_adapter.py','collector/disk_unit_entry.py',
                 'collector/disk_unit_spec.py','collector/disk_binding.py')}
            need(phases['disk']['diskFiles']['collector/prometheus-hardmode'] == prom['sha256'],
                 'original ARM Prometheus build must match installed payload')
    phases['rollback'] = phases['install']
    need(phases['install']['runtimeFiles'] != phases['update']['runtimeFiles'], 'actual runtime version update required')
    lifecycle_raw = read(HERE/'guest_lifecycle.py',128*1024)
    primitives_raw = read(args.primitives.absolute(),128*1024)
    need(digest(lifecycle_raw) == args.lifecycle_sha256 and digest(primitives_raw) == args.primitives_sha256,
         'explicit reviewed existing acceptance sources required')
    return source,packet,normal,authority,phases,lifecycle_raw,primitives_raw


# Disposable-fixture custody only. The shipping installer never writes sysctl
# or tears down its preserved manager prerequisite during product removal.
ARM_HELPER_SHA = '29c84a9c814f44e3bc54853e9c989ba4b2c67d701e83cad4d9210971ef9886bf'
ARM_CUSTODY_SHA = '01ac2184846b2322ca2d01c1aa8b38b13bf4c21d5312a3916296841e3b9f91c7'
ARM_MANAGER = 'user@10002.service'


def arm_gate_profiles(custody):
    raw = custody.read('/sys/kernel/security/apparmor/profiles',131072)
    rows = raw.decode('utf-8','strict').splitlines()
    need(len(rows) == len(set(rows)), 'duplicate host AppArmor profile observation')
    return set(rows)


def arm_gate_initial_idle():
    # Runs before account creation and BEFORE entering the cleanup-authority
    # try/finally. An unknown/live prior UID instance is never stopped by us.
    for unit in (ARM_MANAGER,'user-runtime-dir@10002.service'):
        result=subprocess.run(['/usr/bin/systemctl','show',unit,'--property=LoadState',
            '--property=ActiveState','--property=MainPID'],stdin=subprocess.DEVNULL,
            capture_output=True,timeout=5,check=False)
        need(len(result.stdout)+len(result.stderr)<=4096,'bounded initial manager observation required')
        rows=[line.split('=',1)for line in result.stdout.decode('utf-8','strict').splitlines()]
        need(len(rows)==3 and all(len(row)==2 for row in rows)
             and {row[0]for row in rows}=={'LoadState','ActiveState','MainPID'},
             'complete initial manager observation required')
        actual=dict(rows)
        need(result.returncode==0 and actual==dict(LoadState='loaded',ActiveState='inactive',MainPID='0')
             or result.returncode in (0,4)and actual==dict(LoadState='not-found',ActiveState='inactive',MainPID='0'),
             'initial fixed UID manager instance is not idle')
    for path in (Path('/run/user/10002'),Path('/sys/fs/cgroup/user.slice/user-10002.slice')):
        need(not path.exists()and not path.is_symlink(),'initial fixed UID runtime or cgroup exists')
    return dict(fixedInstancesIdle=True,runtimeAndUserCgroupAbsent=True,cleanupAuthorityNotYetAcquired=True)


def arm_gate_prepare(gate):
    # Import only the already reviewed guard functions. Its diagnostic main,
    # /usr/bin/true probe, profile publisher and A/B runner are never invoked.
    path = HERE/'arm64_capability_observe.py'
    need(digest(read(path,131072)) == ARM_CUSTODY_SHA, 'reviewed sysctl custody source required')
    spec = importlib.util.spec_from_file_location('arm_installed_sysctl_custody',path)
    custody = importlib.util.module_from_spec(spec);spec.loader.exec_module(custody)
    gate['custody'] = custody
    gate['profilesBefore'] = arm_gate_profiles(custody)
    gate['profileName'] = 'consolecrypt-agent-arm64-manager-10002'
    need(not any(row == gate['profileName'] or row.startswith(gate['profileName']+' (')
                 for row in gate['profilesBefore']), 'fresh product profile required')
    for path in (Path('/etc/apparmor.d/consolecrypt-agent-arm64-manager'),
                 Path('/etc/systemd/system/user@10002.service.d/60-consolecrypt-agent.conf'),
                 Path('/usr/local/libexec/consolecrypt-agent-install')):
        need(not path.exists() and not path.is_symlink(), 'fresh owned ARM manager path required')
    gate['policyBefore'] = arm_gate_policy(Path('/usr/lib/systemd/systemd'))
    gate['freshChecked'] = True
    custody.tighten_transition_sysctl(gate['sysctl'])


def arm_gate_policy(executable):
    names = ['ExecStart','FragmentPath','DropInPaths','AppArmorProfile','User','Group',
             'NoNewPrivileges','CapabilityBoundingSet','AmbientCapabilities']
    result = subprocess.run(['/usr/bin/systemctl','--system','--no-pager','--no-ask-password',
        'show',ARM_MANAGER,*['--property='+name for name in names]],
        stdin=subprocess.DEVNULL,capture_output=True,timeout=5,check=False)
    need(result.returncode == 0 and len(result.stdout)+len(result.stderr) <= 16384,
         'bounded actual manager policy required')
    rows = [line.split('=',1) for line in result.stdout.decode('utf-8','strict').splitlines()]
    need(all(len(row) == 2 for row in rows) and len(rows) == len(names)
         and {row[0] for row in rows} == set(names), 'complete manager policy required')
    values = dict(rows)
    prefix = '{ path='+str(executable)+' ; argv[]='+str(executable)+' --user ; ignore_errors=no ; '
    value = values.pop('ExecStart')
    need(value.startswith(prefix) and value.endswith(' }') and value.count('{') == value.count('}') == 1,
         'exact actual manager entrypoint required')
    return values


def arm_gate_plan(gate,source):
    path = source/'delivery/collector_provision.py'
    need(digest(read(path,131072)) == ARM_HELPER_SHA, 'reviewed product ARM helper required')
    spec = importlib.util.spec_from_file_location('arm_installed_product_helper',path)
    product = importlib.util.module_from_spec(spec);spec.loader.exec_module(product)
    product.select_profile({'format':'cc-agent-installed-disk-collector-profile/v1','uid':10002,'gid':10002})
    product.END = time.monotonic()+55
    # A readonly plan pins the actual vendor/parser/helper/unit inputs BEFORE
    # the original lifecycle invokes the unchanged root provisioning CLI.
    plan = product.arm_manager_plan(10002,10002)
    product.arm_manager_preflight(plan,None)
    gate.update(product=product,plan=plan)


def arm_gate_boot(gate):
    product,plan = gate['product'],gate['plan']
    product.END = time.monotonic()+55
    product.arm_manager_assets(plan)
    row = product.show(ARM_MANAGER,['ActiveState','MainPID','InvocationID','ExecStartPre','ExecStartPost'])
    need(row['ActiveState'] == 'active' and row['MainPID'].isdigit() and int(row['MainPID']) > 1
         and re.fullmatch('[0-9a-f]{32}',row['InvocationID']) is not None,
         'actual active manager invocation required')
    # These are systemd's actual completed commands, not a static drop-in read.
    for phase,field in (('pre','ExecStartPre'),('post','ExecStartPost')):
        value = row[field]
        argv = '/usr/bin/python3.12 -I -B '+str(product.ARM_HELPER)+' --verify-arm-manager '+phase
        prefix = '{ path=/usr/bin/python3.12 ; argv[]='+argv+' ; ignore_errors=no ; '
        need(value.startswith(prefix) and value.endswith(' }')
             and value.count('{') == value.count('}') == 1
             and re.search(r'; pid=[1-9][0-9]* ; code=exited ; status=0 \}$',value) is not None,
             'actual successful manager '+phase+' command required')
        # Execute the SAME installed public CLI as the dedicated UID as well.
        environment = dict(PATH='/usr/bin:/bin',HOME='/var/lib/consolecrypt-agent',
                           XDG_RUNTIME_DIR='/run/user/10002',MAINPID=row['MainPID'])
        result = subprocess.run(['/usr/bin/python3.12','-I','-B',str(product.ARM_HELPER),
            '--verify-arm-manager',phase],stdin=subprocess.DEVNULL,capture_output=True,
            timeout=5,check=False,user=10002,group=10002,extra_groups=[],env=environment)
        need(result.returncode == 0 and result.stdout == result.stderr == b'',
             'actual sameUID readonly manager '+phase+' check required')
    product.arm_manager_process(plan,Path('/proc')/row['MainPID'])
    policy = arm_gate_policy(product.ARM_COPY)
    original = gate['policyBefore']
    need(all(policy[key] == original[key] for key in
        ('FragmentPath','AppArmorProfile','User','Group','NoNewPrivileges','CapabilityBoundingSet','AmbientCapabilities')),
         'original manager capability and execution policy required')
    return dict(invocationId=row['InvocationID'],mainPid=int(row['MainPID']),
        actualSystemdPrePassed=True,actualSystemdPostPassed=True,sameUidPrePassed=True,
        sameUidPostPassed=True,actualOwnedManagerProcessPassed=True,hostRebootObserved=False)


def arm_gate_unlink(product,path,expected,mode,gid=0):
    # Only initially absent exact fixture-owned files are eligible. No glob,
    # recursive remove, account removal, adoption or repair of changed bytes.
    before = path.lstat()
    need(before.st_gid == gid and product.root_file(path,max(len(expected),1),mode) == expected
         and product.ident(path.lstat()) == product.ident(before), 'owned ARM teardown file changed')
    path.unlink()
    need(not path.exists() and not path.is_symlink(), 'owned ARM teardown absence required')


def arm_gate_retire(gate,report,output):
    if not gate.get('freshChecked'):
        return
    custody = gate['custody']
    # This second observation also runs after an unrelated lifecycle cleanup
    # error. It never relaxes the manager/cgroup join prerequisites.
    for unit in ('consolecrypt-agent.service','consolecrypt-agent-collector.service',
                 ARM_MANAGER,'user-runtime-dir@10002.service'):
        result = subprocess.run(['/usr/bin/systemctl','show',unit,'--property=LoadState',
            '--property=ActiveState','--property=MainPID'],stdin=subprocess.DEVNULL,
            capture_output=True,timeout=5,check=False)
        need(len(result.stdout)+len(result.stderr) <= 4096,
             'bounded stopped ARM unit observation required')
        rows = [line.split('=',1) for line in result.stdout.decode('utf-8','strict').splitlines()]
        need(len(rows) == 3 and all(len(row) == 2 for row in rows)
             and {row[0] for row in rows} == {'LoadState','ActiveState','MainPID'},
             'complete stopped ARM unit observation required')
        observed = dict(rows)
        # A fresh fixture may fail after tightening but before creating either
        # product service. Accept only exact not-created state, including rc4;
        # loaded services still require rc0. Task census below remains mandatory.
        failed_collector = (unit == 'consolecrypt-agent-collector.service' and result.returncode == 0
            and observed == dict(LoadState='loaded',ActiveState='failed',MainPID='0'))
        if failed_collector:
            need('lifecycle' in gate,'qualified collector terminal verifier missing')
            terminal=gate['lifecycle'].collector_terminal_state()
            need(terminal == dict(activeState='failed',mainPid=0,disabled=True,exactOwnedUnit=True,
                 cgroupAndDescendantsQuiescent=True,failurePreserved=True),'qualified failed collector proof required')
            report['collectorServiceTerminal'] = terminal
        else:
            need((result.returncode == 0 and observed == dict(LoadState='loaded',ActiveState='inactive',MainPID='0'))
                 or (result.returncode in (0,4) and observed == dict(LoadState='not-found',ActiveState='inactive',MainPID='0')),
                 'ARM teardown manager or workload live')
    need(not Path('/run/user/10002').exists() and not Path('/run/user/10002').is_symlink(),
         'owned user runtime directory still live')
    for group in (Path('/sys/fs/cgroup/system.slice/consolecrypt-agent.service'),
                  Path('/sys/fs/cgroup/system.slice/consolecrypt-agent-collector.service'),
                  Path('/sys/fs/cgroup/user.slice/user-10002.slice')):
        for path in group.rglob('cgroup.procs') if group.exists() else ():
            need(not custody.read(str(path),65536).strip(), 'ARM teardown processes remain')
    product,plan = gate.get('product'),gate.get('plan')
    absent = lambda path: not path.exists() and not path.is_symlink()
    try:
        if plan is not None:
            product.END = time.monotonic()+55
            # Materialization is owned only when the product's configured or
            # ready descriptor agrees with the exact plan captured beforehand.
            paths = (plan['managerPath'],product.ARM_COPY,product.ARM_POLICY,product.ARM_META)
            if any(not absent(path) for path in paths[1:]):
                owner = product.decode(product.root_file(product.OWNER,16384,0o600))
                need(owner.get('state') in ('configured','ready') and owner.get('uid') == owner.get('gid') == 10002
                     and owner.get('armManager') == plan['descriptor'], 'owned ARM teardown descriptor required')
            if not absent(plan['managerPath']):
                arm_gate_unlink(product,plan['managerPath'],plan['managerBytes'],0o644)
            command(['/usr/bin/systemctl','daemon-reload'],output/'cleanup-arm-reload.log',20)
            need(arm_gate_policy(product.ARM_VENDOR) == gate['policyBefore'], 'original manager policy not restored')
            report['originalUnitPolicyRestored'] = True
            if product.arm_profile_state(10002) is not None:
                need(product.arm_profile_state(10002) == 'ownedMode'
                     and product.root_file(product.ARM_POLICY,16384,0o644) == plan['profile'],
                     'owned ARM profile teardown mismatch')
                product.command(product.arm_parser_argv('remove'),seconds=15)
            need(product.arm_profile_state(10002) is None, 'owned ARM profile still loaded')
            for path,raw,mode,gid in ((product.ARM_POLICY,plan['profile'],0o644,0),
                (product.ARM_META,plan['public'],0o644,0),(product.ARM_COPY,plan['raw'],0o750,10002)):
                if not absent(path):arm_gate_unlink(product,path,raw,mode,gid)
        need(arm_gate_profiles(custody) == gate['profilesBefore'], 'host AppArmor profile set changed')
        report['temporaryProfileAbsent'] = True
        need(absent(Path('/etc/consolecrypt-agent/collector-systemd'))
             and absent(Path('/etc/apparmor.d/consolecrypt-agent-arm64-manager')),
             'temporary ARM manager copy or profile remains')
        report['temporaryEntrypointAbsent'] = True
    finally:
        state = gate['sysctl']
        try:
            if state['fd'] is not None:
                # Restoration is independent of copy removal errors, but only
                # after the own profile is absent and all manager tasks joined.
                own = gate['profileName']
                profile_absent = not any(row == own or row.startswith(own+' (')
                                         for row in arm_gate_profiles(custody))
                custody.restore_transition_sysctl(state,profile_absent,True)
                report['temporarySysctlRestored'] = state['restored']
                report['temporarySysctl'] = dict(original=int(state['original']),
                    recordedChange=state['recordedChange'],restored=state['restored'])
        finally:
            if state['fd'] is not None:
                os.close(state['fd']);state['fd'] = None


def arm_cleanup_failure(error,gate):
    # Fixed classes, codes and source roles only: never exception repr/path/data.
    known={'bounded stopped ARM unit observation required','complete stopped ARM unit observation required',
        'qualified collector terminal verifier missing','qualified failed collector proof required',
        'ARM teardown manager or workload live','owned user runtime directory still live',
        'ARM teardown processes remain','owned ARM teardown descriptor required',
        'owned ARM teardown file changed','owned ARM teardown absence required',
        'original manager policy not restored','owned ARM profile teardown mismatch',
        'owned ARM profile still loaded','host AppArmor profile set changed',
        'temporary ARM manager copy or profile remains','bounded actual manager policy required',
        'complete manager policy required','exact actual manager entrypoint required',
        'collector_provision_unavailable','collector_control_exit','collector_provision_deadline',
        'collector_control_output','collector_child_custody',
        'diag_sysctl_close',
        'diag_sysctl_exact_bytes',
        'diag_sysctl_identity_changed',
        'diag_sysctl_identity_missing',
        'diag_sysctl_not_procfs_device',
        'diag_sysctl_original_unknown',
        'diag_sysctl_restore_compare',
        'diag_sysctl_restore_readback',
        'diag_sysctl_restore_resources_live',
        'diag_sysctl_restore_short_write',
        'diag_sysctl_root_inode',
        'diag_sysctl_root_parent',
        'diag_sysctl_tighten_compare',
        'diag_sysctl_tighten_readback',
        'diag_sysctl_tighten_short_write',
        'diag_sysctl_unrecorded_state'}
    classes={'ValueError','FileNotFoundError','PermissionError','AssertionError','OSError',
        'TimeoutExpired','ProcessLookupError','RuntimeError','KeyError'}
    closed={arm_gate_retire.__code__:'arm_gate_retire',arm_gate_unlink.__code__:'arm_gate_unlink',
        arm_gate_policy.__code__:'arm_gate_policy',arm_gate_profiles.__code__:'arm_gate_profiles',need.__code__:'need'}
    for role,names in (('product',('need','root_file','parents','command','fields','arm_profile_state')),
                       ('custody',('need','read','restore_transition_sysctl','profiles',
                                   'sysctl_bytes','sysctl_guard','sysctl_path_identity'))):
        module=gate.get(role)
        for name in names:
            function=getattr(module,name,None)
            if hasattr(function,'__code__'):closed[function.__code__]=role+'.'+name
    def project(value):
        code=value.args[0]if type(value)is ValueError and len(value.args)==1 and type(value.args[0])is str and value.args[0]in known else'unclassified'
        frames=[];tb=value.__traceback__
        for _ in range(32):
            if tb is None:break
            if tb.tb_frame.f_code in closed:frames.append({'role':closed[tb.tb_frame.f_code],'line':tb.tb_lineno})
            tb=tb.tb_next
        return {'class':type(value).__name__ if type(value).__name__ in classes else'unclassified',
            'code':code,'frames':frames[-8:]}
    result={'readOnly':True,'stage':'armRestoration','error':project(error)}
    if error.__context__ is not None:result['priorContext']=project(error.__context__)
    return result


def execute(args):
    source,packet,normal,authority,phases,lifecycle_raw,primitives_raw = validate(args)
    # All input checks precede any host change. Never adopt existing product state.
    for path in (FIXTURE,Path('/var/lib/consolecrypt-agent'),Path('/etc/consolecrypt-agent'),
                 Path('/opt/consolecrypt-agent-install'),Path('/etc/consolecrypt-agent-release'),
                 Path('/usr/local/libexec/consolecrypt-agent-release'),OBS):
        need(not path.exists() and not path.is_symlink(), 'fresh disposable host product paths required')
    for lookup,key in ((pwd.getpwnam,'consolecrypt-agent'),(pwd.getpwuid,10002),
                       (grp.getgrnam,'consolecrypt-agent'),(grp.getgrgid,10002)):
        try:
            lookup(key)
        except KeyError:
            continue
        raise ValueError('existing service account name or UID/GID refused')
    for unit in ('consolecrypt-agent.service','consolecrypt-agent-collector.service'):
        shown = subprocess.run(['/usr/bin/systemctl','show','--property=LoadState','--value',unit],
            capture_output=True,timeout=10,check=False)
        need(len(shown.stdout)+len(shown.stderr) <= 4096 and shown.stdout.strip() == b'not-found',
             'existing product unit refused')
    initial_idle = arm_gate_initial_idle()
    need(ctypes.CDLL(None,use_errno=True).prctl(36,1,0,0,0) == 0, 'original command group subreaper required')
    output = args.output.absolute()
    output.mkdir(mode=0o700,parents=False,exist_ok=False)
    report = dict(passed=False,architecture='aarch64',packetSha256=args.packet_sha256,
        syntheticReleaseSignerOnly=True,productionReleaseClaimed=False,hostRebootObserved=False,
        installedAcceptanceExecuted=False,cleanupJoined=False,initialFixedUidIdle=initial_idle)
    owned_identity = None
    gate = dict(sysctl=dict(fd=None,identity=None,original=None,writeAttempted=False,
                            recordedChange=False,restored=False))
    def identity():
        user = pwd.getpwnam('consolecrypt-agent')
        group = grp.getgrnam('consolecrypt-agent')
        need(pwd.getpwuid(10002) == user and grp.getgrgid(10002) == group
             and (user.pw_uid,user.pw_gid,user.pw_dir,user.pw_shell) ==
                 (10002,10002,'/var/lib/consolecrypt-agent','/usr/sbin/nologin')
             and group.gr_gid == 10002 and group.gr_mem == [], 'exact dedicated identity readback required')
        return user,group
    try:
        # This disposable hosted image ships /opt root-owned0777. Prepare
        # this one measured host precondition before any product publication;
        # the shipping installer's complete root-ancestry policy stays intact.
        opt_path = Path('/opt')
        opt_root = Path('/').lstat()
        need(stat.S_ISDIR(opt_root.st_mode) and opt_root.st_uid == opt_root.st_gid == 0
             and not opt_root.st_mode & 0o022, 'controlled physical root ancestor required')
        opt_fd = os.open(opt_path,os.O_RDONLY|os.O_DIRECTORY|os.O_NOFOLLOW|os.O_CLOEXEC)
        try:
            opt_before = os.fstat(opt_fd)
            opt_identity = lambda m: (m.st_dev,m.st_ino,m.st_uid,m.st_gid,m.st_nlink,m.st_size,m.st_mtime_ns)
            need(stat.S_ISDIR(opt_before.st_mode) and opt_before.st_uid == opt_before.st_gid == 0
                 and stat.S_IMODE(opt_before.st_mode) in (0o755,0o777)
                 and opt_identity(opt_path.lstat()) == opt_identity(opt_before)
                 and opt_path.lstat().st_mode == opt_before.st_mode, 'exact measured disposable host parent required')
            if stat.S_IMODE(opt_before.st_mode) == 0o777:os.fchmod(opt_fd,0o755)
            opt_after = os.fstat(opt_fd)
            need(opt_identity(opt_after) == opt_identity(opt_before) == opt_identity(opt_path.lstat())
                 and stat.S_IMODE(opt_after.st_mode) == stat.S_IMODE(opt_path.lstat().st_mode) == 0o755,
                 'stable controlled disposable host parent required')
            report['hostTrustParent'] = dict(uid=opt_after.st_uid,gid=opt_after.st_gid,
                modeBefore=stat.S_IMODE(opt_before.st_mode),mode=stat.S_IMODE(opt_after.st_mode),
                changed=opt_before.st_mode != opt_after.st_mode,samePhysicalInode=True)
        finally:os.close(opt_fd)
        # The original fresh-state check preceded the measured0777 narrowing.
        # Recheck its two /opt children after their parent becomes controlled.
        for opt_child in (FIXTURE,Path('/opt/consolecrypt-agent-install')):
            need(not opt_child.exists() and not opt_child.is_symlink(), 'fresh controlled product paths required')
        command(['/usr/sbin/groupadd','--gid','10002','consolecrypt-agent'],output/'group.log',20)
        created_group = grp.getgrnam('consolecrypt-agent')
        need(created_group == grp.getgrgid(10002) and created_group.gr_gid == 10002
             and created_group.gr_mem == [], 'own newly created group readback required')
        command(['/usr/sbin/useradd','--uid','10002','--gid','10002','--no-create-home',
                 '--home-dir','/var/lib/consolecrypt-agent','--shell','/usr/sbin/nologin','consolecrypt-agent'],output/'user.log',20)
        created_identity = identity()
        need(created_identity[1] == created_group, 'own group changed during user creation')
        owned_identity = created_identity
        report['cleanupAuthorityAcquired'] = True
        arm_gate_prepare(gate)
        for path,mode in ((FIXTURE,0o755),(FIXTURE/'private',0o700),(OBS,0o700),
                          (Path('/var/lib/consolecrypt-agent'),0o755),(Path('/etc/consolecrypt-agent'),0o755),
                          (Path('/opt/consolecrypt-agent-install'),0o755),(Path('/etc/consolecrypt-agent-release'),0o755),
                          (Path('/usr/local/libexec/consolecrypt-agent-release'),0o755)):
            path.mkdir(mode=mode,parents=True,exist_ok=False)
        def publish(path,raw,mode):
            with path.open('xb') as stream:
                stream.write(raw)
            path.chmod(mode)
        publish(Path('/usr/local/libexec/consolecrypt-agent-release/cc-agent-identity'),normal,0o755)
        # Match the existing x86 fixture: root0700 prevents path access,
        # while the exact inherited FD may execute after the UID10002 drop.
        # Only this fresh exclusive private copy receives readonly0555.
        authority_path = FIXTURE/'private/synthetic_node_authority'
        publish(authority_path,authority,0o555)
        private_info = authority_path.parent.lstat()
        authority_info = authority_path.lstat()
        need(stat.S_ISDIR(private_info.st_mode) and private_info.st_uid == private_info.st_gid == 0
             and stat.S_IMODE(private_info.st_mode) == 0o700, 'fresh private root0700 authority directory required')
        need(stat.S_ISREG(authority_info.st_mode) and authority_info.st_nlink == 1
             and authority_info.st_uid == authority_info.st_gid == 0
             and authority_info.st_size == len(authority) and stat.S_IMODE(authority_info.st_mode) == 0o555,
             'fresh exact readonly root authority copy required')
        authority_identity = lambda m: (m.st_dev,m.st_ino,m.st_uid,m.st_gid,m.st_mode,m.st_nlink,
                              m.st_size,m.st_mtime_ns,m.st_ctime_ns)
        need(digest(read(authority_path,64*1024**2)) == digest(authority)
             and authority_identity(authority_path.lstat()) == authority_identity(authority_info), 'private authority copy changed')
        report['bootstrapOwner'] = dict(uid=authority_info.st_uid,gid=authority_info.st_gid,
            mode=stat.S_IMODE(authority_info.st_mode),links=authority_info.st_nlink,bytes=authority_info.st_size,
            privateUid=private_info.st_uid,privateGid=private_info.st_gid,privateMode=stat.S_IMODE(private_info.st_mode),
            childUid=10002,childGid=10002,mountNoexec=bool(os.statvfs(authority_path).f_flag & os.ST_NOEXEC))
        need(not report['bootstrapOwner']['mountNoexec'], 'private authority filesystem must allow execution')
        publish(FIXTURE/'expected-releases.json',json.dumps(dict(version=1,normalElfSha256=digest(normal),
            fixtureAuthoritySha256=digest(authority),phases=phases)).encode(),0o444)
        key = read(packet/'a/signed/release-public-key.json',4096)
        publish(Path('/etc/consolecrypt-agent-release/public-key.json'),key,0o444)
        publish(Path('/etc/consolecrypt-agent-release/trust.json'),json.dumps(dict(version=1,
            verifierSha256=digest(normal),publicKeySha256=digest(key),platform='linux-aarch64')).encode(),0o444)
        # Two finite architecture substitutions in the SAME lifecycle. Every
        # signed read, pidfd, mapped ELF, WAL, revoke, replay and removal check stays.
        text = lifecycle_raw.decode().replace('\r\n','\n')
        need(text.count('144399412') == 5 and text.count("'platform':'linux-x86_64'") == 1,
             'reviewed existing lifecycle adaptation anchors changed')
        text = text.replace('144399412',"expected_releases()['phases']['disk']['prometheusBytes']")
        text = text.replace("'platform':'linux-x86_64'","'platform':'linux-aarch64'")
        primitives = primitives_raw.decode()
        need(primitives.count('NODE_UID=10001') == 1, 'existing primitive UID anchor changed')
        publish(FIXTURE/'sqlite_fixture_flow.py',primitives.replace('NODE_UID=10001','NODE_UID=10002').encode(),0o444)
        publish(FIXTURE/'guest_lifecycle.py',text.encode(),0o444)
        sys.path.insert(0,str(FIXTURE))
        spec = importlib.util.spec_from_file_location('same_agent_arm_host_lifecycle',FIXTURE/'guest_lifecycle.py')
        lifecycle = importlib.util.module_from_spec(spec);spec.loader.exec_module(lifecycle)
        gate['lifecycle'] = lifecycle
        token = str(uuid.uuid4())
        publish(FIXTURE/'token',(token+'\n').encode(),0o444)
        lifecycle.token = lambda: token  # Host transport has no QEMU kernel argv.
        playbook = output/'playbook.yml'
        publish(playbook,b'---\n- hosts: all\n  gather_facts: true\n  gather_subset: [min]\n  become: false\n  roles: ["{{ acceptance_role }}"]\n',0o600)
        environment = dict(os.environ,ANSIBLE_ROLES_PATH=str(source/'delivery/roles'),ANSIBLE_NOCOLOR='1')
        sequence = 0
        def role(number,operation,start=False,collector=False):
            nonlocal sequence
            sequence += 1
            variables = dict(acceptance_role='consolecrypt_agent_collector_service' if collector else 'consolecrypt_agent_install')
            if collector:
                variables['cc_agent_collector_service_operation'] = operation
            else:
                base = packet/('a','b','disk')[number-1]
                variables.update(cc_agent_install_approved=True,cc_agent_install_operation=operation,
                    cc_agent_install_start=start,cc_agent_release_artifact=str(base/'candidate.zip'),
                    cc_agent_release_manifest=str(base/'signed/release-manifest.json'))
            command([args.ansible_python,'-I','-B','-m','ansible.cli.playbook','-i','localhost,','-c','local',
                playbook,'--extra-vars',json.dumps(variables)],output/('%02d-role.log'%sequence),240,environment)
        report['bootstrap'] = lifecycle.bootstrap()
        report['installedAcceptanceExecuted'] = True
        for phase,number,operation in (('install',1,'install'),('update',2,'update'),('rollback',2,'rollback')):
            role(number,operation,True)
            report[phase] = lifecycle.runtime(phase)
        report['diskStopA'] = lifecycle.disk_stop_a()
        role(3,'update')
        arm_gate_plan(gate,source)
        report['diskPrepare'] = lifecycle.disk_prepare()
        report['managerInitialStart'] = arm_gate_boot(gate)
        role(3,'prepare',collector=True)
        gate['terminalInputs'] = lifecycle.capture_collector_terminal_inputs()
        command(['/usr/bin/systemctl','restart',ARM_MANAGER],output/'manager-restart.log',60)
        report['managerRestart'] = arm_gate_boot(gate)
        need(report['managerRestart']['invocationId'] != report['managerInitialStart']['invocationId'],
             'actual fresh manager invocation required')
        report['disk'] = lifecycle.disk_service()
        role(3,'disable',collector=True)
        role(3,'remove')
        report['remove'] = lifecycle.removed()
        # Fresh module import after removal; same host, explicitly no reboot claim.
        reopened = importlib.util.module_from_spec(spec);spec.loader.exec_module(reopened)
        reopened.token = lambda: token
        reopened.TERMINAL_INPUTS = gate['terminalInputs']
        report['readOnlyReopen'] = reopened.removed()
        report['passed'] = True
    except BaseException as error:
        report['errorClass'] = type(error).__name__
        raise
    finally:
        try:
            if owned_identity is None:
                report['cleanupSkippedIdentityUnowned'] = True
            else:
                need(identity() == owned_identity, 'cleanup dedicated identity changed')
                for index,unit in enumerate(('consolecrypt-agent.service','consolecrypt-agent-collector.service','user@10002.service',
                                             'user-runtime-dir@10002.service')):
                    # Authority exists only after our successful create+readback.
                    shown = subprocess.run(['/usr/bin/systemctl','show','--property=LoadState','--value',unit],
                        capture_output=True,timeout=10,check=False)
                    need(len(shown.stdout)+len(shown.stderr) <= 4096, 'bounded cleanup unit observation')
                    if shown.stdout.strip() != b'not-found':
                        need(shown.returncode == 0 and shown.stdout.strip() == b'loaded', 'known unit required for cleanup')
                        if unit == 'consolecrypt-agent-collector.service':
                            need('terminalInputs' in gate and 'lifecycle' in gate,
                                 'original collector ownership frame unavailable')
                            report['collectorAdministrativeCleanup'] = gate['lifecycle'].retire_fixture_collector()
                            continue
                        preserve_failed = False
                        if unit == 'consolecrypt-agent-collector.service':
                            terminal = subprocess.run(['/usr/bin/systemctl','show',unit,
                                '--property=ActiveState','--property=MainPID'],capture_output=True,timeout=5,check=False)
                            need(terminal.returncode == 0 and len(terminal.stdout)+len(terminal.stderr)<=4096,
                                 'bounded collector cleanup terminal observation required')
                            preserve_failed = terminal.stdout in (b'ActiveState=failed\nMainPID=0\n',b'MainPID=0\nActiveState=failed\n')
                        if not preserve_failed:
                            command(['/usr/bin/systemctl','stop',unit],output/('cleanup-%d.log'%index),60)
                groups = [Path('/sys/fs/cgroup/system.slice')/unit for unit in
                          ('consolecrypt-agent.service','consolecrypt-agent-collector.service')]
                groups += [Path('/sys/fs/cgroup/user.slice/user-10002.slice')]
                for group in groups:
                    for path in group.rglob('cgroup.procs') if group.exists() else ():
                        need(not path.read_text().strip(), 'owned service processes remain')
                report['cleanupJoined'] = True
        except BaseException as cleanup:
            report['passed'] = False
            report['cleanupErrorClass'] = type(cleanup).__name__
            # Bounded static provenance only; preserve this original cleanup error.
            try:
                known = ('cleanup dedicated identity changed', 'bounded cleanup unit observation',
                         'known unit required for cleanup', 'existing role/host command failed',
                         'owned command process group not joined', 'command leader exited with descendants',
                         'bounded role log required', 'owned service processes remain')
                code = cleanup.args[0] if type(cleanup) is ValueError and len(cleanup.args) == 1 and type(cleanup.args[0]) is str and cleanup.args[0] in known else 'unclassified'
                frames = []
                closed = {execute.__code__: 'execute', command.__code__: 'command', need.__code__: 'need'}
                tb = cleanup.__traceback__
                while tb is not None:
                    if tb.tb_frame.f_code in closed:
                        frames.append(dict(function=closed[tb.tb_frame.f_code], line=tb.tb_lineno))
                    tb = tb.tb_next
                report['cleanupFailure'] = dict(code=code, frames=frames[-8:], readOnly=True)
            except BaseException:
                pass
        try:
            arm_gate_retire(gate,report,output)
        except BaseException as cleanup:
            report.update(passed=False,cleanupJoined=False,armCleanupErrorClass=type(cleanup).__name__)
            try:report['armCleanupFailure']=arm_cleanup_failure(cleanup,gate)
            except Exception:pass # Preserve the work error and original restoration failure.
        finally:
            if gate['sysctl']['fd'] is not None:
                os.close(gate['sysctl']['fd']);gate['sysctl']['fd'] = None
        (output/'RESULT.json').write_text(json.dumps(report,sort_keys=True,indent=2)+'\n')
    need(report['passed'] and report['cleanupJoined'], 'actual ARM lifecycle/cleanup did not pass')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--execute-disposable-arm-host',action='store_true',required=True)
    parser.add_argument('--exercise-owned-arm-manager',action='store_true',required=True)
    for name in ('agent','packet','runtime-pin','primitives','ansible-python','output',
                 'ansible-wheel-catalog','ansible-lock','ansible-install-report'):
        parser.add_argument('--'+name,type=Path,required=True)
    for name in ('packet-sha256','runtime-pin-sha256','lifecycle-sha256','primitives-sha256'):
        parser.add_argument('--'+name,required=True)
    execute(parser.parse_args())


if __name__ == '__main__':
    main()
