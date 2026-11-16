"""No SSH keys, credential resolver or outside network inside the guest."""
import base64
import os
import socket
import uuid

from ansible.errors import AnsibleConnectionFailure
from ansible.plugins.connection import ConnectionBase

from consolecrypt_runner.protocol import MAX_FRAME_BYTES, decode, encode

DOCUMENTATION = r'''
name: consolecrypt_rpc
short_description: Job-scoped native ConsoleCrypt SSH transport
description:
  - Forwards operations through the job-only Unix socket to a native verified target scope.
author: ConsoleCrypt
options:
  target_id:
    description: Approved ConsoleCrypt host identity. Changing it cannot expand the native scope.
    required: true
    vars:
      - name: consolecrypt_host_id
'''


class Connection(ConnectionBase):
    transport = 'consolecrypt_rpc'
    has_pipelining = False

    def _connect(self):
        self._connected = True
        return self

    def close(self):
        self._connected = False

    def _rpc(self, operation, body):
        host = self.get_option('target_id')
        try:
            if str(uuid.UUID(host)) != host:
                raise ValueError('noncanonical ID')
            raw = encode({'version': 1, 'host_id': host, 'operation': operation, 'body': body}) + b'\n'
            if len(raw) > MAX_FRAME_BYTES:
                raise ValueError('frame limit')
            with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as connection:
                connection.settimeout(3615)
                connection.connect('/run/consolecrypt/broker.sock')
                connection.sendall(raw)
                with connection.makefile('rb') as incoming:
                    response = incoming.readline(MAX_FRAME_BYTES + 1)
            if len(response) > MAX_FRAME_BYTES or not response.endswith(b'\n'):
                raise ValueError('response limit')
            result = decode(response)
            if not isinstance(result, dict) or set(result) != {'version', 'result', 'error'} or result['version'] != 1:
                raise ValueError('response shape')
            if result['error']:
                raise AnsibleConnectionFailure('ConsoleCrypt denied or lost the scoped operation; remote changes may be partial')
            return result['result']
        except (OSError, ValueError, TypeError, KeyError) as error:
            raise AnsibleConnectionFailure('ConsoleCrypt job transport is unavailable or invalid') from error

    def exec_command(self, cmd, in_data=None, sudoable=True):
        super().exec_command(cmd, in_data=in_data, sudoable=sudoable)
        data = in_data.encode('utf-8') if isinstance(in_data, str) else in_data or b''
        if not isinstance(cmd, str) or len(cmd.encode('utf-8')) > 64 * 1024 or len(data) > 256 * 1024:
            raise AnsibleConnectionFailure('ConsoleCrypt command or input exceeds its job limit')
        result = self._rpc('exec', {'command': cmd, 'stdin': base64.b64encode(data).decode('ascii')})
        try:
            if type(result['exit_code']) is not int or not 0 <= result['exit_code'] <= 255:
                raise ValueError('exit code')
            return result['exit_code'], base64.b64decode(result['stdout'], validate=True), base64.b64decode(result['stderr'], validate=True)
        except (ValueError, TypeError, KeyError) as error:
            raise AnsibleConnectionFailure('ConsoleCrypt command has no proven complete result') from error

    def put_file(self, in_path, out_path):
        super().put_file(in_path, out_path)
        transfer = str(uuid.uuid4())
        with open(in_path, 'rb') as source:
            size = os.fstat(source.fileno()).st_size
            if size > 100 * 1024 * 1024:
                raise AnsibleConnectionFailure('ConsoleCrypt transfer exceeds 100 MiB')
            offset = 0
            while True:
                chunk = source.read(128 * 1024)
                if offset + len(chunk) > size:
                    raise AnsibleConnectionFailure('Ansible source file changed during transfer')
                self._rpc('put_chunk', {'transfer_id': transfer, 'path': out_path, 'offset': offset, 'total': size,
                    'finish': offset + len(chunk) == size, 'data': base64.b64encode(chunk).decode('ascii')})
                offset += len(chunk)
                if offset == size:
                    break
                if not chunk:
                    raise AnsibleConnectionFailure('Ansible source file shrank during transfer')

    def fetch_file(self, in_path, out_path):
        super().fetch_file(in_path, out_path)
        offset = 0
        with open(out_path, 'wb') as destination:
            while True:
                result = self._rpc('get_chunk', {'path': in_path, 'offset': offset, 'limit': 128 * 1024})
                try:
                    total = result['total']
                    chunk = base64.b64decode(result['data'], validate=True)
                    if type(total) is not int or not 0 <= total <= 100 * 1024 * 1024 or len(chunk) > 128 * 1024 or offset + len(chunk) > total:
                        raise ValueError('transfer limit')
                except (ValueError, TypeError, KeyError) as error:
                    raise AnsibleConnectionFailure('Invalid ConsoleCrypt file response') from error
                destination.write(chunk)
                offset += len(chunk)
                if offset == total:
                    break
                if not chunk:
                    raise AnsibleConnectionFailure('Remote file changed during transfer')
