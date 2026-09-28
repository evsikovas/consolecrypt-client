import 'package:consolecrypt/core/mock/mock_cloud.dart';
import 'package:consolecrypt/core/mock/mock_config.dart';
import 'package:consolecrypt/core/mock/mock_scope.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/services/app_services.dart';
import 'package:consolecrypt/core/util/value_stream.dart';

final class MockTunnelService extends VaultScopedMock implements TunnelService {
  MockTunnelService(super.cloud) {
    initScope();
  }

  final ValueStreamController<List<Tunnel>> _tunnels = ValueStreamController(const []);
  final ValueStreamController<Map<ObjectId, TunnelRuntime>> _runtime = ValueStreamController(const {});

  MockConfig get _config => cloud.config;

  @override
  void onDataChanged(MockVaultData? data) {
    _tunnels.value = List.unmodifiable(data?.tunnels ?? const <Tunnel>[]);
    // Tunnels never outlive the unlocked vault that owns them.
    if (data == null) _runtime.value = const {};
  }

  void _setRuntime(ObjectId id, TunnelRuntime runtime) {
    _runtime.value = {..._runtime.value, id: runtime};
  }

  @override
  Stream<List<Tunnel>> watchTunnels() => _tunnels.stream;

  @override
  Future<Tunnel> saveTunnel(Tunnel tunnel) async {
    final error = tunnel.validate();
    if (error != null) throw AppException.fromValidation(error);
    await mockDelay(_config.latency);
    final list = data.tunnels;
    final i = list.indexWhere((t) => t.id == tunnel.id);
    if (i >= 0) {
      list[i] = tunnel;
    } else {
      list.add(tunnel);
    }
    onDataChanged(dataOrNull);
    cloud.recordMutation();
    return tunnel;
  }

  @override
  Future<void> deleteTunnel(ObjectId id) async {
    await stop(id);
    data.tunnels.removeWhere((t) => t.id == id);
    _runtime.value = {..._runtime.value}..remove(id);
    onDataChanged(dataOrNull);
    cloud.recordMutation();
  }

  @override
  Future<void> start(ObjectId id) async {
    final tunnel = data.tunnels.where((t) => t.id == id).firstOrNull;
    if (tunnel == null) {
      throw const AppException(AppErrorCode.notFound, 'Tunnel not found', reason: AppErrorReason.tunnelNotFound);
    }
    _setRuntime(id, const TunnelRuntime(state: TunnelRunState.starting));
    await mockDelay(_config.latency * 2);
    final hostExists = data.hosts.any((h) => h.id == tunnel.hostId);
    if (!hostExists) {
      _setRuntime(id, const TunnelRuntime(state: TunnelRunState.failed, error: 'The carrier host was deleted'));
      return;
    }
    if (tunnel.kind != TunnelKind.remote && tunnel.bindPort < 1024) {
      _setRuntime(
        id,
        TunnelRuntime(
          state: TunnelRunState.failed,
          error: 'Cannot bind ${tunnel.bindHost}:${tunnel.bindPort}: ports below 1024 need administrator rights',
        ),
      );
      return;
    }
    final inUse = _runtime.value.entries.any((e) {
      if (e.key == id || e.value.state != TunnelRunState.running) return false;
      final other = data.tunnels.where((t) => t.id == e.key).firstOrNull;
      return other != null && other.kind != TunnelKind.remote && other.bindPort == tunnel.bindPort;
    });
    if (inUse && tunnel.kind != TunnelKind.remote) {
      _setRuntime(id, TunnelRuntime(state: TunnelRunState.failed, error: 'Port ${tunnel.bindPort} is already in use'));
      return;
    }
    _setRuntime(id, TunnelRuntime(state: TunnelRunState.running, since: DateTime.now(), activeConnections: 1));
  }

  @override
  Future<void> stop(ObjectId id) async {
    if (_runtime.value[id]?.state == TunnelRunState.stopped || !_runtime.value.containsKey(id)) return;
    await mockDelay(_config.latency);
    _setRuntime(id, TunnelRuntime.stopped);
  }

  @override
  Stream<Map<ObjectId, TunnelRuntime>> watchRuntime() => _runtime.stream;

  Future<void> dispose() async {
    await disposeScope();
    await _tunnels.close();
    await _runtime.close();
  }
}
