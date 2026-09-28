import 'package:consolecrypt/core/models/models.dart';

/// Port-forwarding profiles (synced) and their runtime (tunnel-core, local).
abstract interface class TunnelService {
  Stream<List<Tunnel>> watchTunnels();

  Future<Tunnel> saveTunnel(Tunnel tunnel);

  /// Stops the tunnel first if running.
  Future<void> deleteTunnel(ObjectId id);

  Future<void> start(ObjectId id);

  Future<void> stop(ObjectId id);

  /// Runtime by tunnel id; missing ids are stopped.
  Stream<Map<ObjectId, TunnelRuntime>> watchRuntime();
}
