import 'dart:async';
import 'dart:collection';
import 'dart:convert';
import 'dart:io';

import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/services/terminal_service.dart';
import 'package:consolecrypt/tcpdump/capture.dart';
import 'package:flutter/foundation.dart';

enum CaptureStatus { connecting, listening, stopped, missing, denied, failed, noPeer }

/// Each request exclusively owns its process or SSH PTY, including prompts.
/// Packets are kept in memory and repaint at most ten times per second.
final class CaptureRun extends ChangeNotifier {
  CaptureRun(this.service);
  final TerminalService service;
  final packets = ListQueue<CapturePacket>(displayLimit);
  int count = 0;
  CaptureStatus status = CaptureStatus.connecting;
  String? manager, error, observedPair;
  bool _root = false;
  String? get installCommand {
    final command = installCommands[manager];
    return _root ? command?.replaceFirst(RegExp(r'^sudo '), '') : command;
  }

  HostKeyInfo? hostKey;
  TerminalPasswordPrompt? passwordPrompt;
  bool get active => status == CaptureStatus.connecting || status == CaptureStatus.listening;
  final _subscriptions = <StreamSubscription<dynamic>>[];
  TerminalSessionId? _id;
  Process? _process;
  Timer? _paint, _startup;
  bool _closed = false, _commandSent = false;
  static final _ansi = RegExp(r'\x1b\[[0-?]*[ -/]*[@-~]');
  static final _peer = RegExp(r'^CC_PEER ([0-9a-fA-F:.]+) ([0-9a-fA-F:.]+)$');

  Future<void> startRemote(Host source, String? other, {bool privileged = false}) async {
    try {
      final command = remoteCaptureCommand(other, privileged: privileged);
      final handle = await service.open(hostId: source.id, size: const TerminalSize(240, 24));
      if (_closed || !active) {
        await service.close(handle.id);
        return;
      }
      _id = handle.id;
      unawaited(_listenBytes(handle.output));
      _subscriptions.add(
        handle.events.listen(
          (event) {
            if (_closed || !active) return;
            switch (event) {
              case TerminalHostKeyPrompt(:final info):
                if (info.changed) {
                  _fail('SSH host key changed');
                  return;
                }
                hostKey = info;
                notifyListeners();
              case TerminalPasswordPrompt():
                passwordPrompt = event;
                notifyListeners();
              case TerminalStateChanged(state: SessionConnectionState.connected):
                if (_commandSent) return;
                _commandSent = true;
                _startDeadline();
                unawaited(
                  service
                      .write(handle.id, Uint8List.fromList(utf8.encode('$command\r')))
                      .catchError((Object e) => _fail(e)),
                );
              case TerminalStateChanged(state: SessionConnectionState.disconnected, :final message):
                _finish(
                  status == CaptureStatus.listening && message == null ? CaptureStatus.stopped : CaptureStatus.failed,
                  message,
                );
              case TerminalExited(:final exitCode):
                _finish(
                  exitCode == 0 && status == CaptureStatus.listening ? CaptureStatus.stopped : CaptureStatus.failed,
                );
              default:
                break;
            }
          },
          onError: _fail,
          onDone: () => _finish(CaptureStatus.stopped),
        ),
      );
    } catch (e) {
      _fail(e);
    }
  }

  Future<void> startLocal(String peer, {String? device}) async {
    try {
      final executable = _localExecutable();
      if (executable == null) {
        manager = _localManager();
        _finish(CaptureStatus.missing);
        return;
      }
      final args = await localCaptureArguments(peer);
      if (device != null && device.isNotEmpty) {
        if (args.first == '-i') args.removeRange(0, 2);
        args.insertAll(0, ['-i', device]);
      }
      if (_closed || !active) return;
      final process = await Process.start(executable, args, environment: {'LC_ALL': 'C'});
      if (_closed || !active) {
        process.kill();
        return;
      }
      _process = process;
      _startDeadline();
      final outputDone = _listenBytes(process.stdout), errorDone = _listenBytes(process.stderr);
      final code = await process.exitCode;
      await Future.wait([outputDone, errorDone]);
      _finish(code == 0 ? CaptureStatus.stopped : CaptureStatus.failed);
    } on ProcessException catch (e) {
      if (e.errorCode == 2 || e.errorCode == 3) {
        manager = _localManager();
        _finish(CaptureStatus.missing);
      } else {
        _fail(e);
      }
    } catch (e) {
      _fail(e);
    }
  }

  static String? _localExecutable() {
    final name = Platform.isWindows ? 'tcpdump.exe' : 'tcpdump';
    final path = Platform.environment['PATH'] ?? '';
    final paths = {
      if (!Platform.isWindows) ...['/usr/sbin', '/sbin', '/opt/homebrew/bin', '/usr/local/bin'],
      ...path.split(Platform.isWindows ? ';' : ':'),
    };
    for (final entry in paths) {
      final dir = Directory(entry.replaceAll('"', ''));
      if (!dir.isAbsolute) continue;
      final file = File('${dir.path}${Platform.pathSeparator}$name');
      if (file.existsSync()) return file.path;
    }
    return null;
  }

  static String? _localManager() {
    if (Platform.isMacOS) return 'brew';
    if (Platform.isLinux) {
      for (final manager in installCommands.keys) {
        for (final bin in ['/usr/bin', '/sbin', '/usr/sbin', '/bin']) {
          if (File('$bin/$manager').existsSync()) return manager;
        }
      }
    }
    return null;
  }

  Future<void> _listenBytes(Stream<List<int>> stream) {
    final done = Completer<void>();
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
          onError: (Object e) {
            _fail(e);
            if (!done.isCompleted) done.complete();
          },
          onDone: () {
            if (!done.isCompleted) done.complete();
          },
        );
    _subscriptions.add(sub);
    // Local exit waits for buffered output; cancellation must also release it.
    _drains.add(done);
    return done.future;
  }

  final _drains = <Completer<void>>[];

  void _startDeadline() {
    _startup = Timer(const Duration(seconds: 20), () => _finish(CaptureStatus.failed));
  }

  void _line(String raw) {
    if (_closed || !active || raw.length > 4096) return;
    final line = raw.replaceAll(_ansi, '').trim();
    final missing = RegExp(r'^CC_MISSING ([a-z-]+)(?: (\d+))?$').firstMatch(line);
    if (missing != null) {
      manager = missing[1];
      _root = missing[2] == '0';
      _finish(CaptureStatus.missing);
      return;
    }
    if (line == 'CC_NO_PEER') {
      _finish(CaptureStatus.noPeer);
      return;
    }
    final peer = _peer.firstMatch(line);
    if (peer != null) {
      observedPair = '${peer[1]} ↔ ${peer[2]}';
      return;
    }
    if (line.startsWith('tcpdump: listening on') || line.startsWith('listening on')) {
      _listening();
      return;
    }
    if (line.startsWith('tcpdump:') || line.startsWith('sudo:') || line.startsWith('(') && line.contains('permitted')) {
      final lower = line.toLowerCase();
      if (lower.contains('permission') ||
          lower.contains('permitted') ||
          lower.contains('privilege') ||
          lower.contains('password is required')) {
        _finish(CaptureStatus.denied, line);
      } else if (!lower.contains('verbose output suppressed') &&
          !lower.contains('warning:') &&
          !lower.startsWith('tcpdump: data link type')) {
        error = line;
      }
      return;
    }
    final packet = CapturePacket.parse(line);
    if (packet == null) return;
    _listening();
    if (packets.length == displayLimit) packets.removeLast();
    packets.addFirst(packet);
    count++;
    _paint ??= Timer(const Duration(milliseconds: 100), () {
      _paint = null;
      if (!_closed) notifyListeners();
    });
  }

  void _listening() {
    if (status == CaptureStatus.listening) return;
    _startup?.cancel();
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

  void _fail(Object e) => _finish(CaptureStatus.failed, e.toString());
  void stop() => _finish(CaptureStatus.stopped);

  void _finish(CaptureStatus next, [String? message]) {
    if (_closed || !active) return;
    status = next;
    error ??= message;
    hostKey = null;
    passwordPrompt = null;
    _release();
    notifyListeners();
  }

  void _release() {
    _startup?.cancel();
    _paint?.cancel();
    _process?.kill();
    _process = null;
    final id = _id;
    _id = null;
    if (id != null) unawaited(service.close(id).catchError((Object _) {}));
    for (final sub in _subscriptions) {
      unawaited(sub.cancel());
    }
    _subscriptions.clear();
    for (final done in _drains) {
      if (!done.isCompleted) done.complete();
    }
    _drains.clear();
  }

  @override
  void dispose() {
    _closed = true;
    _release();
    packets.clear();
    super.dispose();
  }
}
