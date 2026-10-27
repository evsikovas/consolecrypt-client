import 'dart:async';
import 'dart:isolate';

import 'package:yaml/yaml.dart';

final class KubeProposalIdentity {
  const KubeProposalIdentity(this.apiVersion, this.kind, this.name, this.namespace);
  final String apiVersion, kind, name;
  final String? namespace;
}

/// Generated manifests are untrusted. Parse only on an expendable worker,
/// bound input/time, and return four scalar identity fields to the UI.
final class KubeProposalParser {
  Isolate? _worker;
  ReceivePort? _port;
  Timer? _timer;
  Completer<KubeProposalIdentity?>? _result;
  void cancel() {
    _worker?.kill(priority: Isolate.immediate);
    _worker = null;
    _port?.close();
    _port = null;
    _timer?.cancel();
    _timer = null;
    final result = _result;
    _result = null;
    if (result != null && !result.isCompleted) result.complete(null);
  }

  Future<KubeProposalIdentity?> parse(String text) async {
    cancel();
    if (text.length > 256 * 1024) return null;
    final result = Completer<KubeProposalIdentity?>();
    _result = result;
    final port = ReceivePort();
    _port = port;
    port.listen((value) {
      if (!identical(_result, result)) return;
      if (value is List<String?> && value.length == 4 && value.take(3).every((s) => s != null)) {
        result.complete(KubeProposalIdentity(value[0]!, value[1]!, value[2]!, value[3]));
      } else {
        result.complete(null);
      }
      cancel();
    });
    try {
      final worker = await Isolate.spawn(_parse, (port.sendPort, text), onError: port.sendPort, onExit: port.sendPort);
      if (!identical(_result, result)) {
        worker.kill(priority: Isolate.immediate);
      } else {
        _worker = worker;
        // The bound is on parsing, not on starting the worker: a loaded
        // computer can take longer than a second just to spawn an isolate.
        _timer = Timer(const Duration(seconds: 1), cancel);
      }
    } catch (_) {
      if (identical(_result, result)) cancel();
    }
    return result.future;
  }

  static void _parse((SendPort, String) request) {
    final (port, text) = request;
    try {
      final document = loadYaml(text);
      if (document is! Map || document['metadata'] is! Map) {
        port.send(null);
        return;
      }
      final metadata = document['metadata'] as Map;
      final values = [document['apiVersion'], document['kind'], metadata['name'], metadata['namespace']];
      if (values.take(3).any((value) => value is! String || value.isEmpty || value.length > 1024) ||
          (values[3] != null && (values[3] is! String || (values[3] as String).length > 253))) {
        port.send(null);
        return;
      }
      port.send(values.cast<String?>());
    } catch (_) {
      port.send(null);
    }
  }
}
