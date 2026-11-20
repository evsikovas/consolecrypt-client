"""Bounded private stdio endpoint. No network listener or implicit installation."""
from __future__ import annotations

import argparse
import base64
import hashlib
from concurrent.futures import ThreadPoolExecutor
from dataclasses import asdict
from pathlib import Path
import os
import stat
import shutil
import sys
import threading
import uuid

from .errors import RunnerError, require
from .probe import probe
from .protocol import encode, read_message, read_request, validate_request, uuid_field, write_frame
from .snapshot import create_snapshot, relative_path, ProjectSnapshot
from .store import JobStore, read_json, _private_directory
from .validation import validate_project
from .upload import UploadManager
from .inventory import StaticInventory
from .target_channel import TargetChannel
from .execution import InteractiveExecutor
from .revisions import RevisionRepository
from .parameters import validate_parameters
from .validation_reports import write_report
from .git_import import capture_git
from .inventory_sources import capture_inventory,inventory_chunk
from .inventory_artifacts import InventoryUploads,read_artifact


class Helper:
    def __init__(self, root: Path, *, _dependency_preparation=False, _service_store=None):
        require(sys.platform != 'win32', 'unsupported_backend', 'Use a Unix helper, WSL2 or a remote runner')
        require(os.getuid() != 0, 'root_runner', 'Use a dedicated non-root runner account')
        self.root = root.absolute()
        _private_directory(self.root)
        _private_directory(self.root / 'revisions')
        if _service_store is None:
            require(not (self.root/'service-version.json').exists() and not (self.root/'service-version.json').is_symlink(),
                'service_root', 'Use the original enrolled service runtime for this root')
            self.store = JobStore(self.root / 'state')
            self.store.recover_interactive()
        else:
            require(isinstance(_service_store, JobStore) and _service_store.root == self.root/'state',
                'service_scope', 'Original service store required')
            self.store = _service_store
        # The state owner lock prevents another helper using this root. Stale
        # incomplete transfer directories were never accepted as revisions.
        for directory in self.root.glob('uploads-*'):
            uuid_field(directory.name.removeprefix('uploads-'), 'transfer session ID')
            _private_directory(directory)
            shutil.rmtree(directory)
        self.revisions = RevisionRepository(self.root / 'revisions')
        # Internal constructor gate only. No CLI flag or advertised capability
        # until native source approval, independent download and commit exist.
        self.dependency_preparations = None
        if _dependency_preparation:
            from .dependency_prepare import DependencyPreparations
            self.dependency_preparations = DependencyPreparations(self.revisions, self.root/'dependencies')
        self.inventory_uploads=InventoryUploads(self.revisions)
        self.uploads = UploadManager(self.root / ('uploads-' + str(uuid.uuid4())), self.root / 'revisions', repository=self.revisions)
        self.executor = None

    def close(self):
        self.inventory_uploads.close()
        if self.executor is not None:
            self.executor.close()
        self.uploads.close()
        self.store.close()

    def revision(self, revision_id):
        return self.revisions.read(revision_id)

    def request(self, operation, body, cancel):
        if operation.startswith('dependency_'):
            require(self.dependency_preparations is not None, 'unsupported_operation',
                    'Dependency preparation requires the native integration gate')
            return self.dependency_preparations.request(operation, body, cancel)
        if operation.startswith('inventory_upload_'):
            return self.inventory_uploads.request(operation,body,cancel)
        if operation=='inventory_capture':
            require(set(body)=={'revision_id','sources'},'inventory_fields','Invalid inventory source preview')
            return capture_inventory(self.revisions,body['revision_id'],body['sources'],cancel)
        if operation=='inventory_chunk':
            require(set(body)=={'revision_id','key','offset','limit'},'inventory_fields','Invalid inventory preview transfer')
            return inventory_chunk(self.revisions,body['revision_id'],body['key'],body['offset'],body['limit'])
        if operation=='git_capture':
            require(set(body) in ({'url','reference'},{'url','reference','ca_pem'}),'git_fields','Invalid Git import request')
            return capture_git(self.revisions,body['url'],body['reference'],cancel,ca_pem=body.get('ca_pem'))
        if operation=='read_chunk':
            require(set(body)=={'revision_id','path','offset','limit'} and type(body['offset']) is int and body['offset']>=0
                    and type(body['limit']) is int and 1<=body['limit']<=128*1024,'file_chunk','Invalid snapshot chunk request')
            root,snapshot=self.revision(body['revision_id'])
            path=relative_path(body['path'])
            entry=next((item for item in snapshot.files if item['path']==str(path)),None)
            require(entry is not None and body['offset']<=entry['bytes'],'file_chunk','Snapshot file or offset is invalid')
            # Resolve every ancestor relative to an owned FD without following
            # links. Native receiver independently hashes the completed file/tree.
            directory=os.open(root,os.O_RDONLY|os.O_DIRECTORY|os.O_NOFOLLOW)
            try:
                for part in path.parts[:-1]:
                    child=os.open(part,os.O_RDONLY|os.O_DIRECTORY|os.O_NOFOLLOW,dir_fd=directory)
                    os.close(directory);directory=child
                fd=os.open(path.parts[-1],os.O_RDONLY|os.O_NOFOLLOW|os.O_NONBLOCK,dir_fd=directory)
            finally:os.close(directory)
            with os.fdopen(fd,'rb') as source:
                info=os.fstat(source.fileno())
                require(stat.S_ISREG(info.st_mode) and info.st_nlink==1 and info.st_size==entry['bytes'],
                        'file_changed','Snapshot file differs from captured tree')
                source.seek(body['offset']);data=source.read(body['limit'])
            return {'revision_digest':snapshot.digest,'path':str(path),'offset':body['offset'],
                    'data':base64.b64encode(data).decode(),'eof':body['offset']+len(data)==entry['bytes']}
        if operation == 'configure_interactive':
            return self.executor.configure(body)
        if operation == 'configure_manual_owner':
            return self.executor.configure_manual_owner(body)
        if operation == 'accept_job':
            with self.revisions.lock:
                return self.executor.accept(body)
        if operation == 'revision_list':
            require(not body, 'revision_fields', 'Invalid revision query')
            return {'revisions': self.revisions.list()}
        if operation == 'revision_release':
            require(set(body) == {'revision_id'}, 'revision_fields', 'Invalid revision release')
            return self.revisions.release(body['revision_id'], referenced=self.store.references_revision)
        if operation in ('job_get', 'job_cancel', 'job_events'):
            require(set(body) == ({'job_id', 'after', 'limit'} if operation == 'job_events' else {'job_id'}),
                    'job_fields', 'Invalid job query')
            self.executor.check_owner(body['job_id'])
            if operation == 'job_cancel': return self.executor.cancel(body['job_id'])
            if operation == 'job_get':
                result = dict(self.store.get(body['job_id']))
                # Optional DATA only, after SAME existing owner check. Corrupt
                # audit metadata cannot change the observed job outcome.
                try:
                    audit = self.store.admission_audit(body['job_id'])
                    if audit is None:
                        recorded = {'version': 1, 'status': 'unrecorded'}
                    elif len(audit['targets']) <= 512 and len(encode(audit)) <= 65536:
                        recorded = {'version': 1, 'status': 'recorded', 'record': audit}
                    else:
                        recorded = {'version': 1, 'status': 'unavailable'}
                except (OSError, RunnerError, ValueError):
                    recorded = {'version': 1, 'status': 'unavailable'}
                result['admission_audit'] = recorded
                return result
            return self.store.events(body['job_id'], after=body['after'], limit=body['limit'])
        if operation == 'vault_state':
            require(set(body) in ({'locked'}, {'locked', 'generation'}), 'vault_state', 'Invalid vault state request')
            return self.executor.vault_state(body['locked'], body.get('generation'))
        if operation.startswith('upload_'):
            require(operation in ('upload_begin', 'upload_entry', 'upload_chunk', 'upload_abort', 'upload_commit'),
                    'unsupported_operation', 'Unsupported transfer operation')
            return self.uploads.request(operation, body)
        if operation == 'probe':
            require(not body, 'request_fields', 'Probe takes no arguments')
            result = probe()
            result['capabilities']['interactive_prepared'] = self.executor is not None and self.executor.identity is not None
            return result
        if operation == 'capture':
            require(set(body) == {'source'} and isinstance(body['source'], str) and
                    0 < len(body['source']) <= 4096 and '\x00' not in body['source'],
                    'source_path', 'Invalid project source path')
            return self.revisions.capture(Path(body['source']))
        if operation == 'files':
            require(set(body) == {'revision_id', 'offset', 'limit'} and type(body['offset']) is int and body['offset'] >= 0 and
                    type(body['limit']) is int and 1 <= body['limit'] <= 100, 'file_page', 'Invalid project file page')
            _, snapshot = self.revision(body['revision_id'])
            entries = snapshot.entries[body['offset']:body['offset'] + body['limit']]
            return {'entries': entries, 'total': len(snapshot.entries)}
        if operation == 'read_file':
            require(set(body) == {'revision_id', 'path'}, 'request_fields', 'Invalid project file request')
            root, snapshot = self.revision(body['revision_id'])
            path = relative_path(body['path'])
            entry = next((item for item in snapshot.files if item['path'] == str(path)), None)
            require(entry is not None and entry['bytes'] <= 256 * 1024, 'file_preview', 'File is absent or too large for text preview')
            fd = os.open(root.joinpath(*path.parts), os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
            with os.fdopen(fd, 'rb') as source:
                info = os.fstat(source.fileno())
                require(stat.S_ISREG(info.st_mode) and info.st_nlink == 1 and info.st_size <= 256 * 1024,
                        'file_changed', 'Invalid snapshot file')
                data = source.read(256 * 1024 + 1)
            require(len(data) == entry['bytes'] and hashlib.sha256(data).hexdigest() == entry['sha256'],
                    'file_changed', 'Snapshot changed since capture')
            try:
                text = data.decode('utf-8')
            except UnicodeError as error:
                raise RunnerError('file_encoding', 'File is not UTF-8 text') from error
            return {'path': str(path), 'text': text, 'revision_digest': snapshot.digest}
        if operation in ('validate', 'prepare_run'):
            expected = {'revision_id', 'playbook'} if operation == 'validate' else {'revision_id', 'playbook', 'parameters', 'inventory', 'targets'}
            referenced=operation=='prepare_run' and set(body)==expected-{'inventory'}|{'inventory_ref'}
            require(set(body) == expected or referenced, 'request_fields', 'Invalid validation request')
            root, snapshot = self.revision(body['revision_id'])
            options = {} if operation == 'validate' else {'parameters': body['parameters'],
                'inventory': read_artifact(self.revisions,body['revision_id'],body['inventory_ref'])[0] if referenced
                    else StaticInventory.from_snapshot(body['inventory']), 'targets': body['targets']}
            if operation=='prepare_run':
                checked=validate_parameters(body['parameters'])
                if 'vault' in checked:
                    from .vault_sidecar import PinnedVaultSidecar
                    binding=checked['vault']
                    from .vault_ephemeral import reference_matches
                    require(reference_matches(snapshot,binding['reference_sha256']),
                            'vault_binding','Vault reference differs from the frozen revision')
                    options['vault_sidecar_factory']=lambda phase:PinnedVaultSidecar(self.root,binding,phase,snapshot.digest)
            report = validate_project(root, snapshot, body['playbook'], cancel=cancel, **options)
            if operation=='prepare_run' and 'vault' in checked:
                # Do not persist raw potentially decrypted YAML diagnostics.
                # Native independently applies its ORIGINAL sensitivity marker
                # before any result/AI projection; this is disk minimization.
                from dataclasses import replace
                report=replace(report,checks=tuple(dict(check,output='Vault-sensitive diagnostics withheld') for check in report.checks))
            write_report(root.parent, report.to_json())
            result = report.to_json()
            if operation == 'prepare_run':
                # These private response-only witnesses let the native client
                # compare parsed data AND the exact bytes hashed by Python.
                # Python/Rust JSON exponent formatting need not be identical.
                # Do not persist witnesses alongside the validation report.
                result['input_witnesses'] = {
                    'parameters': {'digest': report.parameters_digest,
                        'canonical_json': encode(validate_parameters(body['parameters'])).decode('utf-8')},
                    'inventory': {'digest': report.inventory_digest,
                        'canonical_json': encode(options['inventory'].to_snapshot()).decode('utf-8')},
                }
                if referenced:
                    result['input_witnesses']['inventory']={'digest':report.inventory_digest,'reference':body['inventory_ref']}
            return result
        raise RunnerError('unsupported_operation', 'Operation is not available in this helper')


def serve(helper, source, output, *, _executor_factory=None):
    output_lock, pending_lock = threading.Lock(), threading.Lock()
    pending = {}
    seen = set()

    def control(frame):
        with output_lock:
            write_frame(output, frame)

    targets = TargetChannel(control)
    helper.executor = InteractiveExecutor(helper, targets, helper.store) if _executor_factory is None else _executor_factory(targets)

    def send(request_id, result=None, error=None):
        frame = {'version': 1, 'type': 'response', 'request_id': request_id, 'result': result, 'error': error}
        with output_lock:
            write_frame(output, frame)

    def execute(request):
        request_id = request['request_id']
        try:
            with pending_lock:
                cancel = pending[request_id]
            result = helper.request(request['operation'], request['body'], cancel)
            send(request_id, result=result)
        except RunnerError as error:
            send(request_id, error={'code': error.code, 'message': str(error)})
        except Exception:
            # Project-controlled values and exception text are never dumped to
            # diagnostic stdout or accidentally exposed in protocol errors.
            send(request_id, error={'code': 'helper_error', 'message': 'The helper could not complete the request'})
        finally:
            with pending_lock:
                pending.pop(request_id, None)

    with ThreadPoolExecutor(max_workers=2, thread_name_prefix='cc-runner') as workers:
        try:
            while message := read_message(source):
                if message.get('type') == 'target_response':
                    targets.deliver(message)
                    continue
                request = validate_request(message)
                request_id = request['request_id']
                require(request_id not in seen and len(seen) < 10000, 'request_replay', 'Duplicate request ID or session limit reached')
                seen.add(request_id)
                if request['operation'] in ('job_cancel', 'vault_state'):
                    # Cancellation cannot wait behind a long validation request.
                    try:
                        send(request_id, result=helper.request(request['operation'], request['body'], threading.Event()))
                    except RunnerError as error:
                        send(request_id, error={'code': error.code, 'message': str(error)})
                    continue
                if request['operation'] == 'cancel_request':
                    body = request['body']
                    require(set(body) == {'request_id'}, 'request_fields', 'Invalid cancellation request')
                    target = uuid_field(body['request_id'], 'cancelled request ID')
                    with pending_lock:
                        cancellation = pending.get(target)
                        if cancellation is not None:
                            cancellation.set()
                    send(request_id, result={'cancellation_requested': cancellation is not None})
                    continue
                with pending_lock:
                    require(len(pending) < 64, 'request_queue', 'Helper request queue is full')
                    pending[request_id] = threading.Event()
                workers.submit(execute, request)
        finally:
            with pending_lock:
                for cancellation in pending.values():
                    cancellation.set()
            targets.close()
            helper.executor.close()
    helper.uploads.close()
    helper.store.close()


def main():
    parser = argparse.ArgumentParser(description='ConsoleCrypt scoped automation helper')
    parser.add_argument('--stdio', action='store_true')
    parser.add_argument('--service-stdio', action='store_true')
    parser.add_argument('--dependency-stdio', action='store_true')
    parser.add_argument('--validation-stdio', action='store_true')
    parser.add_argument('--operation-id')
    parser.add_argument('--root', type=Path)
    parser.add_argument('--probe', action='store_true')
    parser.add_argument('--tcp-stdio', action='store_true')
    parser.add_argument('--host')
    parser.add_argument('--port', type=int)
    args = parser.parse_args()
    try:
        require(args.dependency_stdio or args.validation_stdio or args.operation_id is None,
                'helper_arguments', 'Operation ID requires explicit dependency mode')
        if args.service_stdio:
            require(not args.stdio and not args.dependency_stdio and not args.validation_stdio and not args.probe
                and not args.tcp_stdio and args.operation_id is None and args.host is None and args.port is None
                and args.root is not None and args.root.is_absolute(), 'helper_arguments','Invalid private service arguments')
            from .service.entry import serve as serve_service_entry
            serve_service_entry(args.root,sys.stdin.buffer,sys.stdout.buffer)
        elif args.dependency_stdio or args.validation_stdio:
            require(not(args.dependency_stdio and args.validation_stdio) and not args.stdio and not args.probe and not args.tcp_stdio
                    and args.host is None and args.port is None and args.root is not None
                    and args.operation_id is not None,
                    'helper_arguments', 'Invalid dependency helper arguments')
            from .dependency_helper import DependencyHelper, serve_dependency
            try:
                if args.validation_stdio:
                    from .vault_validation_helper import ValidationHelper
                    helper=ValidationHelper(args.root,args.operation_id)
                else:helper = DependencyHelper(args.root, args.operation_id)
            except (RunnerError, OSError):
                request = read_request(sys.stdin.buffer)
                if request is not None:
                    write_frame(sys.stdout.buffer, {'version':1, 'type':'response',
                        'request_id':request['request_id'], 'result':None,
                        'error':{'code':'dependency_root', 'message':'Dependency root is unavailable'}})
                raise SystemExit(65)
            normal_eof = False
            try:
                normal_eof = serve_dependency(helper, sys.stdin.buffer, sys.stdout.buffer)
            finally:
                # Normal EOF/dispose drains workers first. Protocol failure or
                # a crash retains evidence; no startup sweep/recovery occurs.
                helper.close(dispose=normal_eof)
        elif args.tcp_stdio:
            require(not args.stdio and not args.probe and args.root is None, 'helper_arguments', 'Invalid network origin mode')
            from .network import bridge_stdio
            try:
                bridge_stdio(args.host, args.port)
            except OSError:
                raise SystemExit(69)
        elif args.probe:
            write_frame(sys.stdout.buffer, probe())
        else:
            require(args.stdio and args.root is not None and args.root.is_absolute(),
                    'helper_arguments', 'A private absolute helper root and --stdio are required')
            try:
                helper = Helper(args.root)
            except (RunnerError, OSError) as error:
                # Return a bounded error for the first request instead of an
                # unexplained EOF. Never echo the path or raw OS exception.
                request = read_request(sys.stdin.buffer)
                if request is not None:
                    code = error.code if isinstance(error, RunnerError) else 'runner_storage_unavailable'
                    write_frame(sys.stdout.buffer, {'version': 1, 'type': 'response',
                        'request_id': request['request_id'], 'result': None,
                        'error': {'code': code, 'message': 'Runner state initialization failed'}})
                raise SystemExit(65)
            try:
                serve(helper, sys.stdin.buffer, sys.stdout.buffer)
            finally:
                helper.close()
    except RunnerError:
        # A malformed/control-channel request terminates this channel; no data
        # from the offending frame is logged, and running validations cancel.
        raise SystemExit(65)


if __name__ == '__main__':
    main()
