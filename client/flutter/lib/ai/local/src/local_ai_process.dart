import 'dart:async';
import 'dart:io';

/// Internal custody of the ONE child already created by LocalAiRuntime.
/// No program/path selection, process launch, authority or registry.
final class LocalAiOwnedProcess {
  LocalAiOwnedProcess(
    this._child, {
    this.grace = const Duration(seconds: 3),
    this.forceGrace = const Duration(seconds: 3),
  }) {
    exitCode = _child.exitCode.then(
      (value) {
        _exited = true;
        return value;
      },
      onError: (Object error, StackTrace stack) {
        _unknownExit = true;
        Error.throwWithStackTrace(error, stack);
      },
    );
  }
  final Process _child;
  final Duration grace, forceGrace;
  late final Future<int> exitCode;
  bool _exited = false, _unknownExit = false;
  bool get hasExited => _exited;
  bool get hasUnknownExit => _unknownExit;
  Future<void>? _stopping;

  Future<void> stop() => _stopping ??= _stop().whenComplete(() => _stopping = null);
  Future<void> _stop() async {
    if (_exited) return;
    _child.kill();
    try {
      await exitCode.timeout(grace);
    } on TimeoutException {
      _child.kill(ProcessSignal.sigkill);
      await exitCode.timeout(forceGrace);
    }
  }
}

final class LocalAiProcessStopFailed implements Exception {
  const LocalAiProcessStopFailed();
}

/// Same runtime's process slot. Only real exit releases it; errors/failed kill
/// retain the original child and refuse a replacement. No model state here.
final class LocalAiProcessCustody {
  LocalAiOwnedProcess? _current;
  LocalAiOwnedProcess? get current => _current;
  bool get hasUnjoinedProcess => _current != null;
  bool isCurrentUsable(LocalAiOwnedProcess expected) =>
      identical(_current, expected) && !expected.hasExited && !expected.hasUnknownExit;
  void requireStartable() {
    if (_current != null) throw const LocalAiProcessStopFailed();
  }

  LocalAiOwnedProcess attach(
    Process child, {
    required void Function() onExit,
    required void Function() onUnknownExit,
    Duration grace = const Duration(seconds: 3),
    Duration forceGrace = const Duration(seconds: 3),
  }) {
    requireStartable();
    final owned = LocalAiOwnedProcess(child, grace: grace, forceGrace: forceGrace);
    _current = owned;
    unawaited(
      owned.exitCode.then<void>(
        (_) {
          if (identical(_current, owned)) {
            _current = null;
            onExit();
          }
        },
        onError: (Object _, StackTrace _) {
          if (identical(_current, owned)) onUnknownExit();
        },
      ),
    );
    return owned;
  }

  Future<void> retire(LocalAiOwnedProcess expected) async {
    await expected.stop();
    if (!expected.hasExited) throw const LocalAiProcessStopFailed();
    if (identical(_current, expected)) _current = null;
  }

  /// A Process.start ACK may arrive after stop begins. Await that SAME startup,
  /// then inspect the one slot again; never publish Stopped while it is owned.
  Future<void> stopAfter(Future<Object?>? starting) async {
    var failed = false;
    final original = _current;
    if (original != null) {
      try {
        await retire(original);
      } catch (_) {
        failed = true;
      }
    }
    try {
      await starting;
    } catch (_) {
      failed = true;
    }
    final late = _current;
    if (late != null) {
      if (failed) throw const LocalAiProcessStopFailed();
      await retire(late);
    }
    if (_current != null) throw const LocalAiProcessStopFailed();
  }
}
