import 'dart:io';

import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/terminal/terminal_tabs_controller.dart';
import 'package:flutter/widgets.dart';

/// Second line of a terminal tab: the host's list name (or its address when
/// it has none), its address / this computer's network name, or nothing.
String? terminalTabSubtitle(BuildContext context, TerminalTab tab, TerminalTabSubtitle mode) {
  if (tab.tmuxParent != null && tab.customTitle == null && mode == TerminalTabSubtitle.name) return null;
  final host = tab.host;
  String? text = switch (mode) {
    TerminalTabSubtitle.off => null,
    TerminalTabSubtitle.name when tab.isLocal => context.l10n.terminalLocalComputer,
    TerminalTabSubtitle.address when tab.isLocal => Platform.localHostname,
    TerminalTabSubtitle.name => host == null ? null : (host.name.trim().isEmpty ? host.address : host.name),
    TerminalTabSubtitle.address => host?.address,
  };
  text = text?.trim();
  return text == null || text.isEmpty || text == tab.displayTitle ? null : text;
}
