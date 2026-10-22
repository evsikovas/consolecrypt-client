import 'dart:typed_data';

import 'package:consolecrypt/core/models/host.dart';
import 'package:consolecrypt/core/models/ids.dart';
import 'package:consolecrypt/core/models/snippet.dart';

/// Runtime destinations never become inventory objects or sync to the vault.
sealed class TerminalTarget {
  const TerminalTarget();
  String get label;
  String get detail;
  Host? get sshHost;
  SnippetType? get shellType;
}

final class SshTerminalTarget extends TerminalTarget {
  const SshTerminalTarget(this.host);
  final Host host;
  @override
  String get label => host.name;
  @override
  String get detail => host.address;
  @override
  Host get sshHost => host;
  @override
  SnippetType? get shellType => null; // Core resolves saved SSH shell metadata.
}

final class LocalTerminalTarget extends TerminalTarget {
  const LocalTerminalTarget(this.profile);
  final LocalShellProfile profile;
  @override
  String get label => profile.label;
  @override
  String get detail => profile.distribution ?? profile.shell;
  @override
  Host? get sshHost => null;
  @override
  SnippetType get shellType => terminalShellType(profile.shell) ?? SnippetType.shell;
}

/// A shell inside a Docker container, on this computer or a saved SSH host.
final class DockerTerminalTarget extends TerminalTarget {
  const DockerTerminalTarget({
    required this.sessionId,
    required this.hostName,
    required this.containerId,
    required this.containerName,
    this.hostId,
    this.command = const ['/bin/sh'],
  });
  final String sessionId, hostName, containerId, containerName;

  /// The Docker host (`local` or a saved SSH host id), for notes.
  final String? hostId;

  /// The shell to run; the auto-detect marker picks bash, ash or sh.
  final List<String> command;
  @override
  String get label => containerName;
  @override
  String get detail => hostName;
  @override
  Host? get sshHost => null;
  @override
  SnippetType get shellType => terminalShellType(command.firstOrNull) ?? SnippetType.shell;
}

final class KubernetesTerminalTarget extends TerminalTarget {
  const KubernetesTerminalTarget({
    required this.sessionId,
    required this.contextName,
    required this.namespace,
    required this.pod,
    this.contextId,
    this.container,
    this.command = const ['/bin/sh'],
    this.tty = true,
  });
  final String sessionId, contextName, namespace, pod;
  final String? container;

  /// The saved Kubernetes context, for notes.
  final String? contextId;

  /// Null attaches to the container's existing process.
  final List<String>? command;
  final bool tty;
  @override
  String get label => '$namespace/$pod';
  @override
  String get detail => '$contextName · ${container ?? pod}';
  @override
  Host? get sshHost => null;
  @override
  SnippetType get shellType => terminalShellType(command?.firstOrNull) ?? SnippetType.shell;
}

SnippetType? terminalShellType(String? shell) {
  final name = shell?.split(RegExp(r'[/\\]')).last.toLowerCase().replaceFirst(RegExp(r'\.exe$'), '');
  return switch (name) {
    'zsh' => SnippetType.zsh,
    'bash' => SnippetType.bash,
    'pwsh' || 'powershell' => SnippetType.powershell,
    'cmd' => SnippetType.cmd,
    'sh' || 'posix' => SnippetType.shell,
    _ => null,
  };
}

final class LocalShellProfile {
  const LocalShellProfile({
    required this.id,
    required this.kind,
    required this.label,
    required this.shell,
    this.distribution,
  });
  final String id, kind, label, shell;
  final String? distribution;
}

final class LocalShellProfiles {
  const LocalShellProfiles({this.profiles = const [], this.unavailableReason});
  final List<LocalShellProfile> profiles;
  final String? unavailableReason;
}

/// A reset and its snapshot share the output queue with ordinary data. This
/// prevents bytes buffered before a reattach from painting after the reset.
final class TerminalOutputFrame {
  const TerminalOutputFrame(this.data, {this.reset = false});
  final Uint8List data;
  final bool reset;
}

/// PTY size in character cells.
final class TerminalSize {
  const TerminalSize(this.columns, this.rows);

  static const initial = TerminalSize(120, 32);

  final int columns;
  final int rows;

  @override
  bool operator ==(Object other) => other is TerminalSize && other.columns == columns && other.rows == rows;

  @override
  int get hashCode => Object.hash(columns, rows);

  @override
  String toString() => '${columns}x$rows';
}

/// Connection lifecycle of a terminal session.
enum SessionConnectionState { connecting, awaitingHostKey, awaitingPassword, connected, reconnecting, disconnected }

/// Host key presented by a server that is not (yet) in known hosts.
final class HostKeyInfo {
  const HostKeyInfo({
    required this.hostPattern,
    required this.keyType,
    required this.fingerprintSha256,
    this.changed = false,
  });

  final String hostPattern;
  final String keyType;
  final String fingerprintSha256;

  /// The key differs from a recorded one. Always a hard failure (the UI
  /// never offers "accept" for this case).
  final bool changed;
}

/// Answer to an unknown-host-key prompt (`HostKeyPrompt::confirm_unknown`).
enum HostKeyDecision { acceptAndSave, acceptOnce, reject }

/// Events on a terminal session (FRB `StreamSink<TerminalEvent>`).
sealed class TerminalEvent {
  const TerminalEvent();
}

final class TerminalStateChanged extends TerminalEvent {
  const TerminalStateChanged(this.state, {this.message});

  final SessionConnectionState state;

  /// Reason for `disconnected` / `reconnecting`, or a route description
  /// for `connecting` ("via bastion-a → bastion-b").
  final String? message;
}

final class TerminalHostKeyPrompt extends TerminalEvent {
  const TerminalHostKeyPrompt(this.info);

  final HostKeyInfo info;
}

/// The server asks for a password (no stored password for this host).
/// Answer with `TerminalService.answerPassword`; the value is never stored.
final class TerminalPasswordPrompt extends TerminalEvent {
  const TerminalPasswordPrompt({required this.username, required this.hostLabel, this.retry = false});

  final String username;
  final String hostLabel;

  /// The previous attempt was rejected.
  final bool retry;
}

final class TerminalTitleChanged extends TerminalEvent {
  const TerminalTitleChanged(this.title);

  final String title;
}

final class TerminalExited extends TerminalEvent {
  const TerminalExited(this.exitCode);

  final int? exitCode;
}

/// A live session: byte streams in both directions (`write` goes through
/// `TerminalService.write`).
final class TerminalSessionHandle {
  const TerminalSessionHandle({required this.id, required this.output, required this.events, this.framedOutput});

  final TerminalSessionId id;

  /// Raw PTY output bytes (UTF-8, possibly split mid-sequence).
  final Stream<Uint8List> output;
  final Stream<TerminalOutputFrame>? framedOutput;
  final Stream<TerminalEvent> events;
}
