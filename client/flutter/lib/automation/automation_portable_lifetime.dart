import 'dart:async';

import 'package:consolecrypt/core/services/runtime_notice_adapter.dart';
import 'package:flutter/foundation.dart';

/// One dialog's DATA lifetime over the existing backend hub fan-out. This does
/// not read/adopt the observer nonce and cannot authorize any Native operation.
final class PortableUiLifetime extends ChangeNotifier {
  PortableUiLifetime({required this.profileId, required this.events, required this.isOriginalUiCurrent})
    : _epoch = events.currentScope.epoch {
    _subscription = events.watchRuntimeNotices().listen(
      (_) {
        if (!current) retire();
      },
      onError: (Object _) => retire(),
      onDone: retire,
    );
  }
  final String profileId;
  final RuntimeNoticeEvents events;
  final int _epoch;
  final bool Function() isOriginalUiCurrent;
  late final StreamSubscription<Object?> _subscription;
  bool _retired = false, _disposed = false;

  /// Synchronous before fan-out/UI microtasks; domain incomplete alone is not
  /// a profile boundary. Availability transition and the original epoch are.
  bool get current =>
      !_retired &&
      !_disposed &&
      isOriginalUiCurrent() &&
      events.currentScope.epoch == _epoch &&
      !events.currentScope.streamUnavailable;

  /// Caller also invokes this on original project/route/request replacement.
  /// A retired lifetime never captures a new epoch or resumes automatically.
  void retire() {
    if (_retired || _disposed) return;
    _retired = true;
    notifyListeners();
  }

  @override
  void dispose() {
    if (_disposed) return;
    retire();
    _disposed = true;
    unawaited(_subscription.cancel());
    super.dispose();
  }
}
