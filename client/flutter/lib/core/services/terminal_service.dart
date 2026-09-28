import 'dart:typed_data';

import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/security/secret_text.dart';

/// Interactive shells over SSH PTY channels (terminal-core). The UI only
/// passes a host id: app-core's Connection Planner resolves inheritance,
/// credentials and the jump chain (CLIENT_SPEC §6.2).
abstract interface class TerminalService {
  /// Starts connecting; progress, host-key prompts and disconnects arrive on
  /// [TerminalSessionHandle.events]. The output stream buffers until the
  /// first listener subscribes and stays the same across reconnects.
  Future<TerminalSessionHandle> open({required ObjectId hostId, required TerminalSize size});

  /// Keyboard / paste input (UTF-8 bytes).
  Future<void> write(TerminalSessionId id, Uint8List data);

  Future<void> resize(TerminalSessionId id, TerminalSize size);

  /// Answer to a [TerminalHostKeyPrompt]. A changed key is a hard failure
  /// and is never answerable.
  Future<void> answerHostKey(TerminalSessionId id, HostKeyDecision decision);

  /// Answer to a [TerminalPasswordPrompt]. `null` cancels the connection.
  /// The password is used for this authentication only and never stored.
  Future<void> answerPassword(TerminalSessionId id, SecretText? password);

  /// Re-establishes a dropped session (same id, same streams).
  Future<void> reconnect(TerminalSessionId id);

  /// Closes the channel and completes both streams.
  Future<void> close(TerminalSessionId id);
}
