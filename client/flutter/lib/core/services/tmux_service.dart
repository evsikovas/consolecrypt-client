import 'package:consolecrypt/core/models/models.dart';

final class TmuxSession {
  TmuxSession.fromJson(Map<String, dynamic> data)
    : id = data['id'] as int,
      serverPid = data['pid'] as int,
      created = data['created'] as int,
      name = data['name'] as String,
      path = data['path'] as String,
      windows = data['windows'] as int,
      clients = data['clients'] as int,
      activity = DateTime.fromMillisecondsSinceEpoch((data['activity'] as int) * 1000);
  final int id, serverPid, created, windows, clients;
  final String name, path;
  final DateTime activity;
  Map<String, int> get identity => {'id': id, 'pid': serverPid, 'created': created};
  bool matches(TmuxSession? other) =>
      other != null && other.id == id && other.serverPid == serverPid && other.created == created;
}

abstract interface class TmuxService {
  Future<String?> tmuxVersion(TerminalSessionId parent);
  Future<List<TmuxSession>> tmuxSessions(TerminalSessionId parent);
  Future<void> createTmux(TerminalSessionId parent, String name);
  Future<void> renameTmux(TerminalSessionId parent, TmuxSession session, String name);
  Future<void> killTmux(TerminalSessionId parent, TmuxSession session);
  Future<TerminalSessionHandle> attachTmux(TerminalSessionId parent, TmuxSession session);
}
