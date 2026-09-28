import 'dart:math';

import 'package:consolecrypt/ai/code_blocks.dart';
import 'package:consolecrypt/core/mock/mock_backend.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/groups/groups_screen.dart';
import 'package:consolecrypt/hosts/hosts_screen.dart';
import 'package:consolecrypt/snippets/run_flow.dart';
import 'package:consolecrypt/terminal/terminal_tabs_controller.dart';
import 'package:consolecrypt/vault/onboarding_controller.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

ProviderContainer containerFor(MockBackend backend) {
  final container = ProviderContainer(overrides: [appServicesProvider.overrideWithValue(backend.services)]);
  addTearDown(container.dispose);
  addTearDown(backend.dispose);
  return container;
}

void main() {
  group('OnboardingController', () {
    test('picks 3 distinct sorted positions', () {
      for (var seed = 0; seed < 50; seed++) {
        final p = OnboardingController.pickPositions(Random(seed));
        expect(p, hasLength(3));
        expect(p.toSet(), hasLength(3));
        expect([...p]..sort(), p);
        expect(p.every((i) => i >= 0 && i < 24), isTrue);
      }
    });

    test('create → verify wrong/right → complete confirms the kit', () async {
      final backend = MockBackend(config: const MockConfig.test());
      final container = containerFor(backend);
      await backend.profiles.createLocalProfile(name: 'Personal');
      final controller = container.read(onboardingControllerProvider.notifier);

      final passphrase = SecretText('violet-anchor-muffin-glacier-42');
      await controller.createVault(name: 'Personal', passphrase: passphrase);
      expect(passphrase.isWiped, isTrue, reason: 'the passphrase is wiped after hand-off');
      final state = container.read(onboardingControllerProvider);
      expect(state.kit, isNotNull);
      expect(backend.vault.currentStatus.recoveryKitConfirmed, isFalse);

      final words = state.kit!.exposeWords();
      expect(controller.verify({for (final p in state.positions) p: 'wrong'}), isFalse);
      expect(controller.verify({for (final p in state.positions) p: ' ${words[p].toUpperCase()} '}), isTrue);

      await controller.complete();
      expect(backend.vault.currentStatus.recoveryKitConfirmed, isTrue);
      expect(container.read(onboardingControllerProvider).kit, isNull);
    });

    test('rejects weak passphrases', () async {
      final backend = MockBackend(config: const MockConfig.test());
      final container = containerFor(backend);
      await backend.profiles.createLocalProfile(name: 'Personal');
      expect(
        () => container
            .read(onboardingControllerProvider.notifier)
            .createVault(name: 'P', passphrase: SecretText('short')),
        throwsA(isA<Exception>()),
      );
    });
  });

  group('TerminalTabsController', () {
    test('multiline insertion uses bracketed paste and rejects unsafe control input', () async {
      final backend = MockBackend(config: const MockConfig.test());
      final container = containerFor(backend);
      await backend.debugSignInDemoAndUnlock();
      final host = (await backend.inventory.watchHosts().first).firstWhere((h) => h.name == 'raspberry-pi');
      final controller = container.read(terminalTabsProvider.notifier);
      final tab = await controller.open(host);
      await pumpEventQueue();
      final output = <String>[];
      tab.terminal.onOutput = output.add;
      expect(controller.insertIntoActive('echo one\necho two'), isFalse);
      expect(output, isEmpty);
      tab.terminal.write('\x1b[?2004h');
      expect(controller.insertIntoActive('echo one\necho two'), isTrue);
      expect(output.single, '\x1b[200~echo one\necho two\x1b[201~');
      expect(controller.insertIntoActive('echo one\x1b[201~\nwhoami'), isFalse);
      expect(output, hasLength(1));
      tab.state = SessionConnectionState.disconnected;
      expect(controller.insertIntoActive('echo one\necho two'), isFalse);
    });

    test('open → host key prompt → accept → connected; insert and run', () async {
      final backend = MockBackend(config: const MockConfig.test());
      final container = containerFor(backend);
      await backend.debugSignInDemoAndUnlock();
      final hosts = await backend.inventory.watchHosts().first;
      final staging = hosts.firstWhere((h) => h.name == 'staging-web'); // not in known hosts
      final controller = container.read(terminalTabsProvider.notifier);

      final tab = await controller.open(staging);
      await pumpEventQueue();
      expect(tab.hostKey, isNotNull, reason: 'unknown key must be confirmed');
      expect(tab.state, SessionConnectionState.awaitingHostKey);

      expect(controller.insertIntoActive('echo queued'), isTrue);
      await controller.answerHostKey(tab, HostKeyDecision.acceptAndSave);
      await pumpEventQueue();
      expect(tab.state, SessionConnectionState.connected);
      final known = await backend.inventory.watchKnownHosts().first;
      expect(known.any((k) => k.hostPattern == 'staging.example.net'), isTrue);

      controller.runInTab(tab, 'whoami');
      await pumpEventQueue();
      final screen = tab.terminal.buffer.lines.toList().map((l) => l.toString()).join('\n');
      expect(screen, contains('echo queued'));
      expect(screen, contains('ubuntu'), reason: 'whoami prints the inherited username');

      await controller.close(tab);
      expect(container.read(terminalTabsProvider).tabs, isEmpty);
    });

    test('switching profile closes the sessions of the previous profile', () async {
      final backend = MockBackend(config: const MockConfig.test());
      final container = containerFor(backend);
      await backend.debugSignInDemoAndUnlock();
      final pi = (await backend.inventory.watchHosts().first).firstWhere((h) => h.name == 'raspberry-pi');
      await container.read(terminalTabsProvider.notifier).open(pi);
      // Keep the profile stream observed so the controller sees the switch.
      final sub = container.listen(activeProfileProvider, (_, _) {});
      addTearDown(sub.close);
      await pumpEventQueue();
      await backend.debugCreateUnlockedLocalProfile();
      await pumpEventQueue();
      expect(container.read(terminalTabsProvider).tabs, isEmpty);
    });

    test('password prompt: cancel disconnects, answer connects; nothing is stored', () async {
      final backend = MockBackend(config: const MockConfig.test());
      final container = containerFor(backend);
      await backend.debugSignInDemoAndUnlock();
      final bastion = (await backend.inventory.watchHosts().first).firstWhere((h) => h.name == 'bastion-a');
      final host = await backend.inventory.saveHostWithAuth(bastion, const HostAuthPasswordPrompt());
      final controller = container.read(terminalTabsProvider.notifier);

      final tab = await controller.open(host);
      await pumpEventQueue();
      expect(tab.state, SessionConnectionState.awaitingPassword);
      expect(tab.passwordPrompt?.username, 'jump');
      final secret = SecretText('typed');
      await controller.answerPassword(tab, secret);
      await pumpEventQueue();
      expect(tab.state, SessionConnectionState.connected);
      expect(secret.isWiped, isTrue, reason: 'used once, then wiped');

      final second = await controller.open(host);
      await pumpEventQueue();
      await controller.answerPassword(second, null);
      await pumpEventQueue();
      expect(second.state, SessionConnectionState.disconnected);
    });

    test('changed host key is a hard failure', () async {
      final backend = MockBackend(config: const MockConfig.test());
      final container = containerFor(backend);
      await backend.debugSignInDemoAndUnlock();
      final legacy = (await backend.inventory.watchHosts().first).firstWhere((h) => h.name == 'legacy-ftp');
      final tab = await container.read(terminalTabsProvider.notifier).open(legacy);
      await pumpEventQueue();
      expect(tab.hostKey?.changed, isTrue);
      expect(tab.state, SessionConnectionState.disconnected);
    });
  });

  group('pure helpers', () {
    test('filterHosts matches text, tags and group path', () {
      final inv = FakeHosts.build();
      expect(filterHosts(inv.hosts, query: 'prod db', tags: const {}, groups: inv.groups).map((h) => h.name), [
        'prod-db-1',
        'prod-db-2',
      ]);
      expect(filterHosts(inv.hosts, query: '', tags: {'replica'}, groups: inv.groups).single.name, 'prod-db-2');
      expect(filterHosts(inv.hosts, query: 'databases', tags: const {}, groups: inv.groups), hasLength(2));
    });

    test('flattenGroupTree is pre-order with depth', () {
      final inv = FakeHosts.build();
      final nodes = flattenGroupTree(inv.groups.values.toList());
      final names = [for (final n in nodes) '${'  ' * n.depth}${n.group.name}'];
      expect(names.first, 'Home lab');
      expect(names, containsAllInOrder(['Production', '  Databases', '  Web']));
    });

    test('parseAnswer splits fenced code, even while streaming', () {
      final parts = parseAnswer('Run this:\n\n```bash\ndf -h\n```\n\nDone.');
      expect(parts, hasLength(3));
      expect((parts[1] as CodeSegment).code, 'df -h');
      expect((parts[1] as CodeSegment).language, 'bash');
      final streaming = parseAnswer('Try:\n```bash\nls -la');
      expect((streaming.last as CodeSegment).code, 'ls -la');
    });

    test('canRunCommand requires acknowledgment for non-read-only commands', () {
      expect(canRunCommand(RiskLevel.readOnly, acknowledged: false), isTrue);
      for (final r in [RiskLevel.modifying, RiskLevel.destructive, RiskLevel.unknown]) {
        expect(canRunCommand(r, acknowledged: false), isFalse, reason: '$r');
        expect(canRunCommand(r, acknowledged: true), isTrue, reason: '$r');
      }
    });
  });
}

/// Small fixture for the pure helpers.
final class FakeHosts {
  FakeHosts._(this.hosts, this.groups);

  factory FakeHosts.build() {
    final now = DateTime.now();
    Group g(String name, {Group? parent}) =>
        Group(id: ObjectId.generate(), name: name, parentId: parent?.id, createdAt: now, updatedAt: now);
    final prod = g('Production');
    final db = g('Databases', parent: prod);
    final web = g('Web', parent: prod);
    final home = g('Home lab');
    Host h(String name, String address, Group group, List<String> tags) => Host(
      id: ObjectId.generate(),
      name: name,
      address: address,
      groupId: group.id,
      tags: tags,
      createdAt: now,
      updatedAt: now,
    );
    return FakeHosts._(
      [
        h('prod-db-2', '10.10.10.21', db, ['postgres', 'replica']),
        h('prod-db-1', '10.10.10.20', db, ['postgres', 'primary']),
        h('prod-web-1', '10.10.20.11', web, ['nginx']),
        h('raspberry-pi', '192.168.1.50', home, ['home']),
      ],
      {
        for (final x in [prod, db, web, home]) x.id: x,
      },
    );
  }

  final List<Host> hosts;
  final Map<ObjectId, Group> groups;
}
