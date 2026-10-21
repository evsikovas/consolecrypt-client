import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/db/db_connection_dialog.dart';
import 'package:consolecrypt/db/db_models.dart';
import 'package:consolecrypt/db/db_providers.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import 'package:material_ui/material_ui.dart';

const _loopback = {'127.0.0.1', 'localhost'}; // l10n-ignore: host names

/// Opens the database at [host]:[port] reached through [route]: the matching
/// saved connection, or a prefilled form where only the database, login and
/// password are left, then switches to the Databases section.
Future<void> openPrefilledDatabase(
  BuildContext context,
  WidgetRef ref, {
  required DbEngine engine,
  required int port,
  required String name,
  required String tag,
  String host = '127.0.0.1',
  DbRoute route = const DbRoute(),
  String database = '',
  String user = '',
  bool production = false,
}) async {
  if (port == 0) return;
  final databases = ref.read(dbServiceProvider);
  if (databases == null) return;
  bool sameHost(String other) => other == host || (_loopback.contains(other) && _loopback.contains(host));
  final saved = (await databases.connections()).where(
    (c) => c.engine == engine && c.port == port && sameHost(c.host) && c.route.sameAs(route),
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
          name: name,
          engine: engine,
          host: host,
          port: port,
          route: route,
          database: database,
          username: user,
          production: production,
          tls: const DbTls(mode: DbTlsMode.prefer),
          options: DbDriverOptions(engine: engine),
          tags: [tag],
        ),
      ),
    );
  }
  if (connection == null || !context.mounted) return;
  ref.read(dbOpenRequestProvider.notifier).open(connection.id);
  context.go(AppRoutes.databases);
}
