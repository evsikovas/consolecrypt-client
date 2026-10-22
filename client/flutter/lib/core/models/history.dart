import 'package:consolecrypt/core/models/ids.dart';

/// Mirrors `cc_models::history::HistoryEntry`. Synced only with
/// `TerminalHistoryMode.encryptedSync`.
final class HistoryEntry {
  const HistoryEntry({required this.id, required this.command, required this.executedAt, this.hostId, this.exitCode});

  final ObjectId id;
  final ObjectId? hostId;
  final String command;
  final int? exitCode;
  final DateTime executedAt;
}
