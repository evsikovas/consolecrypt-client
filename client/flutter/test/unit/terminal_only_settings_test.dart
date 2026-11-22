import 'dart:convert';
import 'dart:io';

import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/terminal/terminal_only_settings.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  test('standalone restores appearance and merges independent window edits', () async {
    final dir = await Directory.systemTemp.createTemp('cc-terminal-settings-');
    addTearDown(() => dir.delete(recursive: true));
    final file = File('${dir.path}/prefs.json');
    final first = await TerminalOnlySettings.open(file: file);
    final second = await TerminalOnlySettings.open(file: file);
    expect(first.currentLocal.terminalTabPlacement, TerminalTabPlacement.left);
    await first.updateLocal(
      first.currentLocal.copyWith(
        terminalTabPlacement: TerminalTabPlacement.top,
        terminalTabColumnCollapsed: false,
        terminalTabColumnWidth: 228,
        uiAccentColor: 0x22B8AD,
        themeMode: AppThemeMode.dark,
        terminalFontFamily: 'Consolas',
      ),
    );
    await Future.wait([
      second.updateLocal(second.currentLocal.copyWith(terminalFontSize: 15)),
      second.updateLocal(second.currentLocal.copyWith(terminalFontSize: 17)),
    ]);
    final reopened = (await TerminalOnlySettings.open(file: file)).currentLocal;
    expect(reopened.terminalTabPlacement, TerminalTabPlacement.top);
    expect(reopened.terminalTabColumnCollapsed, isFalse);
    expect(reopened.terminalTabColumnWidth, 228);
    expect(reopened.uiAccentColor, 0x22B8AD);
    expect(reopened.themeMode, AppThemeMode.dark);
    expect(reopened.terminalFontFamily, 'Consolas');
    expect(reopened.terminalFontSize, 17);
    final stored = jsonDecode(await file.readAsString()) as Map;
    expect(stored.containsKey('external_vault_connections'), isFalse);
    expect(stored.containsKey('password_vault_files'), isFalse);
  });
}
