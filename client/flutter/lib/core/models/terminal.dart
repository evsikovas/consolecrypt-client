import 'dart:typed_data';

import 'package:consolecrypt/core/models/ids.dart';

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
  const TerminalSessionHandle({required this.id, required this.output, required this.events});

  final TerminalSessionId id;

  /// Raw PTY output bytes (UTF-8, possibly split mid-sequence).
  final Stream<Uint8List> output;
  final Stream<TerminalEvent> events;
}
