/// Guessing which database a container runs from its image, ports and env,
/// shared by the Kubernetes pods and the Docker containers.
library;

import 'package:consolecrypt/db/db_models.dart';

// Image fragments → engine (the more specific fragments first).
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

/// Standard database ports → engine.
const dbStandardPorts = {
  5432: DbEngine.postgresql,
  3306: DbEngine.mysql,
  8123: DbEngine.clickhouse,
  6379: DbEngine.redis,
  27017: DbEngine.mongodb,
};

/// Environment variables holding the database name and the user.
const dbDatabaseEnv = [
  'POSTGRES_DB', // l10n-ignore: env name
  'POSTGRESQL_DATABASE', // l10n-ignore: env name
  'MYSQL_DATABASE', // l10n-ignore: env name
  'MARIADB_DATABASE', // l10n-ignore: env name
  'CLICKHOUSE_DB', // l10n-ignore: env name
  'MONGO_INITDB_DATABASE', // l10n-ignore: env name
];
const dbUserEnv = [
  'POSTGRES_USER', // l10n-ignore: env name
  'POSTGRESQL_USERNAME', // l10n-ignore: env name
  'MYSQL_USER', // l10n-ignore: env name
  'MARIADB_USER', // l10n-ignore: env name
  'CLICKHOUSE_USER', // l10n-ignore: env name
  'MONGO_INITDB_ROOT_USERNAME', // l10n-ignore: env name
];

/// The engine an image name points to, or null.
DbEngine? dbEngineForImage(String image) {
  final lower = image.toLowerCase();
  return _images.where((entry) => lower.contains(entry.$1)).map((entry) => entry.$2).firstOrNull;
}

/// The engine usually listening on [port], or null.
DbEngine? dbEngineForPort(int port) => dbStandardPorts[port];

/// The container port a forward targets for [engine] (ClickHouse over HTTP).
int dbForwardPort(DbEngine engine) => engine == DbEngine.clickhouse ? 8123 : engine.port;
