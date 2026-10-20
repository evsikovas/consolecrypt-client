import 'dart:async';
import 'dart:isolate';

/// A user-entered regex runs off the UI thread and is forcibly terminated at
/// its deadline, including catastrophic backtracking on application logs.
final class KubeLogFilter {
  Isolate? _worker;
  ReceivePort? _port;
  Completer<List<String>>? _completion;
  Timer? _deadline;
  void cancel() {
    _worker?.kill(priority: Isolate.immediate);
    _worker = null;
    _port?.close();
    _port = null;
    _deadline?.cancel();
    _deadline = null;
    final completion = _completion;
    _completion = null;
    if (completion != null && !completion.isCompleted) completion.complete(const []);
  }

  Future<List<String>> apply(List<String> lines, String pattern, bool regex, String level) async {
    cancel();
    if (pattern.length > 512) throw const FormatException('Log pattern is too long');
    if (!regex) return _matching(lines, pattern, false, level);
    final completion = Completer<List<String>>();
    _completion = completion;
    final port = ReceivePort();
    _port = port;
    port.listen((message) {
      if (!identical(_completion, completion)) return;
      if (message is List<String>) {
        completion.complete(message);
      } else {
        completion.completeError(const FormatException('Invalid or slow log pattern'));
      }
      cancel();
    });
    try {
      final worker = await Isolate.spawn(_filter, (port.sendPort, lines, pattern, regex, level), errorsAreFatal: true);
      if (!identical(_completion, completion)) {
        worker.kill(priority: Isolate.immediate);
        return await completion.future;
      }
      _worker = worker;
      _deadline = Timer(const Duration(milliseconds: 500), () {
        if (identical(_completion, completion)) {
          completion.completeError(const FormatException('Invalid or slow log pattern'));
          cancel();
        }
      });
    } catch (error) {
      if (!completion.isCompleted) completion.completeError(error);
      cancel();
    }
    return completion.future;
  }

  static void _filter((SendPort, List<String>, String, bool, String) args) {
    final (port, lines, pattern, regex, level) = args;
    try {
      port.send(_matching(lines, pattern, regex, level));
    } catch (_) {
      port.send(false);
    }
  }

  static List<String> _matching(List<String> lines, String pattern, bool regex, String level) {
    final expression = regex && pattern.isNotEmpty ? RegExp(pattern, caseSensitive: false) : null;
    final needle = pattern.toLowerCase();
    final severity = level == 'all'
        ? null
        : RegExp(switch (level) {
            'error' => r'\b(error|fatal|panic)\b',
            'warn' => r'\b(warn|warning)\b',
            'info' => r'\binfo\b',
            _ => r'\b(debug|trace)\b',
          }, caseSensitive: false);
    return lines
        .where(
          (line) =>
              (severity == null || severity.hasMatch(line)) &&
              (expression?.hasMatch(line) ?? line.toLowerCase().contains(needle)),
        )
        .toList();
  }
}
