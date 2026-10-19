import 'dart:async';

import 'package:consolecrypt/core/models/terminal.dart';
import 'package:consolecrypt/src/rust/api/ssh.dart' as native;

/// Reattaches the output stream after lag without restarting its process.
/// A generation fence discards callbacks from cancelled attachments, and the
/// replacement snapshot carries its reset on the same ordered data channel.
final class TerminalAttachmentRelay {
  TerminalAttachmentRelay({
    required this.attach,
    required this.onOutput,
    required this.onStatus,
    required this.onError,
    required this.onDone,
  });

  final Stream<native.TerminalFrame> Function() attach;
  final void Function(TerminalOutputFrame) onOutput;
  final void Function(native.TerminalFrame) onStatus;
  final void Function(Object) onError;
  final void Function() onDone;
  StreamSubscription<native.TerminalFrame>? _subscription;
  int _generation = 0;
  bool _cancelled = false;

  void start() => _attach(reset: false);

  void _attach({required bool reset}) {
    if (_cancelled) return;
    final generation = ++_generation;
    // An idle native producer may not acknowledge cancellation until its
    // next frame. Do not wait for it before attaching the replacement.
    unawaited(_subscription?.cancel());
    bool current() => !_cancelled && generation == _generation;
    var resetNext = reset;
    _subscription = attach().listen(
      (frame) {
        if (!current()) return;
        switch (frame.kind) {
          case native.TerminalFrameKind.lagged:
            _attach(reset: true);
          case native.TerminalFrameKind.data:
            if (frame.data.isNotEmpty || resetNext) {
              onOutput(TerminalOutputFrame(frame.data, reset: resetNext));
            }
            resetNext = false;
          case native.TerminalFrameKind.closed || native.TerminalFrameKind.failed:
            onStatus(frame);
        }
      },
      onError: (Object error) {
        if (current()) onError(error);
      },
      onDone: () {
        if (current()) onDone();
      },
    );
  }

  Future<void> cancel() async {
    _cancelled = true;
    _generation++;
    await _subscription?.cancel();
  }
}
