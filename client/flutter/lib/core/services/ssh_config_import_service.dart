/// Hosts read from `~/.ssh/config` for import. Parsing happens in the core;
/// nothing is stored until the user picks hosts in the import dialog.
abstract interface class SshConfigImportService {
  /// Parses `path` (default `~/.ssh/config`, `Include` expanded).
  Future<SshConfigPreview> previewSshConfig({String? path});
}

final class SshConfigPreview {
  const SshConfigPreview({required this.path, required this.found, this.hosts = const [], this.warnings = const []});
  final String path;
  final bool found;
  final List<SshConfigHostPreview> hosts;
  final List<String> warnings;

  factory SshConfigPreview.fromJson(Map<String, dynamic> json) => SshConfigPreview(
    path: json['path'] as String? ?? '',
    found: json['found'] == true,
    hosts: List.unmodifiable(
      (json['hosts'] as List? ?? const []).cast<Map<String, dynamic>>().map(SshConfigHostPreview.fromJson),
    ),
    warnings: List.unmodifiable((json['warnings'] as List? ?? const []).cast<String>()),
  );
}

final class SshConfigHostPreview {
  const SshConfigHostPreview({
    required this.alias,
    required this.address,
    this.port,
    this.username,
    this.identityFile,
    this.jump = const [],
    this.proxyCommand = false,
    this.jumpOnly = false,
    this.agentForwarding = false,
    this.keepaliveSecs,
  });
  final String alias, address;
  final int? port;
  final String? username, identityFile;

  /// Hop names (other entries of the same preview), closest first.
  final List<String> jump;
  final bool proxyCommand, jumpOnly, agentForwarding;
  final int? keepaliveSecs;

  factory SshConfigHostPreview.fromJson(Map<String, dynamic> json) => SshConfigHostPreview(
    alias: json['alias'] as String,
    address: json['address'] as String,
    port: json['port'] as int?,
    username: json['username'] as String?,
    identityFile: json['identity_file'] as String?,
    jump: List.unmodifiable((json['jump'] as List? ?? const []).cast<String>()),
    proxyCommand: json['proxy_command'] == true,
    jumpOnly: json['jump_only'] == true,
    agentForwarding: json['agent_forwarding'] == true,
    keepaliveSecs: json['keepalive_secs'] as int?,
  );
}
