import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/db/db_models.dart';
import 'package:consolecrypt/db/db_service.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

/// Root installs the native adapter. Null is an explicit unavailable capability,
/// not a mock connection or an empty successful backend response.
final dbServiceProvider = Provider<DbService?>((ref) => ref.watch(appServicesProvider).databases);

final class DbScope {
  const DbScope(this.profileId, this.unlocked);
  final Object? profileId;
  final bool unlocked;
}

final dbScopeProvider = Provider<DbScope>(
  (ref) =>
      DbScope(ref.watch(activeProfileProvider.select((p) => p?.id)), ref.watch(appStageProvider) == AppStage.unlocked),
);
final _changes = StreamProvider.autoDispose<void>(
  (ref) => ref.watch(dbServiceProvider)?.changes() ?? const Stream.empty(),
);
final dbConnectionsProvider = FutureProvider.autoDispose.family<List<DbConnection>, DbScope>((ref, scope) async {
  if (!scope.unlocked) return const [];
  ref.watch(_changes);
  return ref.watch(dbServiceProvider)?.connections() ?? const [];
});
