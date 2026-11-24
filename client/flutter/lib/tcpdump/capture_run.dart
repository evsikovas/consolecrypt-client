import 'dart:async';
import 'dart:collection';
import 'dart:convert';

import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/services/terminal_service.dart';
import 'package:consolecrypt/tcpdump/capture.dart';
import 'package:flutter/foundation.dart';

enum CaptureStatus {
  connecting,
  listening,
  stopped,
  ready,
  missing,
  denied,
  failed,
  noPeer,
  hostKeyChanged,
  invalidSudoPassword,
}

/// Each request exclusively owns its SSH PTY, including prompts.
/// Packets are kept in memory and repaint at most ten times per second.
final class CaptureRun extends ChangeNotifier {
  CaptureRun(this.service);
  final TerminalService service;
  final packets = ListQueue<CapturePacket>(displayLimit);
  int count = 0;
  CaptureStatus status = CaptureStatus.connecting;
  String? manager, error;
  bool _root = false;
  String? get installCommand {
    final tool = manager;
    final action = installActions[tool];
    if (tool == null || action == null) return null;
    return [if (!_root && tool != 'brew') 'sudo', tool, action, 'tcpdump'].join(' ');
  }

  HostKeyInfo? hostKey;
  TerminalPasswordPrompt? passwordPrompt;
  bool get active => status == CaptureStatus.connecting || status == CaptureStatus.listening;
  final _subscriptions = <StreamSubscription<dynamic>>[];
  TerminalSessionId? _id;
  Timer? _paint, _startup;
  bool _closed = false;
  bool timedOut = false;
  bool _sudoEnabled = false;
  int sudoAttempts = 0;
  int? sudoPrompt;
  CapturePacket? _lastPacket;
  int? exitCode;
  final _drained = Completer<void>();
  static final _ansi = RegExp(r'\x1b\[[0-?]*[ -/]*[@-~]');
  static final _listeningLine = RegExp(r'^(?:tcpdump: )?listening on');
  static final _sudoAuthRequired = RegExp(r'(?:password|interactive authentication) is required');
  static final _informationalDiagnostic = RegExp(r'verbose output suppressed|warning:|^tcpdump: data link type');

  Future<void> startRemote(Host source, ObjectId peerId, {bool privileged = false}) =>
      _startSsh(source, peerId: peerId, sudo: privileged);

  Future<void> checkRemote(Host host) => _startSsh(host);

  Future<void> _startSsh(Host source, {ObjectId? peerId, bool sudo = false}) async {
    _sudoEnabled = sudo && peerId != null;
    try {
      final transport = service;
      if (transport is! TcpdumpService) throw UnsupportedError('TCPdump requires the SSH backend');
      final handle = await (transport as TcpdumpService).openTcpdump(hostId: source.id, peerId: peerId, sudo: sudo);
      if (_closed || !active) {
        await service.close(handle.id);
        return;
      }
      _id = handle.id;
      _listenBytes(handle.output);
      _subscriptions.add(
        handle.events.listen(
          (event) {
            if (_closed || !active) return;
            switch (event) {
              case TerminalHostKeyPrompt(:final info):
                if (info.changed) {
                  _finish(CaptureStatus.hostKeyChanged);
                  return;
                }
                hostKey = info;
                notifyListeners();
              case TerminalPasswordPrompt():
                passwordPrompt = event;
                notifyListeners();
              case TerminalStateChanged(state: SessionConnectionState.connected):
                _startDeadline();
              case TerminalStateChanged(state: SessionConnectionState.disconnected, :final message):
                unawaited(_ended(message));
              case TerminalExited(exitCode: final code):
                exitCode = code;
              default:
                break;
            }
          },
          onError: _fail,
          onDone: () => unawaited(_ended(null)),
        ),
      );
    } catch (e) {
      _fail(e);
    }
  }

  void _listenBytes(Stream<List<int>> stream) {
    String? partial = '';
    // A login banner or malformed diagnostic can omit line breaks. Discard
    // an oversized line until its delimiter instead of retaining it forever.
    final lines = StreamTransformer<String, String>.fromHandlers(
      handleData: (chunk, sink) {
        var start = 0;
        while (start < chunk.length) {
          final end = chunk.indexOf('\n', start);
          final limit = end < 0 ? chunk.length : end;
          final prefix = partial;
          if (prefix != null) {
            partial = prefix.length + limit - start <= 4096 ? prefix + chunk.substring(start, limit) : null;
          }
          if (end < 0) break;
          if (partial case final line?) sink.add(line);
          partial = '';
          start = end + 1;
        }
      },
      handleDone: (sink) {
        if (partial case final line? when line.isNotEmpty) sink.add(line);
        sink.close();
      },
    );
    final sub = stream
        .cast<List<int>>()
        .transform(const Utf8Decoder(allowMalformed: true))
        .transform(lines)
        .listen(
          _line,
          onError: _fail,
          onDone: () {
            if (!_drained.isCompleted) _drained.complete();
          },
        );
    _subscriptions.add(sub);
  }

  Future<void> _ended(String? message) async {
    await _drained.future;
    if (_closed || !active) return;
    final clean = status == CaptureStatus.listening && exitCode == 0 && message == null;
    _finish(clean ? CaptureStatus.stopped : CaptureStatus.failed, message);
  }

  void _startDeadline() {
    if (status != CaptureStatus.connecting || sudoPrompt != null || _startup != null) return;
    _startup = Timer(const Duration(seconds: 20), () {
      timedOut = true;
      _finish(CaptureStatus.failed);
    });
  }

  void _line(String raw) {
    if (_closed || !active || raw.length > 4096) return;
    final line = raw.replaceAll(_ansi, '').trim();
    if (line.startsWith('0x')) {
      if (_lastPacket?.appendHex(line) == true) _schedulePaint();
      return;
    }
    _lastPacket = null;
    if (_sudoEnabled && status == CaptureStatus.connecting && line == 'CC_TCPDUMP_SUDO_PASSWORD') {
      _startup?.cancel();
      _startup = null;
      error = null;
      sudoPrompt = ++sudoAttempts;
      notifyListeners();
      return;
    }
    final missing = RegExp(r'^CC_MISSING ([a-z-]+)(?: (\d+))?$').firstMatch(line);
    if (missing != null) {
      manager = missing[1];
      _root = missing[2] == '0';
      _finish(CaptureStatus.missing);
      return;
    }
    if (line == 'CC_TCPDUMP_READY') {
      _finish(CaptureStatus.ready);
      return;
    }
    if (line == 'CC_NO_PEER') {
      _finish(CaptureStatus.noPeer);
      return;
    }
    if (_listeningLine.hasMatch(line)) {
      _listening();
      return;
    }
    if (line.startsWith('tcpdump:') || line.startsWith('sudo:') || line.startsWith('(') && line.contains('permitted')) {
      final lower = line.toLowerCase();
      if (lower.contains('permission') ||
          lower.contains('permitted') ||
          lower.contains('privilege') ||
          _sudoAuthRequired.hasMatch(lower)) {
        _finish(CaptureStatus.denied, line);
      } else if (!_informationalDiagnostic.hasMatch(lower)) {
        error = line;
      }
      return;
    }
    final packet = CapturePacket.parse(line);
    if (packet == null) {
      // A dedicated exec has no prompt or echoed command. Preserve shell and
      // system diagnostics as well as messages carrying the tcpdump prefix.
      if (status == CaptureStatus.connecting && line.isNotEmpty) error ??= line;
      return;
    }
    _listening();
    if (packets.length == displayLimit) packets.removeLast();
    _lastPacket = packet;
    packets.addFirst(packet);
    count++;
    _schedulePaint();
  }

  void _schedulePaint() {
    _paint ??= Timer(const Duration(milliseconds: 100), () {
      _paint = null;
      if (!_closed) notifyListeners();
    });
  }

  void _listening() {
    if (status == CaptureStatus.listening) return;
    _startup?.cancel();
    sudoPrompt = null;
    error = null;
    status = CaptureStatus.listening;
    notifyListeners();
  }

  Future<void> answerKey(HostKeyDecision decision) async {
    if (_closed || hostKey == null || _id == null) return;
    hostKey = null;
    notifyListeners();
    try {
      await service.answerHostKey(_id!, decision);
    } catch (e) {
      _fail(e);
    }
  }

  Future<void> answerPassword(SecretText secret) async {
    try {
      if (_closed || passwordPrompt == null || _id == null) return;
      passwordPrompt = null;
      notifyListeners();
      await service.answerPassword(_id!, secret);
    } catch (e) {
      _fail(e);
    } finally {
      secret.wipe();
    }
  }

  Future<void> answerSudo(int request, SecretText secret) async {
    Uint8List? bytes, input;
    try {
      final id = _id;
      if (_closed || !active || id == null || sudoPrompt != request) return;
      sudoPrompt = null;
      bytes = secret.exposeBytes();
      // A single terminal line. Control characters would be interpreted by the PTY.
      if (bytes.any((b) => b < 32 || b == 127)) {
        _finish(CaptureStatus.invalidSudoPassword);
        return;
      }
      input = Uint8List(bytes.length + 1)..setAll(0, bytes);
      input.last = 10;
      _startDeadline();
      notifyListeners();
      await service.write(id, input);
    } catch (e) {
      _fail(e);
    } finally {
      bytes?.fillRange(0, bytes.length, 0);
      input?.fillRange(0, input.length, 0);
      secret.wipe();
    }
  }

  void _fail(Object e) => _finish(CaptureStatus.failed, e.toString());
  void stop() => _finish(CaptureStatus.stopped);

  void _finish(CaptureStatus next, [String? message]) {
    if (_closed || !active) return;
    status = next;
    sudoPrompt = null;
    error ??= message;
    hostKey = null;
    passwordPrompt = null;
    _release();
    notifyListeners();
  }

  void _release() {
    if (!_drained.isCompleted) _drained.complete();
    _startup?.cancel();
    _paint?.cancel();
    final id = _id;
    _id = null;
    if (id != null) unawaited(service.close(id).catchError((Object _) {}));
    for (final sub in _subscriptions) {
      unawaited(sub.cancel());
    }
    _subscriptions.clear();
  }

  @override
  void dispose() {
    _closed = true;
    _release();
    packets.clear();
    super.dispose();
  }
}
