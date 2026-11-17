"""Fixed final Mac verification, invoked only after the maintained signed build.

No build, distribution, profile/password/keychain action, permission grant or
process-name retirement. Only original G2 and one fresh-data packaged GUI child.
"""
import argparse
import ast
import ctypes
import hashlib
import json
import math
import os
from pathlib import Path
import plistlib
import re
import shutil
import signal
import stat
import subprocess
import sys
import time
import zipfile

ABI=1141874734
VERSION='1.2.0+67027'
OWNER_SHA='ac754d623f2e52f240ad85144e40c8d6bf3a784b90a003a557b8099e9546dd55'
TEST_SHA='e1ea84cffae9cae7b09dbf5320853b760cec8d7a2eed701f9384d31e7ae640e4'
FRAMEWORK='Contents/Frameworks/cc_bridge.framework/Versions/A/cc_bridge'
HERE=Path(__file__).resolve().parent


FLUTTER_ENGINE='Contents/Frameworks/FlutterMacOS.framework/Versions/A/FlutterMacOS'
FLUTTER_ENGINE_SHA='9afb47e0fded849b1213ddc48f911b0520dca72467a368d1afd38caec3627e36'
FLUTTER_ENGINE_BYTES=30035744
_PACKAGED_HANDLES=[]
_STAGE='preflight'
_STAGES={'preflight','package_witness','framework_load_probe','packaged_flutter_load','native_framework_load','native_abi','original_g2','owned_gui'}


def set_stage(stage):
    global _STAGE
    need(stage in _STAGES,'closed_verification_stage')
    _STAGE=stage


def closed_error_facts(error):
    roles={'main','package_witness','preload_packaged_bridge','digest','read','need','command',
        'bundle_identity','bundle_version','own_dmg_device','find_owned_dmg','graph','bound_sources','gui','set_stage',
        'session_keys','parse_session_keys','session_flag_fact'}
    frames=[];trace=error.__traceback__;seen=0
    while trace is not None and seen<32:
        code=trace.tb_frame.f_code;seen+=1
        if code.co_filename==__file__ and code.co_name in roles:
            frames.append(dict(role=code.co_name,line=trace.tb_lineno))
        trace=trace.tb_next
    number=getattr(error,'errno',None)
    return dict(stage=_STAGE if _STAGE in _STAGES else 'invalid_stage',
        errno=number if type(number)is int and 0<=number<=4095 else None,
        frames=frames[-12:],rawMessageOrPathsRetained=False)


def preload_packaged_bridge(app,framework,output):
    engine=app/FLUTTER_ENGINE
    metadata=engine.lstat()
    need(engine.resolve(strict=True)==engine and stat.S_ISREG(metadata.st_mode)
         and metadata.st_nlink==1 and not metadata.st_mode&0o6000
         and metadata.st_size==FLUTTER_ENGINE_BYTES,'owned_packaged_flutter_input')
    need(digest(engine,64*1024**2)==FLUTTER_ENGINE_SHA,'owned_packaged_flutter_digest')
    set_stage('framework_load_probe')
    probe=dict(actualBeforePreloadProbe=True,loadedBeforePreload=False,
        missingPackagedFlutterFamily=False,errno=None,rawMessageOrPathsRetained=False)
    try:
        before=ctypes.CDLL(str(framework))
    except OSError as error:
        message=str(error)
        probe['missingPackagedFlutterFamily']='Library not loaded: @rpath/FlutterMacOS.framework/Versions/A/FlutterMacOS'in message
        number=getattr(error,'errno',None)
        probe['errno']=number if type(number)is int and 0<=number<=4095 else None
        probe['failureFacts']=closed_error_facts(error)
    else:
        _PACKAGED_HANDLES.append(before);probe['loadedBeforePreload']=True
    save(output/'framework-load-probe.json',probe)
    need(probe['loadedBeforePreload']or probe['missingPackagedFlutterFamily'],'unclassified_bridge_load_probe')
    set_stage('packaged_flutter_load')
    engine_handle=ctypes.CDLL(str(engine),mode=ctypes.RTLD_GLOBAL)
    _PACKAGED_HANDLES.append(engine_handle)
    need(digest(engine,64*1024**2)==FLUTTER_ENGINE_SHA,'packaged_flutter_changed_after_load')
    save(output/'packaged-flutter-host.json',dict(bytes=FLUTTER_ENGINE_BYTES,sha256=FLUTTER_ENGINE_SHA,
        exactOwnedSignedPackagedInput=True,handleRetained=True,loaderSettingsChanged=False))
    set_stage('native_framework_load')
    library=ctypes.CDLL(str(framework));_PACKAGED_HANDLES.append(library)
    return library


def need(value,code):
    if not value:raise ValueError(code)


def read(path,cap):
    path=Path(path)
    with path.open('rb')as stream:
        before=os.fstat(stream.fileno())
        stamp=lambda m:(m.st_dev,m.st_ino,m.st_mode,m.st_size,m.st_mtime_ns,m.st_ctime_ns)
        need(stat.S_ISREG(before.st_mode)and 0<before.st_size<=cap,'bounded_regular_input')
        raw=stream.read(cap+1)
        need(len(raw)==before.st_size and stamp(before)==stamp(os.fstat(stream.fileno()))==stamp(path.stat()),'stable_input')
    return raw


def digest(path,cap=4*1024**3):
    with path.open('rb')as stream:
        before=os.fstat(stream.fileno())
        need(stat.S_ISREG(before.st_mode)and 0<before.st_size<=cap,'bounded_artifact')
        value=hashlib.file_digest(stream,'sha256').hexdigest()
        stamp=lambda m:(m.st_dev,m.st_ino,m.st_size,m.st_mtime_ns,m.st_ctime_ns)
        need(stamp(before)==stamp(os.fstat(stream.fileno()))==stamp(path.stat()),'artifact_changed')
    return value


def save(path,value):
    with path.open('x',encoding='utf-8')as stream:stream.write(json.dumps(value,indent=2)+'\n')


def command(argv,output,label,seconds=30,cap=1024**2):
    timed_out=False
    try:
        result=subprocess.run([str(v)for v in argv],stdin=subprocess.DEVNULL,capture_output=True,
            timeout=seconds,check=False,env=dict(os.environ,LANG='C',LC_ALL='C'))
    except subprocess.TimeoutExpired as error:
        timed_out=True
        result=subprocess.CompletedProcess(argv,-1,error.stdout or b'',error.stderr or b'')
    truncated=len(result.stdout)+len(result.stderr)>cap
    for suffix,raw in (('.stdout',result.stdout),('.stderr',result.stderr)):
        with(output/(label+suffix)).open('xb')as stream:stream.write(raw[:cap//2]if truncated else raw)
    save(output/(label+'.tool.json'),dict(returnCode=result.returncode,timedOut=timed_out,
        outputTruncated=truncated,stdoutBytes=len(result.stdout),stderrBytes=len(result.stderr)))
    need(not timed_out,'fixed_tool_timeout_'+label)
    need(not truncated,'bounded_tool_output')
    need(result.returncode==0,'fixed_tool_failed_'+label)
    return result.stdout


def bundle_identity(info):
    need(info['CFBundleIdentifier']=='io.consolecrypt.consolecrypt','production_bundle_identity')
    need(str(info['CFBundleShortVersionString'])+'+'+str(info['CFBundleVersion'])==VERSION,'packaged_version')
    executable=info['CFBundleExecutable']
    need(type(executable)is str and re.fullmatch(r'[A-Za-z0-9_.-]{1,80}',executable),'fixed_bundle_executable')
    return executable


def bundle_version(app):return bundle_identity(plistlib.loads(read(app/'Contents/Info.plist',1024**2)))


def own_dmg_device(raw,mount):
    value=plistlib.loads(raw)
    rows=value.get('system-entities',[])
    matches=[row.get('dev-entry')for row in rows if row.get('mount-point')==str(mount)]
    need(len(matches)==1 and type(matches[0])is str and re.fullmatch(r'/dev/disk[0-9]+(?:s[0-9]+)*',matches[0]),'owned_dmg_mount')
    return matches[0]


def find_owned_dmg(output,dmg,mount):
    # Global hdiutil inventory is read only for this exact image/mount selector.
    # Never persist raw global stdout/stderr or another image's metadata.
    record=dict(imagePath=str(dmg),mountPath=str(mount),querySucceeded=False,device=None)
    try:
        try:
            result=subprocess.run(['/usr/bin/hdiutil','info','-plist'],stdin=subprocess.DEVNULL,
                capture_output=True,timeout=10,check=False)
        except subprocess.TimeoutExpired:
            record['timedOut']=True
            raise ValueError('owned_dmg_inventory_timeout')
        record['returnCode']=result.returncode
        need(result.returncode==0,'owned_dmg_inventory_failed')
        need(len(result.stdout)+len(result.stderr)<=1024**2,'owned_dmg_inventory_bound')
        images=plistlib.loads(result.stdout).get('images',[])
        need(type(images)is list and len(images)<=1024,'owned_dmg_image_count')
        matches=[image for image in images if image.get('image-path')==str(dmg)
            and any(row.get('mount-point')==str(mount)for row in image.get('system-entities',[]))]
        need(len(matches)<=1,'unique_owned_dmg_image')
        device=own_dmg_device(plistlib.dumps(matches[0]),mount)if matches else None
        record.update(querySucceeded=True,device=device)
        return device
    finally:save(output/'dmg-info-owned.json',record)


def package_witness(root,output):
    app=root/'dist/macos/ConsoleCrypt.app';archive=root/'dist/macos/ConsoleCrypt-macos.zip'
    dmg=root/'dist/macos/ConsoleCrypt.dmg';framework=app/FRAMEWORK
    need(read(root/'dist/macos/ConsoleCrypt.version',128).decode().strip()==VERSION,'dist_version')
    executable=bundle_version(app)
    framework_sha=digest(framework,512*1024**2)
    zip_sha=digest(archive);dmg_sha=digest(dmg)
    save(output/'packaged-inputs.json',dict(version=VERSION,frameworkSha256=framework_sha,
        zipSha256=zip_sha,dmgSha256=dmg_sha,threeMemberEqualityNotYetVerified=True))
    command(['/usr/bin/codesign','--verify','--deep','--strict',app],output,'codesign',45)
    arches=command(['/usr/bin/lipo','-archs',framework],output,'framework-arches',10).decode().split()
    need(len(arches)==2 and set(arches)=={'arm64','x86_64'},'universal_framework')
    with zipfile.ZipFile(archive)as package:
        members=package.infolist();need(len(members)<=50000,'zip_member_bound')
        need(sum(row.file_size for row in members)<=8*1024**3,'zip_expanded_bound')
        name='ConsoleCrypt.app/'+FRAMEWORK
        rows=[row for row in members if row.filename==name]
        need(len(rows)==1 and rows[0].file_size==framework.stat().st_size
             and rows[0].file_size<=512*1024**2,'exact_zip_framework')
        with package.open(rows[0])as stream:zip_framework_sha=hashlib.file_digest(stream,'sha256').hexdigest()
        need(zip_framework_sha==framework_sha,'zip_framework_digest')
        info_rows=[row for row in members if row.filename=='ConsoleCrypt.app/Contents/Info.plist']
        need(len(info_rows)==1 and 0<info_rows[0].file_size<=1024**2,'exact_zip_bundle_version_member')
        need(bundle_identity(plistlib.loads(package.read(info_rows[0])))==executable,'zip_bundle_identity')
    mount=output/'dmg-mount';mount.mkdir(mode=0o700)
    device=None;unmounted=False
    try:
        attached=command(['/usr/bin/hdiutil','attach','-readonly','-nobrowse','-noautoopen',
            '-mountpoint',mount,'-plist',dmg],output,'dmg-attach',45)
        device=own_dmg_device(attached,mount)
        need(bundle_version(mount/'ConsoleCrypt.app')==executable,'dmg_bundle_identity')
        dmg_framework_sha=digest(mount/'ConsoleCrypt.app'/FRAMEWORK,512*1024**2)
        need(dmg_framework_sha==framework_sha,'dmg_framework_digest')
    finally:
        if device is None:
            # A partially failed attach may still own this fresh mountpoint.
            # Query only; never detach by device name guessed from another job.
            device=find_owned_dmg(output,dmg,mount)
        if device is not None:
            command(['/usr/bin/hdiutil','detach',device],output,'dmg-detach',30)
            unmounted=True
        else:need(not os.path.ismount(mount),'unknown_dmg_mount_not_detached')
        need(not os.path.ismount(mount),'owned_dmg_still_mounted')
        mount.rmdir()
    need(unmounted,'actual_dmg_mount_and_join')
    need(digest(framework,512*1024**2)==framework_sha and digest(archive)==zip_sha and digest(dmg)==dmg_sha,'artifacts_changed_after_inspection')
    library=preload_packaged_bridge(app,framework,output)
    set_stage('native_abi')
    getter=library.frb_get_rust_content_hash;getter.argtypes=[];getter.restype=ctypes.c_int32
    need(getter()==ABI,'actual_framework_abi')
    witness=dict(version=VERSION,abi=ABI,frameworkSha256=framework_sha,
        zipFrameworkSha256=zip_framework_sha,dmgFrameworkSha256=dmg_framework_sha,
        zipSha256=zip_sha,dmgSha256=dmg_sha,frameworkArchitectures=arches,
        actualHostArchitecture=os.uname().machine,otherUniversalSliceExecuted=False,
        signedAppVerified=True,ownedDmgUnmounted=True,executable=executable)
    save(output/'packaged-witness.json',witness)
    return app,framework,witness


def bound_sources(owner,test,framework_sha):
    need(hashlib.sha256(owner).hexdigest()==OWNER_SHA and hashlib.sha256(test).hexdigest()==TEST_SHA,'reviewed_g2_source_pins')
    owner_guard=b'APPROVED_SHA = None  # Bind only after the actual final application build.'
    test_guard=b'const String? approvedDllSha256 = null;'
    need(owner.count(owner_guard)==test.count(test_guard)==1,'exact_two_null_guards')
    need(re.fullmatch('[0-9a-f]{64}',framework_sha)is not None,'actual_framework_sha')
    actual=framework_sha.encode()
    bound_owner=owner.replace(owner_guard,b"APPROVED_SHA = '"+actual+b"'  # Bind only after the actual final application build.")
    bound_test=test.replace(test_guard,b"const String? approvedDllSha256 = '"+actual+b"';")
    tree=ast.parse(bound_owner)
    for node in tree.body:
        if isinstance(node,ast.Assign)and any(isinstance(t,ast.Name)and t.id=='APPROVED_SHA'for t in node.targets):
            node.value=ast.Constant(value=None)
    need(ast.dump(tree)==ast.dump(ast.parse(owner)),'owner_changed_beyond_exact_binding')
    return bound_owner,bound_test


def graph(root,framework,witness,output):
    source_owner=read(HERE/'run_current_graph.py',65536);source_test=read(HERE/'current_graph_test.dart',65536)
    owner,test=bound_sources(source_owner,source_test,witness['frameworkSha256'])
    directory=output/'bound-g2';directory.mkdir(mode=0o700)
    (directory/'run_current_graph.py').write_bytes(owner);(directory/'current_graph_test.dart').write_bytes(test)
    need((directory/'current_graph_test.dart').parent==directory and directory.resolve(strict=True)==directory,'owned_private_test_config_parent')
    bootstrap=b"import 'dart:async';\nimport 'dart:convert';\nimport 'dart:ffi';\nimport 'dart:io';\nimport 'package:crypto/crypto.dart';\n\nfinal List<DynamicLibrary> _ownedPackagedLibraries = <DynamicLibrary>[];\n\nFuture<void> testExecutable(FutureOr<void> Function() testMain) async {\n  final nativePath = Platform.environment['CC_OWNED_MONITORING_DLL'];\n  if (!Platform.isMacOS || nativePath == null || File(nativePath).resolveSymbolicLinksSync() != nativePath) {\n    throw StateError('owned_native_bootstrap_input');\n  }\n  final frameworks = File(nativePath).parent.parent.parent.parent;\n  final engine = File('${frameworks.path}/FlutterMacOS.framework/Versions/A/FlutterMacOS');\n  final before = await engine.stat();\n  if (engine.resolveSymbolicLinksSync() != engine.path ||\n      await FileSystemEntity.type(engine.path, followLinks: false) != FileSystemEntityType.file ||\n      before.size != 30035744 || (before.mode & 0xC00) != 0) {\n    throw StateError('owned_packaged_flutter_bootstrap_input');\n  }\n  final hash = (await sha256.bind(engine.openRead()).first).toString();\n  final after = await engine.stat();\n  if (hash != '9afb47e0fded849b1213ddc48f911b0520dca72467a368d1afd38caec3627e36' ||\n      before.size != after.size || before.modified != after.modified || before.changed != after.changed ||\n      before.mode != after.mode || engine.resolveSymbolicLinksSync() != engine.path) {\n    throw StateError('owned_packaged_flutter_bootstrap_digest');\n  }\n  _ownedPackagedLibraries.add(DynamicLibrary.open(engine.path));\n  print('OWNED_PACKAGED_FLUTTER_TEST_BOOTSTRAP ${jsonEncode(<String, Object>{\n    'engineSha256': hash, 'engineBytes': before.size, 'actualEngineLoaded': true, 'ownedHandleRetained': true,\n  })}');\n  await testMain();\n}\n"
    (directory/'flutter_test_config.dart').write_bytes(bootstrap)
    runtime=output/'g2-runtime';runtime.mkdir(mode=0o700)
    environment=dict(os.environ,RUNNER_TEMP=str(runtime))
    with(output/'g2-owner.log').open('xb')as log:
        result=subprocess.run([sys.executable,'-I','-B',str(directory/'run_current_graph.py'),
            '--root',str(root),'--dll',str(framework),'--expected-sha',witness['frameworkSha256'],'--attempt','1'],
            stdin=subprocess.DEVNULL,stdout=log,stderr=subprocess.STDOUT,check=False,env=environment)
        # The original owner owns its 210s test +15s cleanup deadline. Do not
        # kill that owner from under its retained child/process-group custody.
        # The existing CI step separately bounds this entire verification.
    receipt=runtime/'monitoring-final-macos-packaged-graph-ffi-attempt1-receipt.json'
    actual=json.loads(read(receipt,65536))
    need(actual['nativeSha256']==witness['frameworkSha256']and actual['abi']==ABI,'actual_g2_binding')
    need(actual['ownedProcessGroupJoined']and not actual['remainingOwnedFixtures'],'original_g2_cleanup')
    need(digest(HERE/'run_current_graph.py',65536)==OWNER_SHA and digest(HERE/'current_graph_test.dart',65536)==TEST_SHA,'shipping_g2_sources_modified')
    log=runtime/'monitoring-final-macos-packaged-graph-ffi-attempt1.log'
    marker='OWNED_PACKAGED_FLUTTER_TEST_BOOTSTRAP '
    bootstraps=[json.loads(line.split(marker,1)[1])for line in read(log,8*1024**2).decode('utf-8','replace').splitlines()if marker in line]
    need(len(bootstraps)<=1,'single_owned_flutter_test_bootstrap')
    child_host=bootstraps[0]if bootstraps else None
    if child_host is not None:
        need(child_host==dict(engineSha256=FLUTTER_ENGINE_SHA,engineBytes=FLUTTER_ENGINE_BYTES,
            actualEngineLoaded=True,ownedHandleRetained=True),'actual_child_packaged_flutter_host')
    record=dict(passed=result.returncode==0 and actual['exitCode']==0 and child_host is not None,originalG2Receipt=actual,
        privateTestBootstrapSha256=hashlib.sha256(bootstrap).hexdigest(),privateTestBootstrapOwnedParent=True,
        actualChildPackagedFlutterHost=child_host,
        sourceOwnerSha256=OWNER_SHA,sourceTestSha256=TEST_SHA,
        boundOwnerSha256=hashlib.sha256(owner).hexdigest(),boundTestSha256=hashlib.sha256(test).hexdigest(),
        onlyTwoPrivateCopyNullGuardsBound=True,shippingGuardsRemainNull=True)
    save(output/'g2-verification.json',record)
    return record


PINNED_SESSION_KEYS={'kCGSessionOnConsoleKey':'kCGSSessionOnConsoleKey',
    'kCGSessionLoginDoneKey':'kCGSessionLoginDoneKey','kCGSessionUserIDKey':'kCGSSessionUserIDKey'}

def parse_session_keys(raw):
    need(type(raw)is bytes and 0<len(raw)<=65536,'bounded_session_sdk_header')
    text=raw.decode('utf-8','strict');result={}
    for symbol in ('kCGSessionOnConsoleKey','kCGSessionLoginDoneKey','kCGSessionUserIDKey'):
        pattern=r'(?m)^[\t ]*#[\t ]*define[\t ]+'+re.escape(symbol)+r'[\t ]+CFSTR\([\t ]*"([A-Za-z0-9_]{1,96})"[\t ]*\)'
        matches=re.findall(pattern,text)
        need(len(matches)==1,'exact_session_sdk_cfstr_'+symbol)
        result[symbol]=matches[0]
    need(len(set(result.values()))==3,'distinct_session_sdk_keys')
    need(result==PINNED_SESSION_KEYS,'pinned_session_sdk_cfstr_map')
    return result


def session_keys(output=None):
    facts=dict(publicConstantDataOnly=True,compiledOrExecuted=False,rawPathsOrNumericUidRetained=False,
        selectedSdkUnderDeveloper=False,headerUnderSdk=False,metadata=[],pinnedMapMatched=False)
    try:
        result=subprocess.run(['/usr/bin/xcrun','--sdk','macosx','--show-sdk-path'],stdin=subprocess.DEVNULL,
            capture_output=True,check=False,timeout=10)
        need(result.returncode==0 and 0<len(result.stdout)<=4096 and len(result.stderr)<=4096,'trusted_session_sdk_query')
        sdk=Path(result.stdout.decode('utf-8','strict').strip()).resolve(strict=True)
        developer=Path('/Applications/Xcode.app/Contents/Developer').resolve(strict=True)
        facts['selectedSdkUnderDeveloper']=sdk.is_relative_to(developer)
        need(facts['selectedSdkUnderDeveloper'],'trusted_session_sdk_root')
        header=(sdk/'System/Library/Frameworks/CoreGraphics.framework/Headers/CGSession.h').resolve(strict=True)
        facts['headerUnderSdk']=header.is_relative_to(sdk)
        need(facts['headerUnderSdk'],'owned_session_sdk_header_path')
        for number,path in enumerate((header,*header.parents)):
            value=path.stat()
            facts['metadata'].append(dict(role='file'if number==0 else 'ancestor'+str(number),
                ownerRole='root'if value.st_uid==0 else 'current'if value.st_uid==os.getuid()else 'other',
                mode07777=value.st_mode&0o7777,groupWrite=bool(value.st_mode&0o020),worldWrite=bool(value.st_mode&0o002)))
            need(number<32,'bounded_session_sdk_ancestry')
            if path==sdk:break
        raw=read(header,65536)
        keys=parse_session_keys(raw);header_sha=hashlib.sha256(raw).hexdigest()
        facts.update(pinnedMapMatched=True,headerSha256=header_sha,keyNames=keys)
        return keys,header_sha
    finally:
        if output is not None:save(output/'session-sdk-facts.json',facts)


def session_flag_fact(session,key):
    if key not in session:return dict(present=False,kind='missing',value=None)
    value=session[key]
    if type(value)is bool:return dict(present=True,kind='bool',value=value)
    if type(value)is int and value in (0,1):return dict(present=True,kind='int01',value=bool(value))
    return dict(present=True,kind='other',value=None)


class MacWindows:
    def __init__(self):
        void=ctypes.c_void_p
        self.cf=ctypes.CDLL('/System/Library/Frameworks/CoreFoundation.framework/CoreFoundation')
        self.cg=ctypes.CDLL('/System/Library/Frameworks/CoreGraphics.framework/CoreGraphics')
        self.cf.CFPropertyListCreateData.argtypes=[void,void,ctypes.c_long,ctypes.c_ulong,void]
        self.cf.CFPropertyListCreateData.restype=void
        self.cf.CFDataGetLength.argtypes=[void];self.cf.CFDataGetLength.restype=ctypes.c_long
        self.cf.CFDataGetBytePtr.argtypes=[void];self.cf.CFDataGetBytePtr.restype=void
        self.cf.CFRelease.argtypes=[void];self.cf.CFRelease.restype=None
        self.cg.CGSessionCopyCurrentDictionary.argtypes=[];self.cg.CGSessionCopyCurrentDictionary.restype=void
        self.cg.CGWindowListCopyWindowInfo.argtypes=[ctypes.c_uint32,ctypes.c_uint32]
        self.cg.CGWindowListCopyWindowInfo.restype=void
        self.cg.CGPreflightScreenCaptureAccess.argtypes=[];self.cg.CGPreflightScreenCaptureAccess.restype=ctypes.c_bool
        self.proc=ctypes.CDLL('/usr/lib/libproc.dylib')
        self.proc.proc_pidpath.argtypes=[ctypes.c_int,void,ctypes.c_uint32];self.proc.proc_pidpath.restype=ctypes.c_int
    def value(self,reference):
        need(reference,'graphical_session_unavailable')
        data=None
        try:
            data=self.cf.CFPropertyListCreateData(None,reference,100,0,None)
            need(data,'window_metadata_property_list')
            length=self.cf.CFDataGetLength(data);need(0<length<=4*1024**2,'window_metadata_bound')
            return plistlib.loads(ctypes.string_at(self.cf.CFDataGetBytePtr(data),length))
        finally:
            if data:self.cf.CFRelease(data)
            self.cf.CFRelease(reference)
    def session(self):return self.value(self.cg.CGSessionCopyCurrentDictionary())
    def windows(self,pid):
        rows=self.value(self.cg.CGWindowListCopyWindowInfo(17,0))
        need(type(rows)is list and len(rows)<=8192,'window_count_bound')
        result=[]
        for row in rows:
            if row.get('kCGWindowOwnerPID')!=pid or row.get('kCGWindowLayer')!=0:continue
            bounds=row.get('kCGWindowBounds',{})
            width=bounds.get('Width',0);height=bounds.get('Height',0)
            if not all(isinstance(x,(int,float))and math.isfinite(x)for x in (width,height)):continue
            if width<200 or height<150 or row.get('kCGWindowAlpha',0)<=0:continue
            number=row.get('kCGWindowNumber')
            if type(number)is int and 0<number<2**32:
                result.append(dict(windowId=number,ownerPid=pid,width=width,height=height,layer=0))
        return result
    def image(self,pid):
        data=ctypes.create_string_buffer(4096)
        count=self.proc.proc_pidpath(pid,data,len(data));need(0<count<len(data),'owned_gui_image_path')
        return Path(data.value.decode('utf-8','strict')).resolve(strict=True)


def child_status(child):return os.waitid(os.P_PID,child.pid,os.WEXITED|os.WNOHANG|os.WNOWAIT)


def join_gui(child):
    # Retain this direct child's PID until the final wait, so signals cannot
    # target a reused PID. No launchservices/open or process-name/group kill.
    end=time.monotonic()+15
    if child_status(child)is None:os.kill(child.pid,signal.SIGTERM)
    while child_status(child)is None and time.monotonic()<end-3:time.sleep(.05)
    if child_status(child)is None:os.kill(child.pid,signal.SIGKILL)
    return child.wait(timeout=max(.01,end-time.monotonic()))


def gui(app,witness,output):
    report=dict(passed=False,actualAppLaunched=False,actualOwnedPidJoined=False,
        emptyOnboardingVisualReviewRequired=True,profileOrPasswordActions=False,
        keychainActionsByVerifier=False,localAiSmokeUsedAsGuiProof=False)
    child=None;fresh=None
    try:
        api=MacWindows()
        capture=bool(api.cg.CGPreflightScreenCaptureAccess());report['screenCaptureAlreadyAuthorized']=capture
        keys,header_sha=session_keys(output);session=api.session()
        on_console=session_flag_fact(session,keys['kCGSessionOnConsoleKey'])
        login_done=session_flag_fact(session,keys['kCGSessionLoginDoneKey'])
        uid_key=keys['kCGSessionUserIDKey'];uid=session.get(uid_key)
        current_uid_matches=type(uid)is int and uid==os.getuid()
        report['sessionPreflight']=dict(sdkHeaderSha256=header_sha,keyNames=keys,onConsole=on_console,
            loginDone=login_done,userIdPresent=uid_key in session,userIdExactInteger=type(uid)is int,
            currentUidMatches=current_uid_matches)
        need(on_console['value']is True and login_done['value']is True
             and current_uid_matches,'no_current_graphical_login_session')
        report['graphicalSessionAvailable']=True
        need(capture,'screen_capture_permission_absent_no_prompt_or_grant')
        fresh=output/'fresh-gui';fresh.mkdir(mode=0o700)
        for name in ('home','tmp','data','xdg-cache','xdg-config','xdg-data'):(fresh/name).mkdir(mode=0o700)
        executable=(app/'Contents/MacOS'/witness['executable']).resolve(strict=True)
        need(not executable.stat().st_mode&0o6000,'normal_packaged_gui_executable')
        image_sha=digest(executable,256*1024**2)
        environment={key:value for key,value in os.environ.items()if not key.startswith(('DYLD_','LD_','CONSOLECRYPT_','CC_OWNED_'))}
        environment.update(HOME=str(fresh/'home'),TMPDIR=str(fresh/'tmp')+'/',
            CONSOLECRYPT_DATA_DIR=str(fresh/'data'),XDG_CACHE_HOME=str(fresh/'xdg-cache'),
            XDG_CONFIG_HOME=str(fresh/'xdg-config'),XDG_DATA_HOME=str(fresh/'xdg-data'),
            CI='true',FLUTTER_SUPPRESS_ANALYTICS='true',DART_SUPPRESS_ANALYTICS='true')
        with(output/'gui-startup.log').open('xb')as log:
            child=subprocess.Popen([str(executable)],cwd=str(fresh/'home'),env=environment,
                stdin=subprocess.DEVNULL,stdout=log,stderr=subprocess.STDOUT,close_fds=True,start_new_session=True)
            report.update(actualAppLaunched=True,pid=child.pid,imagePath=str(executable),imageSha256=image_sha)
            until=time.monotonic()+35;window=None;first_seen=None
            while time.monotonic()<until:
                need(child_status(child)is None,'packaged_gui_exited_before_window')
                need(api.image(child.pid)==executable,'packaged_gui_pid_image_changed')
                windows=api.windows(child.pid)
                if len(windows)==1:
                    if window is None or window['windowId']!=windows[0]['windowId']:
                        window=windows[0];first_seen=time.monotonic()
                    elif time.monotonic()-first_seen>=5:break
                else:window=None;first_seen=None
                time.sleep(.2)
            need(window is not None and first_seen is not None and time.monotonic()-first_seen>=5,'owned_gui_window_not_observed')
            report['window']=window
            png=output/'gui-empty-onboarding.png'
            command(['/usr/sbin/screencapture','-x','-l',str(window['windowId']),png],output,'gui-window-capture',10)
            raw=read(png,16*1024**2);need(raw[:8]==b'\x89PNG\r\n\x1a\n'and raw[12:16]==b'IHDR','owned_window_png')
            need(child_status(child)is None and api.image(child.pid)==executable
                 and any(row['windowId']==window['windowId']for row in api.windows(child.pid)),'owned_gui_binding_after_capture')
            need(digest(executable,256*1024**2)==image_sha,'packaged_gui_image_changed')
            report.update(passed=True,pngSha256=hashlib.sha256(raw).hexdigest(),
                isolatedFreshDataWindowCaptured=True,emptyOnboardingVisuallyConfirmed=False)
    except BaseException as error:
        report.update(passed=False,errorKind=type(error).__name__,errorCode=str(error)[:160]if isinstance(error,ValueError)else'gui_verification_failed')
        report['errorFacts']=closed_error_facts(error)
    finally:
        if child is not None:
            try:report.update(actualExitCode=join_gui(child),actualOwnedPidJoined=True)
            except BaseException as error:report.update(passed=False,joinErrorKind=type(error).__name__)
        if fresh is not None and report['actualOwnedPidJoined']:
            try:
                need(fresh.parent==output and fresh.resolve(strict=True)==fresh
                     and fresh.stat().st_uid==os.getuid(),'owned_gui_cleanup_root')
                shutil.rmtree(fresh)
                report['ownedFreshDirectoriesRemoved']=not fresh.exists()and not fresh.is_symlink()
                need(report['ownedFreshDirectoriesRemoved'],'owned_gui_directories_remain')
            except BaseException as error:report.update(passed=False,cleanupErrorKind=type(error).__name__)
        save(output/'gui-verification.json',report)
    return report


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--execute-final-macos-verification',action='store_true',required=True)
    parser.add_argument('--root',type=Path,required=True);parser.add_argument('--output',type=Path,required=True)
    args=parser.parse_args()
    need(sys.platform=='darwin'and sys.version_info>=(3,13)and hasattr(os,'waitid')and hasattr(os,'WNOWAIT'),'macos_python313_required')
    need(os.getuid()==os.geteuid()!=0,'unprivileged_host_account')
    root=args.root.resolve(strict=True);output=args.output.absolute();temporary=Path(os.environ['RUNNER_TEMP']).resolve(strict=True)
    need(root==Path(os.environ['GITHUB_WORKSPACE']).resolve(strict=True),'actual_ci_source_root')
    need(output.parent==temporary and not output.exists()and not output.is_symlink(),'fresh_owned_verification_output')
    os.umask(0o077);output.mkdir(mode=0o700)
    report=dict(passed=False,actualMacVerification=True,version=VERSION,appBuildPerformedByVerifier=False)
    try:
        source=command(['git','rev-parse','HEAD'],output,'source-head',10).decode().strip()
        need(source==os.environ['GITHUB_SHA'],'frozen_ci_source_commit')
        original=command(['git','show',source+':client/flutter/pubspec.yaml'],output,'source-floor',10).decode()
        need(re.search(r'^version:\s*1\.2\.0\+67026\s*$',original,re.M),'frozen_pre_bump_source_floor')
        generated=read(root/'client/flutter/lib/src/rust/frb_generated.dart',8*1024**2).decode()
        need('rustContentHash => 1141874734;'in generated,'current_generated_abi')
        need(digest(HERE/'run_current_graph.py',65536)==OWNER_SHA
             and digest(HERE/'current_graph_test.dart',65536)==TEST_SHA,'reviewed_g2_sources_before_framework_load')
        set_stage('package_witness')
        app,framework,witness=package_witness(root,output);report['packaged']=witness
        set_stage('original_g2')
        g2=graph(root,framework,witness,output);report['g2Passed']=g2['passed']
        set_stage('owned_gui')
        actual_gui=gui(app,witness,output);report['guiObserved']=actual_gui['passed']
        report['emptyOnboardingVisualReviewRequired']=True
        report['passed']=(g2['passed']and actual_gui['passed']and actual_gui['actualOwnedPidJoined']
            and actual_gui.get('ownedFreshDirectoriesRemoved')is True)
    except BaseException as error:
        report.update(errorKind=type(error).__name__,errorCode=str(error)[:160]if isinstance(error,ValueError)else'final_mac_verification_failed')
        report['errorFacts']=closed_error_facts(error)
    finally:save(output/'RESULT.json',report)
    return 0 if report['passed']else 1


if __name__=='__main__':raise SystemExit(main())
