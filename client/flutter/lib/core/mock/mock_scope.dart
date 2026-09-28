import 'dart:async';

import 'package:consolecrypt/core/mock/mock_cloud.dart';
import 'package:consolecrypt/core/services/app_services.dart';

/// Base for mock services whose data belongs to the active unlocked vault.
/// Reloads (and re-emits) whenever the active working set changes — profile
/// switch, lock/unlock, restore.
abstract base class VaultScopedMock {
  VaultScopedMock(this.cloud) {
    _sub = cloud.changed.listen((_) => _check());
  }

  final MockCloud cloud;
  late final StreamSubscription<void> _sub;
  MockVaultData? _data;
  bool _initialised = false;

  /// Call at the end of the subclass constructor.
  void initScope() {
    _initialised = true;
    _data = cloud.activeData;
    onDataChanged(_data);
  }

  void _check() {
    if (!_initialised) return;
    final next = cloud.activeData;
    if (identical(next, _data)) return;
    _data = next;
    onDataChanged(next);
  }

  /// Re-emit all streams from [data] (`null` = locked: emit empty).
  void onDataChanged(MockVaultData? data);

  /// Working set for mutations; throws while locked.
  MockVaultData get data {
    final d = _data;
    if (d == null) {
      throw const AppException(AppErrorCode.notFound, 'The vault is locked', reason: AppErrorReason.vaultLocked);
    }
    return d;
  }

  MockVaultData? get dataOrNull => _data;

  Future<void> disposeScope() => _sub.cancel();
}
