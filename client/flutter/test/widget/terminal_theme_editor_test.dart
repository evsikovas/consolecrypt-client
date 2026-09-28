import 'dart:convert';
import 'dart:typed_data';

import 'package:consolecrypt/app/theme.dart';
import 'package:consolecrypt/app/theme/terminal_palettes.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/settings/terminal_theme_io.dart';
import 'package:file_selector_platform_interface/file_selector_platform_interface.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';
import 'package:xterm/xterm.dart';

import '../helpers/test_app.dart';

class _Files extends FileSelectorPlatform {
  XFile? next;
  @override
  Future<XFile?> openFile({
    List<XTypeGroup>? acceptedTypeGroups,
    String? initialDirectory,
    String? confirmButtonText,
  }) async => next;
}

void main() {
  testWidgets('saved profiles are offered without opening one until the user chooses', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugCreateUnlockedLocalProfile(name: 'My vault');
    final profile = backend.profiles.currentProfiles.active!;
    backend.cloud.lockActive();
    backend.cloud.publishProfiles(clearActive: true);
    await pumpApp(tester, backend, locale: AppLocale.ru, size: const Size(1024, 720));
    expect(backend.profiles.currentProfiles.active, isNull);
    expect(find.text('Открыть профиль'), findsOneWidget);
    expect(find.textContaining('пароль связки'), findsOneWidget);
    await tapKey(tester, 'reopen-last-profile');
    expect(backend.settings.currentLocal.reopenLastProfile, isTrue);
    expect(backend.profiles.currentProfiles.active, isNull);
    await tapKey(tester, 'resume-profile-${profile.id.value}');
    expect(backend.profiles.currentProfiles.active!.id, profile.id);
    expect(backend.vault.currentStatus.phase, VaultPhase.locked);
    expect(tester.takeException(), isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('spectrum and HEX edit a draft; cancel preserves saved colors; apply updates live terminal', (
    tester,
  ) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    await pumpApp(tester, backend, locale: AppLocale.ru, size: const Size(1024, 720));
    await enterKey(tester, 'hosts-search', 'staging-web');
    await tapKey(tester, 'connect-staging-web');
    await tapKey(tester, 'accept-host-key');
    final terminal = tester.widget<TerminalView>(find.byType(TerminalView).first).terminal;
    await tapKey(tester, 'nav-settings');
    await tapKey(tester, 'terminal-theme-edit');
    await tapKey(tester, 'terminal-color-background');
    await tester.tapAt(tester.getCenter(find.byKey(const ValueKey('color-spectrum'))));
    await settle(tester);
    await enterKey(tester, 'color-picker-hex', '#ZZZZZZ');
    expect(isEnabled(tester, 'color-picker-apply'), isFalse);
    await enterKey(tester, 'color-picker-hex', '#123456');
    await tapKey(tester, 'color-picker-apply');
    expect(backend.services.settings.currentLocal.customTerminalColors, isNull);
    await tapKey(tester, 'terminal-theme-cancel');
    expect(backend.services.settings.currentLocal.customTerminalColors, isNull);
    await tapKey(tester, 'terminal-theme-edit');
    await tapKey(tester, 'terminal-color-background');
    await enterKey(tester, 'color-picker-hex', '#102030');
    await tapKey(tester, 'color-picker-apply');
    await tapKey(tester, 'terminal-theme-save');
    expect(backend.services.settings.currentLocal.terminalColorScheme, TerminalColorScheme.custom);
    expect(backend.services.settings.currentLocal.customTerminalColors!['background'], 0x102030);
    await tapKey(tester, 'nav-terminal');
    final view = tester.widget<TerminalView>(find.byType(TerminalView).first);
    expect(view.terminal, same(terminal));
    expect(view.theme.background, const Color(0xFF102030));
    expect(tester.takeException(), isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('file import validates before replacing the draft, cancellation leaves settings unchanged', (
    tester,
  ) async {
    final original = FileSelectorPlatform.instance;
    final files = _Files();
    FileSelectorPlatform.instance = files;
    addTearDown(() => FileSelectorPlatform.instance = original);
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    await pumpApp(tester, backend, locale: AppLocale.ru, size: const Size(1024, 720));
    await tapKey(tester, 'nav-settings');
    await tapKey(tester, 'terminal-theme-edit');
    files.next = XFile.fromData(Uint8List.fromList(utf8.encode('{}')), name: 'bad.json');
    await tapKey(tester, 'terminal-theme-import');
    expect(find.textContaining('Не удалось импортировать'), findsOneWidget);
    expect(backend.services.settings.currentLocal.customTerminalColors, isNull);
    final palette = colorsFromTerminalTheme(AppTheme.terminalTheme(Brightness.dark)).withColor('foreground', 0xFEDCBA);
    files.next = XFile.fromData(Uint8List.fromList(utf8.encode(encodeTerminalTheme(palette))), name: 'test.json');
    await tapKey(tester, 'terminal-theme-import');
    expect(find.textContaining('Не удалось импортировать'), findsNothing);
    expect(find.text('#FEDCBA'), findsOneWidget);
    files.next = null;
    await tapKey(tester, 'terminal-theme-import');
    expect(find.text('#FEDCBA'), findsOneWidget);
    await tapKey(tester, 'terminal-theme-save');
    expect(backend.services.settings.currentLocal.customTerminalColors!.colors, palette.colors);
    expect(tester.takeException(), isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));
}
