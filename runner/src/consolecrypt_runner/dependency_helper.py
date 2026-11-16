"""Explicit Linux dependency-only control plane. Never a job/executor peer."""
from concurrent.futures import ThreadPoolExecutor
import hashlib
import os
from pathlib import Path
import stat
import sys
import threading
import uuid

from . import VERSION, PROTOCOL_VERSION
from .errors import RunnerError, require
from .protocol import read_message, validate_request, uuid_field, write_frame
from .revisions import RevisionRepository
from .upload import UploadManager
from .dependency_prepare import DependencyPreparations
from .store import atomic_json
from . import dependency_retention as retention

NAMESPACE = '.consolecrypt-dependency-operations'
READ = frozenset(('files', 'read_file', 'read_chunk'))
UPLOAD = frozenset(('upload_begin', 'upload_entry', 'upload_chunk', 'upload_commit', 'upload_abort'))
DEPENDENCY = frozenset(('dependency_prepare', 'dependency_archive_info', 'dependency_archive_chunk',
                        'dependency_files', 'dependency_chunk', 'dependency_status'))
ALLOWED = READ | UPLOAD | DEPENDENCY | {'probe', 'revision_release', 'dependency_dispose'}


def operation_path(configured_root, operation_id, *, _purpose='dependency_prepare'):
    """Same deterministic derivation required of the private Native factory."""
    require(_purpose in ('dependency_prepare','vault_validation'),'private_purpose','Unsupported private purpose')
    namespace=NAMESPACE if _purpose=='dependency_prepare' else '.consolecrypt-validation-operations'
    value = os.fspath(configured_root)
    require(isinstance(value, str) and 0 < len(value.encode('utf-8')) <= 4096
            and value.startswith('/') and not value.endswith('/')
            and not any(ord(c) < 32 or ord(c) == 127 for c in value)
            and all(part not in ('', '.', '..') for part in value.split('/')[1:]),
            'dependency_root', 'Invalid configured Runner root')
    uuid_field(operation_id, 'operation ID')
    require(uuid.UUID(operation_id).int != 0, 'dependency_root', 'Invalid operation ID')
    root = Path(value)
    family = hashlib.sha256(root.name.encode('utf-8')).hexdigest()
    return root.parent / namespace / family / operation_id


def _open_directory(parent, name, *, create=False):
    if create:
        try:
            os.mkdir(name, 0o700, dir_fd=parent)
            os.fsync(parent)
        except FileExistsError:
            pass
    descriptor = os.open(name, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC,
                         dir_fd=parent)
    if create:
        info = os.fstat(descriptor)
        if not (info.st_uid == os.getuid() and stat.S_IMODE(info.st_mode) == 0o700):
            os.close(descriptor)
            raise RunnerError('dependency_root', 'Dependency namespace must be owned and private')
    return descriptor


class DependencyHelper:
    def __init__(self, configured_root, operation_id, *, _purpose='dependency_prepare'):
        require(sys.platform == 'linux' and os.getuid() != 0,
                'dependency_backend', 'Dependency mode requires a non-root Linux account')
        destination = operation_path(configured_root, operation_id,_purpose=_purpose)
        self._purpose=_purpose
        self._namespace=destination.parent.parent.name
        self.operation_id = operation_id
        self.root_family = destination.parent.name
        self._fd = None
        self._family_fd = None
        self._identity = self._data_identity = None
        self._workers_drained = False
        self._dispose_attempted = self._disposed = False
        self.uploads = None
        self._prepare_started = False
        self._prepare_lock = threading.Lock()
        # No JobStore, stale-job recovery, upload glob cleanup, executor or
        # target channel is created. Existing normal Runner roots are untouched.
        directory = os.open('/', os.O_RDONLY | os.O_DIRECTORY | os.O_CLOEXEC)
        try:
            parts = destination.parts[1:]
            for index, part in enumerate(parts[:-1]):
                if index == len(parts)-3:
                    # Inspect only the configured jobs-root entry, never enter,
                    # create, chmod, recover or delete any of its contents.
                    try:
                        configured = os.stat(Path(configured_root).name, dir_fd=directory,
                                             follow_symlinks=False)
                    except FileNotFoundError:
                        pass
                    else:
                        require(stat.S_ISDIR(configured.st_mode)
                                and configured.st_uid == os.getuid()
                                and stat.S_IMODE(configured.st_mode) == 0o700,
                                'dependency_root', 'Configured root must be an owned private directory')
                child = _open_directory(directory, part, create=index >= len(parts)-3)
                os.close(directory)
                directory = child
            # Native UUID is exclusive: crashes/previous operation directories
            # are retained for review, never reopened, removed or repaired.
            self._family_fd = os.dup(directory)
            self._fd = retention.claim(directory, parts[-1])
            self._identity = retention.identity(self._fd)
            info = os.fstat(self._fd)
            require(info.st_uid == os.getuid() and stat.S_IMODE(info.st_mode) == 0o700,
                    'dependency_root', 'Dependency operation root must be private')
            # All internal writes remain bound to this owned inode if any
            # ambient ancestor is renamed. No portable/path fallback exists.
            anchor = Path('/proc/self/fd') / str(self._fd)
            anchored = anchor.stat()
            require((anchored.st_dev, anchored.st_ino) == (info.st_dev, info.st_ino),
                    'dependency_backend', 'Linux descriptor storage is unavailable')
            # A real final component is needed by shared atomic_json's
            # O_NOFOLLOW parent open; never weaken that existing protection.
            os.mkdir('data', 0o700, dir_fd=self._fd)
            data = os.stat('data', dir_fd=self._fd, follow_symlinks=False)
            self._data_identity = (data.st_dev, data.st_ino)
            self.root = anchor/'data'
            atomic_json(self.root/'operation.json', {'version':1, 'mode':self._purpose,
                                                    'operation_id':operation_id})
            self.revisions = RevisionRepository(self.root/'revisions', max_revisions=2,
                                                max_bytes=200*1024*1024)
            self.uploads = UploadManager(self.root/'uploads', self.root/'revisions', repository=self.revisions)
            self.dependencies = DependencyPreparations(self.revisions, self.root/'dependencies') if _purpose=='dependency_prepare' else None
        except BaseException:
            self.close()
            raise
        finally:
            os.close(directory)

    def _close_uploads(self):
        uploads, self.uploads = self.uploads, None
        if uploads is not None:
            uploads.close()

    def _dispose(self):
        require(self._workers_drained and not self._dispose_attempted,
                'dependency_cleanup_busy', 'Drain the private operation before disposal')
        self._dispose_attempted = True  # No retry of partial deletion/recovery.
        result = retention.dispose(self._family_fd, self.operation_id, self._fd,
                                   self._identity, self._data_identity, self._close_uploads)
        self._disposed = True
        return result

    def close(self, *, dispose=False):
        try:
            if dispose and not self._dispose_attempted:
                self._dispose()
            elif not self._dispose_attempted:
                self._close_uploads()
        finally:
            if self._fd is not None:
                os.close(self._fd)
                self._fd = None
            if self._family_fd is not None:
                os.close(self._family_fd)
                self._family_fd = None

    def revision(self, revision_id):
        return self.revisions.read(revision_id)

    def request(self, operation, body, cancel):
        require(operation in ALLOWED, 'unsupported_operation', 'Operation is unavailable in dependency mode')
        if operation == 'dependency_dispose':
            require(set(body) == {'operation_id', 'root_family'} and body['operation_id'] == self.operation_id
                    and body['root_family'] == self.root_family, 'dependency_cleanup_identity',
                    'Dispose only the current Native operation')
            return self._dispose()
        if operation == 'probe':
            require(not body, 'request_fields', 'Probe takes no arguments')
            return {'version':1, 'mode':self._purpose, 'mode_version':1,
                    'helper':VERSION, 'protocol':PROTOCOL_VERSION,
                    'root_namespace':self._namespace, 'root_family':self.root_family,
                    'operation_id':self.operation_id, 'execution_enabled':False,
                    'target_dispatch_enabled':False, 'network_policy':'reviewed_exact_https_worker' if self._purpose=='dependency_prepare' else 'none'}
        if operation in DEPENDENCY:
            if operation == 'dependency_prepare':
                with self._prepare_lock:
                    require(not self._prepare_started, 'dependency_once', 'Prepare requires a fresh operation')
                    self._prepare_started = True
            return self.dependencies.request(operation, body, cancel)
        if operation == 'revision_release':
            require(set(body) == {'revision_id'}, 'revision_fields', 'Invalid revision release')
            return self.revisions.release(body['revision_id'], referenced=lambda *_: False)
        if operation in UPLOAD:
            return self.uploads.request(operation, body)
        # Reuse actual fixed snapshot readers, never a second walker/uploader.
        # No forbidden operation reaches the normal helper dispatcher.
        from .__main__ import Helper
        return Helper.request(self, operation, body, cancel)


def serve_dependency(helper, source, output):
    """Strict private frames; bounded queue, cancellation, no raw target frames."""
    output_lock, pending_lock = threading.Lock(), threading.Lock()
    pending, seen = {}, set()
    dispose_request = None
    dispose_operation=getattr(helper,'dispose_operation','dependency_dispose')
    def control(frame):
        with output_lock:write_frame(output,frame)
    if getattr(helper,'private_control',None) is not None:
        helper.private_control.attach_sender(control)

    def send(identity, result=None, error=None):
        with output_lock:
            write_frame(output, {'version':1, 'type':'response', 'request_id':identity,
                                 'result':result, 'error':error})

    def execute(request):
        identity = request['request_id']
        try:
            with pending_lock:
                cancel = pending[identity]
            result = helper.request(request['operation'], request['body'], cancel)
            send(identity, result=result)
        except RunnerError as error:
            send(identity, error={'code':error.code, 'message':'Dependency request rejected'})
        except Exception:
            send(identity, error={'code':'helper_error', 'message':'Dependency request failed'})
        finally:
            with pending_lock:
                pending.pop(identity, None)

    with ThreadPoolExecutor(max_workers=2, thread_name_prefix='cc-dependency') as workers:
        try:
            while message := read_message(source):
                if getattr(helper,'private_control',None) is not None and message.get('type') in ('vault_validation_response','vault_job_response'):
                    helper.private_control.deliver(message)
                    continue
                # Includes target_response/revoke, which must never be routed.
                request = validate_request(message)
                identity = request['request_id']
                require(identity not in seen and len(seen) < 10000,
                        'request_replay', 'Duplicate request or session limit reached')
                seen.add(identity)
                if request['operation'] == dispose_operation:
                    # Stop reading/new admission before cancelling and joining
                    # every worker. Only the main control loop may dispose.
                    dispose_request = request
                    break
                if request['operation'] == 'cancel_request':
                    require(set(request['body']) == {'request_id'}, 'request_fields', 'Invalid cancellation')
                    target = uuid_field(request['body']['request_id'], 'cancelled request ID')
                    with pending_lock:
                        cancellation = pending.get(target)
                        if cancellation is not None:
                            cancellation.set()
                    send(identity, result={'cancellation_requested':cancellation is not None})
                    continue
                with pending_lock:
                    require(len(pending) < 8, 'request_queue', 'Dependency request queue is full')
                    pending[identity] = threading.Event()
                workers.submit(execute, request)
        finally:
            with pending_lock:
                for cancel in pending.values():
                    cancel.set()

    # ThreadPoolExecutor has joined every current worker, including cancellation
    # and trusted worker process-group completion. No guest work remains active.
    helper._workers_drained = True
    if dispose_request is not None:
        try:
            result = helper.request(dispose_operation, dispose_request['body'], threading.Event())
            send(dispose_request['request_id'], result=result)
        except (RunnerError, OSError) as error:
            send(dispose_request['request_id'], error={'code':getattr(error, 'code', 'dependency_cleanup_failed'),
                                                      'message':'Private operation cleanup requires review'})
        # Invalid explicit requests do not turn into implicit disposal or retry.
        return False
    return True
