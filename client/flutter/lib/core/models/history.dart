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

/// Mirrors `cc_models::note::Note` (knowledge-base note, synced E2EE).
final class Note {
  const Note({
    required this.id,
    required this.title,
    required this.body,
    required this.createdAt,
    required this.updatedAt,
    this.tags = const [],
  });

  final ObjectId id;
  final String title;
  final String body;
  final List<String> tags;
  final DateTime createdAt;
  final DateTime updatedAt;
}
