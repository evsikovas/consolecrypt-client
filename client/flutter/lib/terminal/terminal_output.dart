import 'dart:async';
import 'dart:convert';

/// Decodes terminal bytes incrementally, yielding to the event queue before
/// each bounded ordinary-text parser call. Surrogate pairs stay together.
/// ANSI tokens are emitted atomically: xterm reparses incomplete CSI/OSC from
/// their start, so splitting a long token would introduce quadratic work.
/// A single escape token may exceed the ordinary-text quantum; this does not
/// promise a CPU time limit for xterm handling an unusually costly command.
///
/// Holds one decoded input event plus at most 2MiB UTF-16 units of an incomplete
/// escape token. An over-limit control token is discarded through its normal
/// terminator, preserving the ordinary text after it. Its partial prefix never
/// reaches xterm, including at EOF. Upstream transport/FRB buffers are separate.
/// Cancelling releases the strings/timer and cancels the source explicitly.
Stream<String> yieldingTerminalOutput(Stream<List<int>> source, {int maxCodeUnitsPerTurn = 4096}) {
  if (maxCodeUnitsPerTurn < 2) throw RangeError.range(maxCodeUnitsPerTurn, 2, null, 'maxCodeUnitsPerTurn');
  return _TerminalOutputPump(source, maxCodeUnitsPerTurn).stream;
}

const terminalEscapeTokenLimit = 2 * 1024 * 1024;

final class _TerminalOutputPump {
  _TerminalOutputPump(this._source, this._limit) {
    _controller = StreamController<String>(
      sync: true,
      onListen: _listen,
      onPause: _pause,
      onResume: _resume,
      onCancel: _cancel,
    );
  }

  final Stream<List<int>> _source;
  final int _limit;
  // ignore: close_sinks
  late final StreamController<String> _controller;
  // ignore: cancel_subscriptions
  StreamSubscription<String>? _input;
  Timer? _timer;
  String? _pending;
  StringBuffer? _token;
  int _tokenUnits = 0;
  bool _discardToken = false;
  _EscapeMode _mode = _EscapeMode.text;
  int _offset = 0;
  bool _cancelled = false;
  bool _paused = false;
  bool _inputPaused = false;
  bool _inputDone = false;

  Stream<String> get stream => _controller.stream;

  void _listen() {
    _input = const Utf8Decoder(allowMalformed: true)
        .bind(_source)
        .listen(
          _receive,
          onError: _controller.addError,
          onDone: () {
            _inputDone = true;
            _schedule();
          },
        );
  }

  void _receive(String chunk) {
    if (_cancelled || chunk.isEmpty) return;
    _pending = chunk;
    _offset = 0;
    _pauseInput();
    _schedule();
  }

  void _pauseInput() {
    if (!_inputPaused) {
      _inputPaused = true;
      _input?.pause();
    }
  }

  void _resumeInput() {
    if (_inputPaused && !_cancelled && !_paused) {
      _inputPaused = false;
      _input?.resume();
    }
  }

  void _schedule() {
    if (_timer == null && !_cancelled && !_paused) _timer = Timer(Duration.zero, _drain);
  }

  void _drain() {
    _timer = null;
    final chunk = _pending;
    if (_cancelled || _paused) return;
    if (chunk == null) {
      if (_inputDone) {
        // Preserve an incomplete final token once, exactly as direct xterm
        // input would. Never rescan it while waiting for more source frames.
        final tail = _token?.toString();
        _token = null;
        if (tail != null && tail.isNotEmpty) _controller.add(tail);
        if (!_cancelled) unawaited(_controller.close());
      }
      return;
    }
    var end = (_offset + _limit).clamp(0, chunk.length);
    // UTF-8 is decoded before slicing. Never turn a supplementary codepoint
    // into two replacement characters by splitting its UTF-16 surrogate pair.
    if (end < chunk.length && _isHighSurrogate(chunk.codeUnitAt(end - 1)) && _isLowSurrogate(chunk.codeUnitAt(end))) {
      end--;
    }
    final part = _scan(chunk, _offset, end);
    _offset = end;
    if (end == chunk.length) {
      _pending = null;
      _offset = 0;
    }
    if (part.isNotEmpty) _controller.add(part);
    // A synchronous consumer can cancel/pause while parsing this slice.
    if (_cancelled || _paused) return;
    if (_pending != null || _inputDone) {
      _schedule();
    } else {
      _resumeInput();
    }
  }

  String _scan(String chunk, int start, int end) {
    final output = StringBuffer();
    var runStart = start;
    for (var i = start; i < end; i++) {
      final char = chunk.codeUnitAt(i);
      var complete = false;
      switch (_mode) {
        case _EscapeMode.text:
          if (char == 0x1b) {
            output.write(chunk.substring(runStart, i));
            _token = StringBuffer();
            _tokenUnits = 0;
            _discardToken = false;
            _mode = _EscapeMode.escape;
            runStart = i;
          }
        case _EscapeMode.escape:
          _mode = switch (char) {
            0x5b => _EscapeMode.csi,
            0x5d => _EscapeMode.osc,
            0x28 || 0x29 => _EscapeMode.charset,
            _ => _EscapeMode.text,
          };
          complete = _mode == _EscapeMode.text;
        case _EscapeMode.csi:
          complete = char >= 0x40 && char <= 0x7e;
        case _EscapeMode.osc:
          complete = char == 0x07;
          if (char == 0x1b) _mode = _EscapeMode.oscEscape;
        case _EscapeMode.oscEscape:
          // Match xterm4: OSC ends after ESC plus any next scalar, not just ST.
          complete = true;
        case _EscapeMode.charset:
          complete = true;
      }
      if (complete) {
        _appendToken(chunk, runStart, i + 1);
        if (!_discardToken) output.write(_token);
        _token = null;
        _tokenUnits = 0;
        _discardToken = false;
        _mode = _EscapeMode.text;
        runStart = i + 1;
      }
    }
    if (_mode == _EscapeMode.text) {
      output.write(chunk.substring(runStart, end));
    } else {
      _appendToken(chunk, runStart, end);
    }
    return output.toString();
  }

  void _appendToken(String chunk, int start, int end) {
    if (_discardToken) return;
    final length = end - start;
    if (_tokenUnits + length > terminalEscapeTokenLimit) {
      _token = null;
      _tokenUnits = 0;
      _discardToken = true;
      return;
    }
    _tokenUnits += length;
    _token!.write(chunk.substring(start, end));
  }

  void _pause() {
    _paused = true;
    _timer?.cancel();
    _timer = null;
    _pauseInput();
  }

  void _resume() {
    _paused = false;
    if (_pending != null || _inputDone) {
      _schedule();
    } else {
      _resumeInput();
    }
  }

  Future<void> _cancel() async {
    _cancelled = true;
    _timer?.cancel();
    _timer = null;
    _pending = null;
    _token = null;
    _tokenUnits = 0;
    await _input?.cancel();
  }
}

enum _EscapeMode { text, escape, csi, osc, oscEscape, charset }

bool _isHighSurrogate(int codeUnit) => codeUnit >= 0xd800 && codeUnit <= 0xdbff;
bool _isLowSurrogate(int codeUnit) => codeUnit >= 0xdc00 && codeUnit <= 0xdfff;
