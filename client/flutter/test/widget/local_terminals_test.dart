import 'package:consolecrypt/ai/terminal_attachment.dart';
import 'package:consolecrypt/app/app.dart';
import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/app/workspace_tools_controller.dart';
import 'package:consolecrypt/core/mock/mock_backend.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/services/app_services.dart';
import 'package:consolecrypt/terminal/terminal_tabs_controller.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/test_app.dart';

const profiles = [
  LocalShellProfile(id: 'windows:powershell', kind: 'powershell', label: 'PowerShell', shell: 'powershell'),
  LocalShellProfile(id: 'windows:cmd', kind: 'cmd', label: 'Command Prompt', shell: 'cmd'),
  LocalShellProfile(id: 'wsl:Ubuntu', kind: 'wsl', label: 'Ubuntu', shell: 'sh', distribution: 'Ubuntu'),
  LocalShellProfile(id: 'wsl:Debian', kind: 'wsl', label: 'Debian', shell: 'sh', distribution: 'Debian'),
];

final class _Ai implements AiService {
  _Ai(this.delegate);
  final AiService delegate;
  SnippetType? dialect;
  AiContextSelection? generatedContext, chatContext;
  @override
  Stream<List<AiProviderConfig>> watchProviders() => delegate.watchProviders();
  @override
  Stream<AiStreamEvent> generateCommand({
    required ObjectId providerId,
    required String request,
    SnippetType? dialect,
    AiContextSelection context = AiContextSelection.none,
  }) {
    this.dialect = dialect;
    generatedContext = context;
    return Stream.value(
      AiCompleted(
        fullText: '',
        report: const SanitizationReport(profile: PrivacyProfile.local, redactions: 0, remote: false),
        command: GeneratedCommand(
          command: 'Get-Process',
          explanation: 'Processes',
          suggestedRisk: RiskLevel.readOnly,
          dialect: dialect,
        ),
      ),
    );
  }

  @override
  Stream<AiStreamEvent> chat({
    required ObjectId providerId,
    required List<ChatMessage> history,
    AiContextSelection context = AiContextSelection.none,
  }) {
    chatContext = context;
    return Stream.value(
      const AiCompleted(
        fullText: 'Ready',
        report: SanitizationReport(profile: PrivacyProfile.local, redactions: 0, remote: false),
      ),
    );
  }

  @override
  dynamic noSuchMethod(Invocation invocation) => throw UnimplementedError();
}

Future<(MockBackend, ProviderContainer, _Ai)> fixture(WidgetTester tester, {AppLocale locale = AppLocale.en}) async {
  final backend = testBackend();
  addTearDown(backend.dispose);
  await backend.debugSignInDemoAndUnlock();
  backend.terminal.localShellProfiles = const LocalShellProfiles(profiles: profiles);
  setTestLocale(backend, locale);
  final ai = _Ai(backend.ai);
  tester.view.physicalSize = const Size(1100, 850);
  tester.view.devicePixelRatio = 1;
  addTearDown(tester.view.reset);
  await tester.pumpWidget(
    ProviderScope(
      overrides: [appServicesProvider.overrideWithValue(backend.services), aiServiceProvider.overrideWithValue(ai)],
      child: const ConsoleCryptApp(),
    ),
  );
  await settle(tester);
  return (backend, ProviderScope.containerOf(tester.element(find.byType(Navigator).first)), ai);
}

Future<void> newTab(WidgetTester tester) async {
  await tester.sendKeyDownEvent(LogicalKeyboardKey.controlLeft);
  await tester.sendKeyEvent(LogicalKeyboardKey.keyT);
  await tester.sendKeyUpEvent(LogicalKeyboardKey.controlLeft);
  await settle(tester);
}

void main() {
  for (final locale in [AppLocale.en, AppLocale.ru]) {
    testWidgets('mixed picker preserves WSL distributions, SSH and local toolbar (${locale.name})', (tester) async {
      final (_, c, _) = await fixture(tester, locale: locale);
      await newTab(tester);
      expect(find.byKey(const ValueKey('terminal-picker')), findsOneWidget);
      expect(find.byKey(const ValueKey('pick-local-wsl:Ubuntu')), findsOneWidget);
      expect(find.byKey(const ValueKey('pick-local-wsl:Debian')), findsOneWidget);
      await enterKey(tester, 'host-picker-search', 'Debian');
      expect(find.byKey(const ValueKey('pick-local-wsl:Ubuntu')), findsNothing);
      await tapKey(tester, 'pick-local-wsl:Debian');
      expect(c.read(terminalTabsProvider).active!.isLocal, isTrue);
      expect((c.read(terminalTabsProvider).active!.target as LocalTerminalTarget).profile.id, 'wsl:Debian');
      expect(find.byKey(const ValueKey('terminal-sftp')), findsNothing);
      await tapKey(tester, 'glass-tab-add');
      await pickHost(tester, 'raspberry-pi');
      expect(c.read(terminalTabsProvider).tabs, hasLength(2));
      expect(c.read(terminalTabsProvider).active!.host?.name, 'raspberry-pi');
      expect(find.byKey(const ValueKey('terminal-sftp')), findsOneWidget);
      c.read(terminalTabsProvider.notifier).activate(0);
      await settle(tester);
      expect(find.byKey(const ValueKey('terminal-sftp')), findsNothing);
      expect(tester.takeException(), isNull);
    }, variant: TargetPlatformVariant.only(TargetPlatform.windows));
  }

  testWidgets('local AI carries PowerShell dialect; Insert sends no Enter and Run is unavailable', (tester) async {
    final (_, c, ai) = await fixture(tester);
    final tab = await c.read(terminalTabsProvider.notifier).openTarget(LocalTerminalTarget(profiles[0]));
    c.read(routerProvider).go(AppRoutes.terminal);
    await settle(tester);
    await tapKey(tester, 'terminal-ask-ai');
    await enterKey(tester, 'palette-input', 'list processes');
    await tapKey(tester, 'palette-generate');
    expect(ai.dialect, SnippetType.powershell);
    expect(ai.generatedContext?.hostId, isNull);
    expect(ai.generatedContext?.shellTarget, SnippetType.powershell);
    expect(ai.generatedContext?.selectedTerminalText, isNull);
    expect(isEnabled(tester, 'ai-run'), isFalse);
    final sent = <String>[];
    tab.terminal.onOutput = sent.add;
    await tapKey(tester, 'ai-insert');
    expect(sent, ['Get-Process']);
    expect(sent.any((value) => value.contains('\r') || value.contains('\n')), isFalse);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('local selection is opt-in AI context with no fake host ID', (tester) async {
    final (_, c, ai) = await fixture(tester);
    final tab = await c.read(terminalTabsProvider.notifier).openTarget(LocalTerminalTarget(profiles[1]));
    c.read(routerProvider).go(AppRoutes.terminal);
    await settle(tester);
    c.read(terminalAttachmentProvider.notifier).attach(tab, 'explicit local selection');
    c.read(workspaceToolsProvider.notifier).open(WorkspaceTool.ai);
    await settle(tester);
    await enterKey(tester, 'ai-input', 'explain this');
    await tapKey(tester, 'ai-send');
    expect(ai.chatContext?.hostId, isNull);
    expect(ai.chatContext?.shellTarget, SnippetType.cmd);
    expect(ai.chatContext?.selectedTerminalText, 'explicit local selection');
    expect(c.read(terminalAttachmentProvider), isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('mobile terminal picker never offers desktop local profiles', (tester) async {
    final (_, c, _) = await fixture(tester);
    c.read(routerProvider).go(AppRoutes.terminal);
    await settle(tester);
    await tapKey(tester, 'terminal-connect');
    await enterKey(tester, 'host-picker-search', 'raspberry-pi');
    expect(find.byKey(const ValueKey('pick-local-windows:powershell')), findsNothing);
    expect(find.byKey(const ValueKey('pick-raspberry-pi')), findsOneWidget);
  }, variant: TargetPlatformVariant.only(TargetPlatform.android));
}
