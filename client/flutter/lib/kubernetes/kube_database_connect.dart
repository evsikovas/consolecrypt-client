import 'package:consolecrypt/db/db_forward_connect.dart';
import 'package:consolecrypt/db/db_models.dart';
import 'package:consolecrypt/db/db_port_detect.dart';
import 'package:consolecrypt/kubernetes/kube_browser_service.dart';
import 'package:consolecrypt/kubernetes/kube_relations_service.dart';
import 'package:consolecrypt/kubernetes/kube_service.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

/// A database a pod runs, guessed from its image, ports and plain env values.
final class KubeDbTarget {
  const KubeDbTarget({required this.engine, required this.remotePort, this.database = '', this.user = ''});
  final DbEngine engine;
  final int remotePort;
  final String database, user;
}

/// The first container that looks like a database, or null.
KubeDbTarget? kubeDatabaseTarget(KubeJson pod) {
  final containers = ((pod['spec'] as Map<String, dynamic>?)?['containers'] as List?) ?? const [];
  for (final container in containers.whereType<Map<String, dynamic>>()) {
    final image = container['image'] as String? ?? '';
    final ports = [
      for (final port in (container['ports'] as List? ?? const []).whereType<Map<String, dynamic>>())
        if (port['containerPort'] case final int value) value,
    ];
    final engine = dbEngineForImage(image) ?? ports.map(dbEngineForPort).nonNulls.firstOrNull;
    if (engine == null) continue;
    final wanted = dbForwardPort(engine);
    final remote = ports.contains(wanted) || ports.isEmpty
        ? wanted
        : (ports.where((port) => dbEngineForPort(port) == engine).firstOrNull ?? ports.first);
    String env(List<String> names) {
      for (final entry in (container['env'] as List? ?? const []).whereType<Map<String, dynamic>>()) {
        // Only literal values: secrets referenced with valueFrom stay in the cluster.
        if (names.contains(entry['name']) && entry['value'] is String) return entry['value'] as String;
      }
      return '';
    }

    return KubeDbTarget(engine: engine, remotePort: remote, database: env(dbDatabaseEnv), user: env(dbUserEnv));
  }
  return null;
}

/// What a saved connection forwards to: the pod's StatefulSet pod (stable
/// name), else the only Service selecting it, else its Deployment, else the
/// pod itself. Services and Deployments survive pod restarts.
Future<(DbKubeTargetKind, String)> kubeDatabaseRouteTarget(
  KubeJson pod,
  String name, {
  Future<List<String>> Function()? services,
}) async {
  final metadata = pod['metadata'] as Map<String, dynamic>? ?? const {};
  final owners = (metadata['ownerReferences'] as List? ?? const []).whereType<Map<String, dynamic>>();
  final owner = owners.where((o) => o['controller'] == true).firstOrNull ?? owners.firstOrNull;
  if (owner?['kind'] == 'StatefulSet') return (DbKubeTargetKind.pod, name);
  if (services != null) {
    try {
      final selecting = await services().timeout(const Duration(seconds: 5));
      if (selecting.length == 1) return (DbKubeTargetKind.service, selecting.single);
    } catch (_) {
      // Best effort: fall back to the owner or the pod.
    }
  }
  final hash = (metadata['labels'] as Map<String, dynamic>?)?['pod-template-hash'];
  final replicaSet = owner?['kind'] == 'ReplicaSet' ? owner!['name'] as String? : null;
  if (replicaSet != null && hash is String && hash.isNotEmpty && replicaSet.endsWith('-$hash')) {
    return (DbKubeTargetKind.deployment, replicaSet.substring(0, replicaSet.length - hash.length - 1));
  }
  return (DbKubeTargetKind.pod, name);
}

/// Opens the matching saved connection or a prefilled form where only the
/// password is left. The connection keeps a Kubernetes route (context,
/// namespace, workload and container port), so connecting later restores the
/// port-forward instead of relying on a local port from this run.
Future<void> connectKubeDatabase(
  BuildContext context,
  WidgetRef ref, {
  required KubeSession session,
  required KubeResourceRef resource,
  required KubeJson object,
  required KubeDbTarget target,
  KubeRelationsService? relations,
}) async {
  final namespace = resource.namespace ?? '';
  final (kind, name) = await kubeDatabaseRouteTarget(
    object,
    resource.name,
    services: relations == null
        ? null
        : () async {
            final related = await relations.relatedObjects(session.id, resource);
            return [
              for (final item in (related['items'] as List? ?? const []).whereType<Map<String, dynamic>>())
                if (item['relation'] == 'service') // l10n-ignore: wire value
                  if ((item['reference'] as Map<String, dynamic>?)?['name'] case final String service) service,
            ];
          },
  );
  if (!context.mounted) return;
  await openPrefilledDatabase(
    context,
    ref,
    engine: target.engine,
    port: target.remotePort,
    route: DbRoute(
      kind: DbRouteKind.kubernetes,
      reference: session.contextId,
      namespace: namespace,
      targetKind: kind,
      targetName: name,
    ),
    name: '$namespace/$name',
    database: target.database,
    user: target.user,
    production: session.production,
    tag: 'kubernetes', // l10n-ignore: tag
  );
}
