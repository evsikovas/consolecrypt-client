import 'package:consolecrypt/core/mock/mock_cloud.dart';
import 'package:consolecrypt/core/mock/mock_config.dart';
import 'package:consolecrypt/core/mock/mock_scope.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/services/app_services.dart';
import 'package:consolecrypt/core/util/value_stream.dart';

final class MockSettingsService extends VaultScopedMock implements SettingsService {
  MockSettingsService(super.cloud) {
    initScope();
  }

  final ValueStreamController<LocalSettings> _local = ValueStreamController(const LocalSettings());
  final ValueStreamController<VaultSettings?> _vault = ValueStreamController(null);

  @override
  void onDataChanged(MockVaultData? data) => _vault.value = data?.settings;

  @override
  Stream<LocalSettings> watchLocal() => _local.stream;

  @override
  LocalSettings get currentLocal => _local.value;

  @override
  Future<void> updateLocal(LocalSettings settings) async => _local.value = settings;

  @override
  Stream<VaultSettings?> watchVault() => _vault.stream;

  @override
  Future<void> updateVault(VaultSettings settings) async {
    await mockDelay(cloud.config.latency);
    final d = data..settings = settings;
    final keys = cloud.activeRecord?.vault;
    if (keys != null) keys.name = settings.vaultName;
    _vault.value = d.settings;
    cloud.recordMutation();
    cloud.notifyChanged();
  }

  Future<void> dispose() async {
    await disposeScope();
    await _local.close();
    await _vault.close();
  }
}
