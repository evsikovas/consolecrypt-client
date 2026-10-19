import 'dart:async';
import 'dart:typed_data';

import 'package:consolecrypt/core/mock/mock_backend.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/services/app_services.dart';
import 'package:consolecrypt/terminal/terminal_tabs_controller.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

const ubuntu = LocalShellProfile(id: 'wsl:Ubuntu', kind: 'wsl', label: 'Ubuntu', shell: 'sh', distribution: 'Ubuntu');
const debian = LocalShellProfile(id: 'wsl:Debian', kind: 'wsl', label: 'Debian', shell: 'sh', distribution: 'Debian');

void main() {
  test('two WSL distributions and SSH coexist without creating host objects', () async {
    final backend = MockBackend(config: const MockConfig.test());
    backend.terminal.localShellProfiles = const LocalShellProfiles(profiles: [ubuntu, debian]);
    final c = ProviderContainer(overrides: [appServicesProvider.overrideWithValue(backend.services)]);
    addTearDown(c.dispose);
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    final hosts = await backend.inventory.watchHosts().first;
    final ctl = c.read(terminalTabsProvider.notifier);
    final ssh = await ctl.open(hosts.firstWhere((h) => h.name == 'raspberry-pi'));
    final first = await ctl.openTarget(const LocalTerminalTarget(ubuntu));
    final second = await ctl.openTarget(const LocalTerminalTarget(debian));
    await pumpEventQueue();
    expect(c.read(terminalTabsProvider).tabs, [ssh, first, second]);
    expect(first.host, isNull);
    expect(first.hostKey, isNull);
    expect(first.passwordPrompt, isNull);
    expect(first.sessionId, isNot(second.sessionId));
    expect((first.target as LocalTerminalTarget).profile.id, ubuntu.id);
    expect((second.target as LocalTerminalTarget).profile.id, debian.id);
    expect(await backend.inventory.watchHosts().first, hosts);
    expect(first.isConnected && second.isConnected, isTrue);
    final sent = <String>[];
    second.terminal.onOutput = sent.add;
    expect(ctl.insertIntoActive('printf ready'), isTrue);
    expect(sent, ['printf ready']);
    ctl.runInTab(second, 'should not run');
    expect(sent, ['printf ready'], reason: 'local generated Run must not append Enter');
    ctl.closeAll();
    await pumpEventQueue();
  });

  test('late local open after closeAll is closed and never resurrects a tab', () async {
    final backend = MockBackend(config: const MockConfig.test());
    final service = _DelayedTerminal();
    final c = ProviderContainer(
      overrides: [
        appServicesProvider.overrideWithValue(backend.services),
        terminalServiceProvider.overrideWithValue(service),
      ],
    );
    addTearDown(c.dispose);
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    final ctl = c.read(terminalTabsProvider.notifier);
    final opening = ctl.openTarget(const LocalTerminalTarget(ubuntu));
    final failed = expectLater(opening, throwsA(isA<AppException>()));
    ctl.closeAll();
    service.opened.complete(
      TerminalSessionHandle(id: service.id, output: const Stream.empty(), events: const Stream.empty()),
    );
    await failed;
    expect(service.closed, [service.id]);
    expect(c.read(terminalTabsProvider).tabs, isEmpty);
  });

  test('shell dialect mapping distinguishes PowerShell, cmd, zsh and WSL', () {
    expect(terminalShellType(r'C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe'), SnippetType.powershell);
    expect(terminalShellType('pwsh'), SnippetType.powershell);
    expect(terminalShellType('cmd'), SnippetType.cmd);
    expect(terminalShellType('/bin/zsh'), SnippetType.zsh);
    expect(const LocalTerminalTarget(ubuntu).shellType, SnippetType.shell);
  });
}

final class _DelayedTerminal implements TerminalService, LocalTerminalService {
  final opened = Completer<TerminalSessionHandle>();
  final id = TerminalSessionId.generate();
  final closed = <TerminalSessionId>[];
  @override
  Future<TerminalSessionHandle> openLocal({required String profileId, required TerminalSize size}) => opened.future;
  @override
  Future<LocalShellProfiles> listLocalProfiles() async => const LocalShellProfiles(profiles: [ubuntu]);
  @override
  Future<void> close(TerminalSessionId id) async => closed.add(id);
  @override
  Future<TerminalSessionHandle> open({required ObjectId hostId, required TerminalSize size}) => opened.future;
  @override
  Future<void> write(TerminalSessionId id, Uint8List data) async {}
  @override
  Future<void> resize(TerminalSessionId id, TerminalSize size) async {}
  @override
  Future<void> reconnect(TerminalSessionId id) async {}
  @override
  Future<void> answerHostKey(TerminalSessionId id, HostKeyDecision decision) async {}
  @override
  Future<void> answerPassword(TerminalSessionId id, SecretText? password) async => password?.wipe();
}
