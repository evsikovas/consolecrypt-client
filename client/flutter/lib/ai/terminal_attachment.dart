import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/terminal/terminal_tabs_controller.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

/// Explicit, in-memory selection snapshot. Opening the side panel can reflow
/// terminal lines; subsequent output must not change the attachment.
final class TerminalAttachment {
  const TerminalAttachment({
    required this.sessionId,
    required this.hostId,
    required this.hostLabel,
    required this.text,
  });
  final TerminalSessionId sessionId;
  final ObjectId hostId;
  final String hostLabel;
  final String text;
}

class TerminalAttachmentController extends Notifier<TerminalAttachment?> {
  @override
  TerminalAttachment? build() {
    ref.listen(activeProfileProvider.select((p) => p?.id), (before, after) {
      if (before != after) clear();
    });
    ref.listen(vaultStatusProvider.select((s) => s.value?.isUnlocked ?? false), (_, unlocked) {
      if (!unlocked) clear();
    });
    ref.listen(terminalTabsProvider.select((s) => s.active?.sessionId), (_, session) {
      if (state?.sessionId != session) clear();
    });
    return null;
  }

  void attach(TerminalTab tab, String text) =>
      state = TerminalAttachment(sessionId: tab.sessionId, hostId: tab.host.id, hostLabel: tab.host.name, text: text);
  void clear() => state = null;
}

final terminalAttachmentProvider = NotifierProvider<TerminalAttachmentController, TerminalAttachment?>(
  TerminalAttachmentController.new,
);
