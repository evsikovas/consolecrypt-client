import 'package:consolecrypt/kubernetes/kube_service.dart';

abstract interface class KubeContextTerminalService {
  Future<KubeContextTerminalProfiles> contextTerminalProfiles();
  Future<String> openContextTerminal(String sessionId, String namespace, String profileId);
}

final class KubeContextTerminalProfiles {
  const KubeContextTerminalProfiles({required this.profiles, required this.wslSupported});
  final List<KubeContextTerminalProfile> profiles;
  final bool wslSupported;
  factory KubeContextTerminalProfiles.fromJson(KubeJson json) => KubeContextTerminalProfiles(
    profiles: (json['profiles'] as List)
        .cast<KubeJson>()
        .map(
          (profile) => KubeContextTerminalProfile(
            id: profile['id'] as String,
            label: profile['label'] as String,
            shell: profile['shell'] as String,
          ),
        )
        .toList(),
    wslSupported: json['wsl_supported'] as bool,
  );
}

final class KubeContextTerminalProfile {
  const KubeContextTerminalProfile({required this.id, required this.label, required this.shell});
  final String id, label, shell;
}
