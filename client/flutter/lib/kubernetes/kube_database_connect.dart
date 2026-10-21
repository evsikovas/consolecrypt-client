import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/db/db_connection_dialog.dart';
import 'package:consolecrypt/db/db_models.dart';
import 'package:consolecrypt/db/db_providers.dart';
import 'package:consolecrypt/kubernetes/kube_browser_service.dart';
import 'package:consolecrypt/kubernetes/kube_forward_service.dart';
import 'package:consolecrypt/kubernetes/kube_service.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import 'package:material_ui/material_ui.dart';

/// A database a pod runs, guessed from its image, ports and plain env values.
final class KubeDbTarget {
  const KubeDbTarget({required this.engine, required this.remotePort, this.database = '', this.user = ''});
  final DbEngine engine;
  final int remotePort;
  final String database, user;
}

// Image fragments → engine; env names holding the database and the user.
const _images = <(String, DbEngine)>[
  ('postgis', DbEngine.postgresql), // l10n-ignore: image name
  ('timescale', DbEngine.postgresql), // l10n-ignore: image name
  ('postgres', DbEngine.postgresql), // l10n-ignore: image name
  ('mariadb', DbEngine.mariadb), // l10n-ignore: image name
  ('mysql', DbEngine.mysql), // l10n-ignore: image name
  ('clickhouse', DbEngine.clickhouse), // l10n-ignore: image name
  ('valkey', DbEngine.redis), // l10n-ignore: image name
  ('keydb', DbEngine.redis), // l10n-ignore: image name
  ('redis', DbEngine.redis), // l10n-ignore: image name
  ('mongo', DbEngine.mongodb), // l10n-ignore: image name
];
const _ports = {
  5432: DbEngine.postgresql,
  3306: DbEngine.mysql,
  8123: DbEngine.clickhouse,
  6379: DbEngine.redis,
  27017: DbEngine.mongodb,
};
const _databaseEnv = [
  'POSTGRES_DB', // l10n-ignore: env name
  'POSTGRESQL_DATABASE', // l10n-ignore: env name
  'MYSQL_DATABASE', // l10n-ignore: env name
  'MARIADB_DATABASE', // l10n-ignore: env name
  'CLICKHOUSE_DB', // l10n-ignore: env name
  'MONGO_INITDB_DATABASE', // l10n-ignore: env name
];
const _userEnv = [
  'POSTGRES_USER', // l10n-ignore: env name
  'POSTGRESQL_USERNAME', // l10n-ignore: env name
  'MYSQL_USER', // l10n-ignore: env name
  'MARIADB_USER', // l10n-ignore: env name
  'CLICKHOUSE_USER', // l10n-ignore: env name
  'MONGO_INITDB_ROOT_USERNAME', // l10n-ignore: env name
];

int _defaultPort(DbEngine engine) => engine == DbEngine.clickhouse ? 8123 : engine.port;

/// The first container that looks like a database, or null.
KubeDbTarget? kubeDatabaseTarget(KubeJson pod) {
  final containers = ((pod['spec'] as Map<String, dynamic>?)?['containers'] as List?) ?? const [];
  for (final container in containers.whereType<Map<String, dynamic>>()) {
    final image = (container['image'] as String? ?? '').toLowerCase();
    final ports = [
      for (final port in (container['ports'] as List? ?? const []).whereType<Map<String, dynamic>>())
        if (port['containerPort'] case final int value) value,
    ];
    var engine = _images.where((entry) => image.contains(entry.$1)).map((entry) => entry.$2).firstOrNull;
    engine ??= ports.map((port) => _ports[port]).nonNulls.firstOrNull;
    if (engine == null) continue;
    final wanted = _defaultPort(engine);
    final remote = ports.contains(wanted) || ports.isEmpty
        ? wanted
        : (ports.where((port) => _ports[port] == engine).firstOrNull ?? ports.first);
    String env(List<String> names) {
      for (final entry in (container['env'] as List? ?? const []).whereType<Map<String, dynamic>>()) {
        // Only literal values: secrets referenced with valueFrom stay in the cluster.
        if (names.contains(entry['name']) && entry['value'] is String) return entry['value'] as String;
      }
      return '';
    }

    return KubeDbTarget(engine: engine, remotePort: remote, database: env(_databaseEnv), user: env(_userEnv));
  }
  return null;
}

/// Forwards the pod's database port to localhost (or reuses a running
/// forward), then opens the matching saved connection or a prefilled form
/// where only the password is left.
Future<void> connectKubeDatabase(
  BuildContext context,
  WidgetRef ref, {
  required KubeSession session,
  required String namespace,
  required String pod,
  required KubeForwardService service,
  required KubeDbTarget target,
}) async {
  final running = (await service.forwards()).where(
    (forward) =>
        forward.sessionId == session.id &&
        forward.namespace == namespace &&
        forward.pod == pod &&
        forward.remotePort == target.remotePort,
  );
  KubeForward forward;
  if (running.isNotEmpty) {
    forward = running.first;
  } else {
    // A stable local port keeps the saved connection usable next time.
    try {
      forward = await service.startForward(
        session.id,
        namespace: namespace,
        pod: pod,
        remotePort: target.remotePort,
        localPort: 10000 + _defaultPort(target.engine) % 55000,
      );
    } catch (_) {
      forward = await service.startForward(session.id, namespace: namespace, pod: pod, remotePort: target.remotePort);
    }
  }
  final port = int.tryParse(forward.address.split(':').last) ?? 0;
  if (!context.mounted || port == 0) return;
  final databases = ref.read(dbServiceProvider);
  if (databases == null) return;
  final saved = (await databases.connections()).where(
    (c) => c.engine == target.engine && c.port == port && (c.host == '127.0.0.1' || c.host == 'localhost'),
  );
  DbConnection? connection = saved.firstOrNull;
  if (connection == null) {
    if (!context.mounted) return;
    connection = await showAppDialog<DbConnection>(
      context,
      secure: true,
      builder: (_) => DbConnectionDialog(
        template: DbConnection(
          id: '',
          name: '$namespace/$pod',
          engine: target.engine,
          host: '127.0.0.1',
          port: port,
          database: target.database,
          username: target.user,
          production: session.production,
          tls: const DbTls(mode: DbTlsMode.prefer),
          options: DbDriverOptions(engine: target.engine),
          tags: const ['kubernetes'], // l10n-ignore: tag
        ),
      ),
    );
  }
  if (connection == null || !context.mounted) return;
  ref.read(dbOpenRequestProvider.notifier).open(connection.id);
  context.go(AppRoutes.databases);
}
