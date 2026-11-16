import 'dart:async';

import 'package:consolecrypt/core/services/runtime_notice_events.dart';

abstract interface class RuntimeNoticeEvents {
  RuntimeNoticeFrame get currentScope;
  Stream<RuntimeNoticeFrame> watchRuntimeNotices();
}

/// Owned by the EXISTING backend hub. It fans out typed metadata from that
/// hub's ONE Native event subscription; it opens no service, RPC or timer.
final class RuntimeNoticeEventAdapter implements RuntimeNoticeEvents {
  final _projection = RuntimeNoticeProjection();
  final _frames = StreamController<RuntimeNoticeFrame>.broadcast();
  bool _disposed = false;

  int get epoch => _projection.epoch;
  @override
  RuntimeNoticeFrame get currentScope => _projection.currentScope;
  @override
  Stream<RuntimeNoticeFrame> watchRuntimeNotices() => _frames.stream;

  void _emit(RuntimeNoticeFrame? frame) {
    if (!_disposed && frame != null) _frames.add(frame);
  }

  void boundary({bool unlockPending = false}) => _emit(_projection.boundary(unlockPending: unlockPending));

  void retireGlobalStream({bool closed = false}) => _emit(_projection.retireGlobalStream(closed: closed));

  void acceptScope({required int originalEpoch, required Map<String, dynamic>? nativeProfile}) {
    final reset = _projection.establish(originalEpoch, nativeProfile);
    _emit(reset);
    if (reset != null) {
      for (final frame in _projection.unlockReplay) {
        _emit(frame);
      }
    }
  }

  List<RuntimeNoticeFrame> takeUnlockReplay(RuntimeNoticeFrame original, String profileId) =>
      _projection.takeUnlockReplay(original, profileId);

  void ingest(Map<String, dynamic> event) {
    if (_disposed) return;
    _emit(event['type'] == 'lagged' ? _projection.gap() : _projection.consume(event));
  }

  void streamLost({bool closed = false}) => _emit(_projection.gap(streamUnavailable: closed));

  Future<void> dispose() async {
    if (_disposed) return;
    boundary();
    _disposed = true;
    _projection.dispose();
    await _frames.close();
  }
}
