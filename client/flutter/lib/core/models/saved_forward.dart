/// Device-local forwarding presets. Credentials remain in the host/cluster
/// inventory; runtime session ids and network approvals are never persisted.
enum SavedForwardKind { docker, kubernetes }

final class SavedForward {
  const SavedForward({
    required this.id,
    required this.profile,
    required this.kind,
    required this.connectionId,
    required this.target,
    required this.remotePort,
    required this.localPort,
    this.namespace = '',
    this.localAddress = '127.0.0.1',
  });
  final String id, profile, connectionId, target, namespace, localAddress;
  final SavedForwardKind kind;
  final int remotePort, localPort;
  String get name => namespace.isEmpty ? target : '$namespace/$target';
  bool get network => !const {'127.0.0.1', '::1', 'localhost'}.contains(localAddress);
  String get identity => '$profile/${kind.name}/$connectionId/$namespace/$target:$remotePort:$localAddress:$localPort';

  Map<String, Object?> toJson() => {
    'id': id,
    'profile': profile,
    'kind': kind.name,
    'connection_id': connectionId,
    'target': target,
    'namespace': namespace,
    'remote_port': remotePort,
    'local_port': localPort,
    'local_address': localAddress,
  };

  static SavedForward? tryFromJson(Object? value) {
    if (value is! Map) return null;
    final kind = SavedForwardKind.values.where((k) => k.name == value['kind']).firstOrNull;
    if (kind == null ||
        [
          value['id'],
          value['profile'],
          value['connection_id'],
          value['target'],
        ].any((v) => v is! String || v.isEmpty) ||
        value['remote_port'] is! int ||
        value['local_port'] is! int) {
      return null;
    }
    final remote = value['remote_port'] as int, local = value['local_port'] as int;
    if (remote < 1 || remote > 65535 || local < 0 || local > 65535) return null;
    final namespace = value['namespace'];
    final address = value['local_address'];
    if (namespace is! String ||
        address is! String ||
        address.isEmpty ||
        (kind == SavedForwardKind.kubernetes && namespace.isEmpty)) {
      return null;
    }
    return SavedForward(
      id: value['id'] as String,
      profile: value['profile'] as String,
      kind: kind,
      connectionId: value['connection_id'] as String,
      target: value['target'] as String,
      namespace: namespace,
      remotePort: remote,
      localPort: local,
      localAddress: address,
    );
  }
}
