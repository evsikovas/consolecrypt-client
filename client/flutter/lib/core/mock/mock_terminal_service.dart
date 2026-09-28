import 'dart:async';
import 'dart:convert';
import 'dart:typed_data';

import 'package:consolecrypt/core/mock/fake_data.dart';
import 'package:consolecrypt/core/mock/mock_config.dart';
import 'package:consolecrypt/core/mock/mock_inventory_service.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/services/app_services.dart';

/// Simulated SSH shell. Honours host-key policies against the mock known
/// hosts (unknown → prompt, changed → hard failure), supports a few
/// commands, `simulate-drop` (network drop → reconnect banner) and `exit`.
final class MockTerminalService implements TerminalService {
  MockTerminalService(this._config, this._inventory);

  final MockConfig _config;
  final MockInventoryService _inventory;
  final Map<TerminalSessionId, _MockShell> _sessions = {};

  @override
  Future<TerminalSessionHandle> open({required ObjectId hostId, required TerminalSize size}) async {
    final host = _inventory.hostById(hostId);
    if (host == null) {
      throw const AppException(AppErrorCode.notFound, 'Host not found', reason: AppErrorReason.hostNotFound);
    }
    final effective = await _inventory.resolveEffective(host);
    final shell = _MockShell(
      id: TerminalSessionId.generate(),
      host: host,
      effective: effective,
      size: size,
      config: _config,
      inventory: _inventory,
    );
    _sessions[shell.id] = shell;
    unawaited(shell.connect());
    return TerminalSessionHandle(id: shell.id, output: shell.output.stream, events: shell.events.stream);
  }

  _MockShell _shell(TerminalSessionId id) {
    final s = _sessions[id];
    if (s == null) {
      throw const AppException(AppErrorCode.notFound, 'Session closed', reason: AppErrorReason.sessionClosed);
    }
    return s;
  }

  @override
  Future<void> write(TerminalSessionId id, Uint8List data) async => _sessions[id]?.input(data);

  @override
  Future<void> resize(TerminalSessionId id, TerminalSize size) async => _sessions[id]?.size = size;

  @override
  Future<void> answerPassword(TerminalSessionId id, SecretText? password) async {
    final pending = _shell(id).pendingPassword;
    if (pending == null || pending.isCompleted) {
      password?.wipe();
      return;
    }
    // Mock: any non-empty password is accepted. It is used once and wiped.
    pending.complete(password?.isNotEmpty);
    password?.wipe();
  }

  @override
  Future<void> answerHostKey(TerminalSessionId id, HostKeyDecision decision) async {
    final pending = _shell(id).pendingHostKey;
    if (pending != null && !pending.isCompleted) pending.complete(decision);
  }

  @override
  Future<void> reconnect(TerminalSessionId id) => _shell(id).connect(reconnect: true);

  @override
  Future<void> close(TerminalSessionId id) async => _sessions.remove(id)?.dispose();

  Future<void> dispose() async {
    for (final s in _sessions.values) {
      await s.dispose();
    }
    _sessions.clear();
  }
}

final class _MockShell {
  _MockShell({
    required this.id,
    required this.host,
    required this.effective,
    required this.size,
    required this.config,
    required this.inventory,
  });

  final TerminalSessionId id;
  final Host host;
  final EffectiveHostConfig effective;
  final MockConfig config;
  final MockInventoryService inventory;
  TerminalSize size;

  final StreamController<Uint8List> output = StreamController<Uint8List>();
  final StreamController<TerminalEvent> events = StreamController<TerminalEvent>();
  Completer<HostKeyDecision>? pendingHostKey;

  /// Completes with `true` (accepted), `false` (rejected) or `null` (cancel).
  Completer<bool?>? pendingPassword;

  bool _connected = false;
  bool _connecting = false;
  bool _acceptedOnce = false;
  bool _disposed = false;
  String _line = '';
  String _cwd = '~';
  int _escape = 0;

  String get _user => effective.username.value ?? 'root';

  int get _port => effective.port.value ?? defaultSshPort;

  void _out(String text) {
    if (!_disposed) output.add(Uint8List.fromList(utf8.encode(text)));
  }

  void _event(TerminalEvent e) {
    if (!_disposed) events.add(e);
  }

  void _prompt() => _out('\x1b[1;32m$_user@${host.name}\x1b[0m:\x1b[1;34m$_cwd\x1b[0m\$ ');

  Future<void> connect({bool reconnect = false}) async {
    if (_connected || _connecting || _disposed) return;
    _connecting = true;
    try {
      final via = effective.route.isEmpty
          ? 'direct'
          : 'via ${effective.route.map((h) => h.label.split(' ').first).join(' → ')}';
      _event(
        TerminalStateChanged(
          reconnect ? SessionConnectionState.reconnecting : SessionConnectionState.connecting,
          message: '$_user@${host.address}:$_port ($via)',
        ),
      );
      await mockDelay(config.latency);
      if (!await _verifyHostKey()) return;
      if (!await _authenticate()) return;
      await mockDelay(config.latency);
      if (_disposed) return;
      _connected = true;
      _event(const TerminalStateChanged(SessionConnectionState.connected));
      _event(TerminalTitleChanged('$_user@${host.name}'));
      if (reconnect) {
        _out('\r\n\x1b[2m[reconnected]\x1b[0m\r\n');
      } else {
        _out(_banner());
      }
      _prompt();
    } finally {
      _connecting = false;
    }
  }

  /// No credential resolves (inline password not saved, or none at all):
  /// ask the user, like an interactive `ssh` password prompt.
  Future<bool> _authenticate() async {
    if (effective.credentialId.value != null) return true;
    var retry = false;
    while (!_disposed) {
      final completer = pendingPassword = Completer<bool?>();
      _event(const TerminalStateChanged(SessionConnectionState.awaitingPassword));
      _event(TerminalPasswordPrompt(username: _user, hostLabel: '${host.name} (${host.address})', retry: retry));
      final result = await completer.future;
      pendingPassword = null;
      if (result == null) {
        _event(const TerminalStateChanged(SessionConnectionState.disconnected, message: 'Authentication cancelled.'));
        return false;
      }
      if (result) return true;
      retry = true;
    }
    return false;
  }

  Future<bool> _verifyHostKey() async {
    final pattern = hostPattern(host.address, _port);
    if (inventory.keyChanged(pattern)) {
      _event(
        TerminalHostKeyPrompt(
          HostKeyInfo(
            hostPattern: pattern,
            keyType: 'ssh-ed25519',
            fingerprintSha256: fakeFingerprint(),
            changed: true,
          ),
        ),
      );
      _event(
        const TerminalStateChanged(
          SessionConnectionState.disconnected,
          message:
              'Host key verification failed: the server key CHANGED. '
              'This can mean a man-in-the-middle attack. Connection refused.',
        ),
      );
      return false;
    }
    if (inventory.knownHostFor(pattern) != null || _acceptedOnce) return true;
    final info = HostKeyInfo(hostPattern: pattern, keyType: 'ssh-ed25519', fingerprintSha256: fakeFingerprint());
    switch (host.hostKeyPolicy) {
      case HostKeyPolicy.strict:
        _event(
          const TerminalStateChanged(
            SessionConnectionState.disconnected,
            message: 'Host key is not in known hosts (policy: Strict).',
          ),
        );
        return false;
      case HostKeyPolicy.acceptNew:
        _remember(info, KnownHostSource.tofu);
        return true;
      case HostKeyPolicy.ask:
        final completer = pendingHostKey = Completer<HostKeyDecision>();
        _event(const TerminalStateChanged(SessionConnectionState.awaitingHostKey));
        _event(TerminalHostKeyPrompt(info));
        final decision = await completer.future;
        pendingHostKey = null;
        switch (decision) {
          case HostKeyDecision.reject:
            _event(const TerminalStateChanged(SessionConnectionState.disconnected, message: 'Host key rejected.'));
            return false;
          case HostKeyDecision.acceptOnce:
            _acceptedOnce = true;
            return true;
          case HostKeyDecision.acceptAndSave:
            _remember(info, KnownHostSource.tofu);
            return true;
        }
    }
  }

  void _remember(HostKeyInfo info, KnownHostSource source) {
    final now = DateTime.now().toUtc();
    inventory.addKnownHost(
      KnownHost(
        id: ObjectId.generate(),
        hostPattern: info.hostPattern,
        keyType: info.keyType,
        publicKey: fakeBase64(32),
        fingerprintSha256: info.fingerprintSha256,
        source: source,
        addedAt: now,
        updatedAt: now,
      ),
    );
  }

  String _banner() {
    final lines = [
      'Welcome to Ubuntu 24.04.1 LTS (GNU/Linux 6.8.0-45-generic x86_64)',
      '',
      '  \x1b[2mConsoleCrypt demo shell — this is a simulated session (mock backend).\x1b[0m',
      '  Try: help, ls -la, df -h, kubectl get pods, simulate-drop, exit',
      '',
      'Last login: ${DateTime.now().subtract(const Duration(hours: 20)).toIso8601String().substring(0, 19)} from 192.0.2.10',
    ];
    return '${lines.join('\r\n')}\r\n';
  }

  void input(Uint8List data) {
    if (!_connected || _disposed) return;
    final text = utf8.decode(data, allowMalformed: true);
    for (final rune in text.runes) {
      if (_escape == 1) {
        _escape = (rune == 0x5b || rune == 0x4f) ? 2 : 0;
        continue;
      }
      if (_escape == 2) {
        if (rune >= 0x40 && rune <= 0x7e) _escape = 0;
        continue;
      }
      switch (rune) {
        case 0x1b:
          _escape = 1;
        case 0x0d:
          _out('\r\n');
          final command = _line;
          _line = '';
          _execute(command);
        case 0x0a:
          break;
        case 0x7f || 0x08:
          if (_line.isNotEmpty) {
            _line = String.fromCharCodes(_line.runes.toList()..removeLast());
            _out('\b \b');
          }
        case 0x03:
          _out('^C\r\n');
          _line = '';
          _prompt();
        case 0x04:
          if (_line.isEmpty) _execute('exit');
        case 0x0c:
          _out('\x1b[2J\x1b[H');
          _prompt();
        default:
          if (rune >= 0x20) {
            final ch = String.fromCharCode(rune);
            _line += ch;
            _out(ch);
          }
      }
    }
  }

  void _execute(String raw) {
    final command = raw.trim();
    if (command.isEmpty) {
      _prompt();
      return;
    }
    final parts = command.split(RegExp(r'\s+'));
    final rest = parts.skip(1).join(' ');
    String? out;
    switch (parts.first) {
      case 'help':
        out =
            'Demo commands: ls [-la], pwd, cd, whoami, hostname, uname -a, date, uptime,\r\n'
            'df -h, free -h, ss -tulpn, kubectl get pods, docker ps, echo, clear,\r\n'
            'simulate-drop (network drop), exit';
      case 'ls':
        out = rest.contains('l')
            ? 'total 32\r\n'
                  'drwxr-xr-x 5 $_user $_user 4096 Sep 26 09:12 \x1b[1;34mapp\x1b[0m\r\n'
                  'drwxr-xr-x 2 $_user $_user 4096 Sep 25 23:00 \x1b[1;34mbackups\x1b[0m\r\n'
                  'drwxr-xr-x 2 $_user $_user 4096 Sep 26 08:41 \x1b[1;34mlogs\x1b[0m\r\n'
                  '-rw-r--r-- 1 $_user $_user 3771 Sep 20 11:02 .bashrc\r\n'
                  '-rw-r--r-- 1 $_user $_user  512 Sep 24 17:30 notes.txt'
            : '\x1b[1;34mapp\x1b[0m  \x1b[1;34mbackups\x1b[0m  \x1b[1;34mlogs\x1b[0m  notes.txt';
      case 'pwd':
        out = _cwd == '~' ? '/home/$_user' : _cwd;
      case 'cd':
        _cwd = rest.isEmpty || rest == '~' ? '~' : (rest.startsWith('/') ? rest : '~/$rest');
      case 'whoami':
        out = _user;
      case 'hostname':
        out = host.name;
      case 'uname':
        out = 'Linux ${host.name} 6.8.0-45-generic #45-Ubuntu SMP x86_64 GNU/Linux';
      case 'date':
        out = DateTime.now().toUtc().toString();
      case 'uptime':
        out =
            ' ${DateTime.now().toIso8601String().substring(11, 19)} up 41 days,  3:07,  1 user,  load average: 0.21, 0.18, 0.12';
      case 'df':
        out =
            'Filesystem      Size  Used Avail Use% Mounted on\r\n'
            '/dev/vda1        78G   31G   44G  42% /\r\n'
            'tmpfs           3.9G     0  3.9G   0% /dev/shm\r\n'
            '/dev/vdb1       492G  301G  166G  65% /var/lib/postgresql';
      case 'free':
        out =
            '               total        used        free      shared  buff/cache   available\r\n'
            'Mem:           7.8Gi       3.1Gi       1.2Gi        54Mi       3.5Gi       4.4Gi\r\n'
            'Swap:          2.0Gi       128Mi       1.9Gi';
      case 'ss':
        out =
            'Netid State  Local Address:Port  Process\r\n'
            'tcp   LISTEN 0.0.0.0:22           sshd\r\n'
            'tcp   LISTEN 127.0.0.1:5432       postgres\r\n'
            'tcp   LISTEN 0.0.0.0:443          nginx';
      case 'kubectl':
        out = rest.startsWith('logs')
            ? '2026-09-26T09:14:02Z INFO  request handled path=/health status=200\r\n'
                  '2026-09-26T09:14:07Z WARN  slow query duration_ms=812'
            : 'NAME                   READY   STATUS    RESTARTS   AGE\r\n'
                  'api-7d9c6b7f9d-2kqzv   1/1     Running   0          3d4h\r\n'
                  'worker-5f6b8c9d-x7p2m  1/1     Running   2          3d4h';
      case 'docker':
        out =
            'CONTAINER ID   IMAGE          STATUS        NAMES\r\n'
            '3f2a9c1b7e44   nginx:1.27     Up 6 days     edge\r\n'
            '9b1d0e4c2a11   postgres:16    Up 6 days     db';
      case 'echo':
        out = rest;
      case 'clear':
        _out('\x1b[2J\x1b[H');
      case 'exit' || 'logout':
        _out('logout\r\n');
        _connected = false;
        _event(const TerminalExited(0));
        _event(
          const TerminalStateChanged(SessionConnectionState.disconnected, message: 'Session closed by remote shell.'),
        );
        return;
      case 'simulate-drop':
        _connected = false;
        _event(
          const TerminalStateChanged(
            SessionConnectionState.disconnected,
            message: 'Connection reset by peer (simulated network drop).',
          ),
        );
        return;
      case 'sudo' || 'systemctl' || 'rm' || 'find' || 'helm' || 'psql' || 'terraform':
        out = '\x1b[33m[demo] "$command" is not executed by the mock shell.\x1b[0m';
      default:
        out = '${parts.first}: command not found';
    }
    if (out != null) _out('$out\r\n');
    _prompt();
  }

  Future<void> dispose() async {
    if (_disposed) return;
    _disposed = true;
    final pending = pendingHostKey;
    if (pending != null && !pending.isCompleted) pending.complete(HostKeyDecision.reject);
    final password = pendingPassword;
    if (password != null && !password.isCompleted) password.complete(null);
    // Not awaited: `close()` of a never-listened single-subscription
    // controller only completes once someone listens.
    unawaited(output.close());
    unawaited(events.close());
  }
}
