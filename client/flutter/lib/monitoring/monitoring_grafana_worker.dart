import 'dart:async';
import 'dart:convert';
import 'dart:isolate';

import 'package:consolecrypt/monitoring/grafana_constant_review.dart';
import 'package:consolecrypt/monitoring/grafana_import.dart';

final class GrafanaInspectionCancelled implements Exception {
  const GrafanaInspectionCancelled();
}

final class GrafanaInspection {
  const GrafanaInspection(this.report, {this.apiVersion, this.kind, this.schemaVersion, this.constantNames = const []});
  final GrafanaImportReport report;
  final String? apiVersion, kind, schemaVersion;
  final List<String> constantNames;
}

abstract interface class GrafanaDashboardInspector {
  Future<GrafanaInspection> inspect(String input);
  void cancel();
  void dispose();
}

/// Optional capability on the same cancellable inspector. Existing import
/// callers/fakes remain valid; unsupported capabilities cannot fabricate review.
abstract interface class GrafanaManualConstantInspector {
  Future<GrafanaConstantReview> reviewConstants(String input, String panelPath, Map<String, String> values);
}

List<String> _constantNames(Map<String, dynamic> root) {
  if (root['apiVersion'] == 'dashboard.grafana.app/v2') return const [];
  final dashboard = root['spec'] is Map
      ? root['spec'] as Map
      : root['dashboard'] is Map
      ? root['dashboard'] as Map
      : root;
  final templating = dashboard['templating'];
  final declarations = templating is Map ? templating['list'] : null;
  if (declarations is! List) return const [];
  final names = <String>{};
  for (final declaration in declarations) {
    if (declaration is! Map || declaration['type'] != 'constant') continue;
    final name = declaration['name'];
    if (name is String && RegExp(r'^[A-Za-z_][A-Za-z0-9_]{0,127}$').hasMatch(name) && !name.startsWith('__')) {
      names.add(name);
      if (names.length > 64) return const [];
    }
  }
  return List.unmodifiable(names);
}

String? _identifier(Object? value) {
  if (value is! String && value is! int) return null;
  final text = value.toString();
  return String.fromCharCodes(text.runes.take(128).map((r) => r >= 32 && r <= 126 ? r : 0xfffd));
}

void _inspectDashboard((SendPort, String, String?, Map<String, String>?) message) {
  try {
    if (message.$3 case final panelPath?) {
      message.$1.send(reviewGrafanaConstants(message.$2, panelPath, message.$4 ?? const {}));
      return;
    }
    final report = inspectGrafanaDashboard(message.$2);
    final root = jsonDecode(message.$2) as Map<String, dynamic>;
    final body = root['spec'] is Map
        ? root['spec'] as Map
        : root['dashboard'] is Map
        ? root['dashboard'] as Map
        : root;
    message.$1.send(
      GrafanaInspection(
        report,
        apiVersion: _identifier(root['apiVersion']),
        kind: _identifier(root['kind']),
        schemaVersion: _identifier(body['schemaVersion']),
        constantNames: _constantNames(root),
      ),
    );
  } on GrafanaConstantException catch (error) {
    message.$1.send(error.problem);
  } on GrafanaImportException catch (error) {
    message.$1.send(error.problem);
  } catch (_) {
    // No original JSON, server values, exception text or stack is projected.
    message.$1.send(GrafanaImportProblem.invalidDashboard);
  }
}

/// One cancellable CPU-only worker per dialog. It owns no source, credentials,
/// file path, network, persistence, Native service or application authority.
final class IsolatedGrafanaDashboardInspector implements GrafanaDashboardInspector, GrafanaManualConstantInspector {
  Isolate? _isolate;
  ReceivePort? _port;
  Completer<Object>? _pending;
  int _epoch = 0;
  bool _disposed = false;

  @override
  Future<GrafanaInspection> inspect(String input) => _execute<GrafanaInspection>(input);

  @override
  Future<GrafanaConstantReview> reviewConstants(String input, String panelPath, Map<String, String> values) =>
      _execute<GrafanaConstantReview>(input, panelPath: panelPath, values: Map.of(values));

  Future<T> _execute<T>(String input, {String? panelPath, Map<String, String>? values}) {
    cancel();
    if (_disposed) return Future.error(const GrafanaInspectionCancelled());
    final token = _epoch, pending = Completer<Object>(), port = ReceivePort();
    _pending = pending;
    _port = port;
    port.listen((message) {
      if (_disposed || token != _epoch || !identical(_pending, pending)) return;
      _pending = null;
      _port = null;
      port.close();
      _isolate?.kill(priority: Isolate.immediate);
      _isolate = null;
      if (message is GrafanaInspection || message is GrafanaConstantReview) {
        pending.complete(message);
      } else if (message is GrafanaConstantProblem) {
        pending.completeError(GrafanaConstantException(message));
      } else {
        pending.completeError(
          GrafanaImportException(message is GrafanaImportProblem ? message : GrafanaImportProblem.invalidDashboard),
        );
      }
    });
    unawaited(_spawn(input, port.sendPort, token, pending, panelPath: panelPath, values: values));
    return pending.future.then<T>((value) {
      if (value is! T) throw const GrafanaImportException(GrafanaImportProblem.invalidDashboard);
      return value as T;
    });
  }

  Future<void> _spawn(
    String input,
    SendPort reply,
    int token,
    Completer<Object> pending, {
    String? panelPath,
    Map<String, String>? values,
  }) async {
    try {
      final isolate = await Isolate.spawn(
        _inspectDashboard,
        (reply, input, panelPath, values),
        onError: reply,
        errorsAreFatal: true,
      );
      if (_disposed || token != _epoch || !identical(_pending, pending)) {
        isolate.kill(priority: Isolate.immediate);
      } else {
        _isolate = isolate;
      }
    } catch (_) {
      if (token != _epoch || !identical(_pending, pending)) return;
      _pending = null;
      _port?.close();
      _port = null;
      pending.completeError(const GrafanaImportException(GrafanaImportProblem.invalidDashboard));
    }
  }

  @override
  void cancel() {
    _epoch++;
    final pending = _pending;
    _pending = null;
    _isolate?.kill(priority: Isolate.immediate);
    _isolate = null;
    _port?.close();
    _port = null;
    pending?.completeError(const GrafanaInspectionCancelled());
  }

  @override
  void dispose() {
    cancel();
    _disposed = true;
  }
}
