import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/db/db_models.dart';

final class DbRouteChoice {
  const DbRouteChoice({required this.route, required this.label});
  final DbRoute route;
  final String label;
}

final class DbGroupChoice {
  const DbGroupChoice({required this.id, required this.label});
  final String id, label;
}

final class DbConnectionChoices {
  const DbConnectionChoices({this.routes = const [], this.groups = const []});
  final List<DbRouteChoice> routes;
  final List<DbGroupChoice> groups;
}

final class DbImportEntry {
  const DbImportEntry({
    required this.index,
    required this.name,
    required this.engine,
    required this.host,
    required this.port,
    this.database = '',
    this.username = '',
    this.hasPassword = false,
    this.warningCodes = const [],
    this.errorCode,
  });
  final int index, port;
  final String name, host, database, username;
  final DbEngine engine;
  final bool hasPassword;
  final List<String> warningCodes;
  final String? errorCode;
}

final class DbImportReview {
  const DbImportReview({required this.ticket, required this.expiresAt, required this.entries});
  final String ticket;
  final DateTime expiresAt;
  final List<DbImportEntry> entries;
}

final class DbImportChoice {
  const DbImportChoice({
    required this.index,
    this.production = true,
    this.savePassword = true,
    this.route = const DbRoute(),
  });
  final int index;
  final bool production, savePassword;
  final DbRoute route;
  DbJson toJson() => {'index': index, 'production': production, 'save_password': savePassword, 'route': route.toJson()};
}

final class DbConnectionCheck {
  const DbConnectionCheck({required this.serverVersion, required this.serverReadOnly, this.warningCodes = const []});
  final String serverVersion;
  final DbServerReadOnly serverReadOnly;
  final List<String> warningCodes;
}

/// Every implementation must scope tickets and requests to the unlocked profile.
/// Import previews contain metadata only; the password remains native.
abstract interface class DbService {
  Stream<void> changes();
  Future<List<DbConnection>> connections();
  Future<DbConnectionChoices> connectionChoices();
  Future<DbConnection> saveConnection(DbConnectionDraft draft);
  Future<void> deleteConnection(String id);
  Future<DbConnectionCheck> testConnection(DbConnectionDraft draft);
  Future<DbImportReview> importConnectionString(SecretText uri);
  Future<DbImportReview> importPgpass(SecretText contents);
  Future<void> confirmImport(String ticket, List<DbImportChoice> choices);
  Future<void> discardImport(String ticket);
}

/// Optional platform capability. Reads the user's explicitly selected local
/// .pgpass file natively; returns only an opaque preview ticket and safe metadata.
abstract interface class DbPgpassFileService {
  Future<DbImportReview?> selectPgpassFile();
}
