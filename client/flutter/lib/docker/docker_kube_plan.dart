import 'dart:async';
import 'dart:convert';
import 'dart:isolate';

import 'package:consolecrypt/docker/docker_service.dart';
import 'package:consolecrypt/kubernetes/kube_browser_service.dart';
import 'package:yaml/yaml.dart';

enum DockerKubeBlocker { mounts, unsupportedConfiguration }

/// Only these scalar fields may leave the Docker boundary for generation.
/// Never retain inspect, environment, command arguments or labels here.
final class DockerKubeSource {
  const DockerKubeSource({required this.ports, this.blocker});
  final List<int> ports;
  final DockerKubeBlocker? blocker;
  bool get unsupported => blocker != null;

  factory DockerKubeSource.inspect(DockerContainerDetail detail, DockerSessionInfo session) {
    final host = detail.inspect['HostConfig'];
    final config = host is Map ? host : const <String, Object?>{};
    final mounts = detail.inspect['Mounts'];
    bool hasEntries(String key) => config[key] is List && (config[key] as List).isNotEmpty;
    final ports =
        detail.container.ports
            .where((p) => p.protocol == 'tcp' && p.privatePort > 0 && p.privatePort <= 65535)
            .map((p) => p.privatePort)
            .toSet()
            .toList()
          ..sort();
    DockerKubeBlocker? blocker;
    // Fail closed when inspect cannot establish that this is stateless.
    if ((mounts is List && mounts.isNotEmpty) || hasEntries('Binds') || hasEntries('VolumesFrom')) {
      blocker = DockerKubeBlocker.mounts;
    } else if (session.os.toLowerCase() != 'linux' ||
        host is! Map ||
        mounts is! List ||
        config['Privileged'] == true ||
        const ['host', 'container'].any((s) => '${config['NetworkMode']}'.startsWith(s)) ||
        config['PidMode'] == 'host' ||
        config['IpcMode'] == 'host' ||
        const ['Devices', 'DeviceRequests', 'CapAdd', 'ExtraHosts'].any(hasEntries)) {
      blocker = DockerKubeBlocker.unsupportedConfiguration;
    }
    return DockerKubeSource(ports: List.unmodifiable(ports), blocker: blocker);
  }
}

final class DockerKubeTarget {
  const DockerKubeTarget({
    required this.name,
    required this.namespace,
    required this.image,
    required this.port,
    this.pullSecret = '',
  });
  final String name, namespace, image, pullSecret;
  final int port;

  static bool validName(String value) => value.length <= 63 && RegExp(r'^[a-z]([a-z0-9-]*[a-z0-9])?$').hasMatch(value);

  /// Explicit registry and immutable digest or explicit tag; no credentials,
  /// URLs, local image IDs or implicit latest. Reachability is checked by Kube.
  static bool validImage(String value) {
    if (value.length > 512 ||
        !RegExp(
          r'^[a-z0-9][a-z0-9.:-]*/[a-z0-9][a-z0-9._/-]*(?::[A-Za-z0-9_][A-Za-z0-9_.-]{0,127}|@sha256:[a-f0-9]{64})$',
        ).hasMatch(value)) {
      return false;
    }
    final registry = value.split('/').first.split(':').first;
    return registry.contains('.') &&
        !const ['localhost', '127.0.0.1', '0.0.0.0'].contains(registry) &&
        !value.contains('//') &&
        !value.contains('/../');
  }

  bool get valid =>
      validName(name) &&
      validName(namespace) &&
      validImage(image) &&
      port > 0 &&
      port <= 65535 &&
      (pullSecret.isEmpty || validName(pullSecret));

  Map<String, dynamic> get deployment => {
    'apiVersion': 'apps/v1',
    'kind': 'Deployment',
    'metadata': {'name': name, 'namespace': namespace},
    'spec': {
      'replicas': 1,
      'selector': {
        'matchLabels': {'app.kubernetes.io/name': name},
      },
      'template': {
        'metadata': {
          'labels': {'app.kubernetes.io/name': name},
        },
        'spec': {
          'automountServiceAccountToken': false,
          if (pullSecret.isNotEmpty)
            'imagePullSecrets': [
              {'name': pullSecret},
            ],
          'containers': [
            {
              'name': name,
              'image': image,
              'imagePullPolicy': 'Always',
              'ports': [
                {'containerPort': port, 'protocol': 'TCP'},
              ],
              'securityContext': {
                'allowPrivilegeEscalation': false,
                'capabilities': {
                  'drop': ['ALL'],
                },
              },
              'resources': {
                'requests': {'cpu': '100m', 'memory': '128Mi'},
                'limits': {'cpu': '1', 'memory': '512Mi'},
              },
              'readinessProbe': {
                'tcpSocket': {'port': port},
                'initialDelaySeconds': 5,
                'periodSeconds': 5,
              },
            },
          ],
        },
      },
    },
  };

  Map<String, dynamic> get service => {
    'apiVersion': 'v1',
    'kind': 'Service',
    'metadata': {'name': name, 'namespace': namespace},
    'spec': {
      'type': 'ClusterIP',
      'selector': {'app.kubernetes.io/name': name},
      'ports': [
        {'port': port, 'targetPort': port, 'protocol': 'TCP'},
      ],
    },
  };

  String get template =>
      '${const JsonEncoder.withIndent('  ').convert(deployment)}\n---\n${const JsonEncoder.withIndent('  ').convert(service)}';

  // Template contains only user-reviewed deployment settings. No raw Docker or
  // Kubernetes context, container name, environment, secrets or kubeconfig.
  String get prompt =>
      'Write exactly two Kubernetes YAML documents: Deployment and Service. '
      'Use the following template. Keep every field and identity unchanged except container resources '
      '(requests and limits: cpu and memory) and readinessProbe timing (1..300 seconds). '
      'Do not add any other fields, containers, commands, environment variables or resources. '
      'Return YAML only, no explanation. This is an experimental stateless deployment; '
      'there is no measured usage data, so keep conservative resource defaults.\n$template';
}

final class DockerKubeDocument {
  const DockerKubeDocument(this.resource, this.manifest);
  final KubeResourceRef resource;
  final String manifest;
}

/// AI/YAML is untrusted. An expendable isolate bounds parsing, aliases and
/// nesting; only the constrained Deployment/Service pair crosses back.
final class DockerKubeParser {
  Isolate? _worker;
  ReceivePort? _port;
  Timer? _timer;
  Completer<List<String>?>? _result;

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

  Future<List<DockerKubeDocument>?> parse(String text, DockerKubeTarget target) async {
    cancel();
    if (!target.valid || text.length > 65536) return null;
    final result = _result = Completer<List<String>?>();
    final port = _port = ReceivePort();
    port.listen((value) {
      if (!identical(result, _result)) return;
      result.complete(value is List<String> && value.length == 2 ? value : null);
      cancel();
    });
    try {
      final worker = await Isolate.spawn(
        _parse,
        (port.sendPort, text, target),
        onError: port.sendPort,
        onExit: port.sendPort,
      );
      if (!identical(result, _result)) {
        worker.kill(priority: Isolate.immediate);
      } else {
        _worker = worker;
        _timer = Timer(const Duration(seconds: 2), cancel);
      }
    } catch (_) {
      cancel();
    }
    final documents = await result.future;
    if (documents == null) return null;
    return [
      DockerKubeDocument(
        KubeResourceRef(api: deploymentKind, name: target.name, namespace: target.namespace),
        documents[0],
      ),
      DockerKubeDocument(
        KubeResourceRef(api: serviceKind, name: target.name, namespace: target.namespace),
        documents[1],
      ),
    ];
  }

  static const deploymentKind = KubeResourceKind(
    group: 'apps',
    version: 'v1',
    resource: 'deployments',
    kind: 'Deployment',
    namespaced: true,
  );
  static const serviceKind = KubeResourceKind(
    group: '',
    version: 'v1',
    resource: 'services',
    kind: 'Service',
    namespaced: true,
  );
  static const namespaceKind = KubeResourceKind(
    group: '',
    version: 'v1',
    resource: 'namespaces',
    kind: 'Namespace',
    namespaced: false,
  );

  static void _parse((SendPort, String, DockerKubeTarget) request) {
    final (port, raw, target) = request;
    try {
      var text = raw.trim();
      if (text.startsWith('```yaml\n') || text.startsWith('```yml\n') || text.startsWith('```json\n')) {
        if (!text.endsWith('```')) {
          port.send(null);
          return;
        }
        text = text.substring(text.indexOf('\n') + 1, text.length - 3).trim();
      }
      // Anchors/aliases are unnecessary for this small, constrained manifest.
      if (RegExp(r'(^|\s)[&*][^\s]+', multiLine: true).hasMatch(text)) {
        port.send(null);
        return;
      }
      final docs = loadYamlDocuments(text);
      if (docs.length != 2) {
        port.send(null);
        return;
      }
      final objects = docs.map((d) => jsonDecode(jsonEncode(d.contents.value)) as Map<String, dynamic>).toList();
      final deployment = objects.singleWhere((d) => d['kind'] == 'Deployment');
      final service = objects.singleWhere((d) => d['kind'] == 'Service');
      final original = target.deployment;
      Map<String, dynamic> containerOf(Map<String, dynamic> object) {
        final spec = object['spec'] as Map<String, dynamic>;
        final template = spec['template'] as Map<String, dynamic>;
        final pod = template['spec'] as Map<String, dynamic>;
        return (pod['containers'] as List<dynamic>).first as Map<String, dynamic>;
      }

      final container = containerOf(deployment);
      final baseline = containerOf(original);
      final resources = container['resources'];
      if (resources is! Map || resources.length != 2 || !resources.keys.toSet().containsAll(['requests', 'limits'])) {
        port.send(null);
        return;
      }
      for (final part in ['requests', 'limits']) {
        final quantities = resources[part];
        if (quantities is! Map ||
            quantities.length != 2 ||
            !quantities.keys.toSet().containsAll(['cpu', 'memory']) ||
            quantities.values.any(
              (v) => v is! String || v.length > 16 || !RegExp(r'^[1-9][0-9]*(m|Ki|Mi|Gi)?$').hasMatch(v),
            )) {
          port.send(null);
          return;
        }
      }
      final probe = container['readinessProbe'];
      if (probe is! Map ||
          !probe.containsKey('tcpSocket') ||
          probe.keys.any(
            (k) => ![
              'tcpSocket',
              'initialDelaySeconds',
              'periodSeconds',
              'timeoutSeconds',
              'failureThreshold',
              'successThreshold',
            ].contains(k),
          ) ||
          !_equal(probe['tcpSocket'], {'port': target.port}) ||
          probe.entries.any(
            (e) => e.key != 'tcpSocket' && (e.value is! int || (e.value as int) < 1 || (e.value as int) > 300),
          )) {
        port.send(null);
        return;
      }
      final encoded = [jsonEncode(deployment), jsonEncode(service)];
      container['resources'] = baseline['resources'];
      container['readinessProbe'] = baseline['readinessProbe'];
      if (!_equal(deployment, original) || !_equal(service, target.service)) {
        port.send(null);
        return;
      }
      port.send(encoded);
    } catch (_) {
      port.send(null);
    }
  }

  static bool _equal(dynamic a, dynamic b) {
    if (a is Map && b is Map) {
      return a.length == b.length && a.keys.every((k) => b.containsKey(k) && _equal(a[k], b[k]));
    }
    if (a is List && b is List) {
      return a.length == b.length && List.generate(a.length, (i) => i).every((i) => _equal(a[i], b[i]));
    }
    return a == b;
  }
}
