// Capture the released widgets with the mock inventory and an in-memory SSH transport.
import 'dart:convert';
import 'dart:io' as io;
import 'dart:typed_data';

import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/mock/mock_docker.dart';
import 'package:consolecrypt/docker/docker_providers.dart';
import 'package:consolecrypt/docker/docker_service.dart';
import 'package:consolecrypt/kubernetes/kube_browser_service.dart';
import 'package:consolecrypt/kubernetes/kube_manifest_service.dart';
import 'package:consolecrypt/kubernetes/kube_providers.dart';
import 'package:consolecrypt/kubernetes/kube_service.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/services/terminal_service.dart';
import 'package:consolecrypt/core/services/tmux_service.dart';
import 'package:consolecrypt/terminal/terminal_tabs_controller.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../test/helpers/automation_ui_proof.dart';
import '../test/helpers/test_app.dart';
import 'kube_capture_support.dart';

class _DemoCluster extends Fake implements KubeService, KubeBrowserService, KubeManifestService {
  @override
  Stream<void> changes() => const Stream.empty();
  @override
  Future<List<KubeConnection>> inventory() async => guideKubeConnections;
  @override
  Future<KubeSession> connect(String contextId) async => GuideKubeService.session;
  @override
  Future<List<KubeResourceKind>> discover(String sessionId) async => [];
  @override
  Future<List<String>> namespaces(String sessionId) async => ['shop'];
  @override
  Future<void> disconnect(String sessionId) async {}
}

class _DemoTransport extends Fake implements TerminalService, TcpdumpService, TmuxService {
  @override
  Future<TerminalSessionHandle> open({required ObjectId hostId, required TerminalSize size}) async =>
      TerminalSessionHandle(
        id: TerminalSessionId.generate(),
        output: const Stream.empty(),
        events: Stream.value(const TerminalStateChanged(SessionConnectionState.connected)),
      );

  @override
  Future<void> resize(TerminalSessionId id, TerminalSize size) async {}
  @override
  Future<void> write(TerminalSessionId id, Uint8List data) async {}
  @override
  Future<void> close(TerminalSessionId id) async {}
  @override
  Future<String?> tmuxVersion(TerminalSessionId parent) async => 'tmux 3.6';
  @override
  Future<List<TmuxSession>> tmuxSessions(TerminalSessionId parent) async => [
    for (final (index, name, path, windows) in [(1, 'deploy', '/srv/web', 2), (2, 'logs', '/var/log', 1)])
      TmuxSession.fromJson({
        'id': index,
        'pid': 1200 + index,
        'created': 40,
        'name': name,
        'path': path,
        'windows': windows,
        'clients': 0,
        'activity': DateTime.now().subtract(const Duration(minutes: 3)).millisecondsSinceEpoch ~/ 1000,
      }),
  ];

  @override
  Future<TerminalSessionHandle> openTcpdump({required ObjectId hostId, ObjectId? peerId, required bool sudo}) async {
    const client = '192.0.2.10.49020', server = '192.168.1.50.8080';
    const exchange = [
      '$client > $server: Flags [S], seq 420, win 64240, length 0',
      '$server > $client: Flags [S.], seq 1020, ack 421, win 65535, length 0',
      '$client > $server: Flags [.], ack 1021, win 64240, length 0',
      '$client > $server: Flags [P.], seq 421:443, ack 1021, win 64240, length 22',
      '$server > $client: Flags [.], ack 443, win 65535, length 0',
    ];
    final lines = ['tcpdump: listening on eth0'];
    for (final (index, packet) in exchange.indexed) {
      lines.add('1795597200.12010$index eth0 ${packet.startsWith(client) ? 'Out' : 'In'} IP $packet');
      if (index == 3) {
        lines.addAll([
          '0x0000:  4500 003e 0001 4000 4006 b62d c000 020a',
          '0x0010:  c0a8 0132 bf7c 1f90 0000 01a5 0000 03fd',
          '0x0020:  5018 faf0 1eb2 0000 4745 5420 2f68 6561',
          '0x0030:  6c74 6820 4854 5450 2f31 2e30 0d0a',
        ]);
      }
    }
    return TerminalSessionHandle(
      id: TerminalSessionId.generate(),
      output: Stream.value(Uint8List.fromList(utf8.encode('${lines.join('\n')}\n'))),
      events: Stream.value(const TerminalExited(0)),
    );
  }
}

class _DemoEngine extends MockDockerService implements DockerObservationService {
  @override
  Future<DockerJson> diskUsage(String sessionId) async => {
    'images': {'bytes': 4875878400, 'count': 14, 'reclaimable': 1386217472},
    'volumes': {'bytes': 1744830464, 'count': 6},
    'cache': {'bytes': 887095296, 'count': 18},
    'containers': {'bytes': 780140544, 'count': 8},
  };
  @override
  Future<DockerJson> telemetry(String sessionId, String containerId) async => {};
  @override
  Future<List<DockerImage>> images(String sessionId) async => [];
  @override
  Future<List<DockerJson>> networks(String sessionId) async => [];
  @override
  Future<List<DockerJson>> recentEvents(String sessionId) async => [];
  @override
  Future<bool> removeImage(String sessionId, String id, {String? tag}) async => false;
}

void main() {
  for (final locale in [AppLocale.ru, AppLocale.en, AppLocale.es, AppLocale.zh]) {
    for (final android in [false, true]) {
      testWidgets('guide ${locale.wireName} ${android ? 'tablet' : 'desktop'}', (tester) async {
        final backend = testBackend();
        addTearDown(backend.dispose);
        await backend.debugSignInDemoAndUnlock();
        await backend.inventory.createPasswordCredential(
          name: 'Build Hub',
          username: 'demo-team',
          registry: 'docker.io',
          password: SecretText('demo-only'),
        );
        await backend.services.settings.updateLocal(
          backend.services.settings.currentLocal.copyWith(themeMode: AppThemeMode.dark),
        );
        await loadAutomationProofFonts(tester);
        for (final (family, asset) in [
          ('ConsoleCryptTools', 'assets/fonts/ConsoleCryptTools.ttf'),
          ('ConsoleCryptSymbols', 'assets/nerd-fonts/ConsoleCryptSymbols.ttf'),
        ]) {
          await (FontLoader(family)..addFont(rootBundle.load(asset))).load();
        }
        if (locale == AppLocale.zh) {
          final font = io.File(
            io.Platform.environment['CONSOLECRYPT_UI_PROOF_TEXT_FONT'] ?? 'C:/Windows/Fonts/msyh.ttc',
          );
          await tester.runAsync(() async {
            for (final family in [
              'Roboto',
              'Segoe UI',
              'Segoe UI Variable Text',
              'Segoe UI Variable Display',
              'Ahem',
            ]) {
              await (FontLoader(family)..addFont(font.readAsBytes().then(ByteData.sublistView))).load();
            }
          });
        }
        await pumpApp(
          tester,
          backend,
          locale: locale,
          size: const Size(1440, 900),
          captureBoundary: true,
          overrides: [
            devicesProvider.overrideWith((ref) => const Stream.empty()),
            terminalServiceProvider.overrideWithValue(_DemoTransport()),
            if (!android) dockerServiceProvider.overrideWithValue(_DemoEngine()),
            if (!android) kubeServiceProvider.overrideWithValue(_DemoCluster()),
          ],
        );
        final context = tester.element(find.byType(Navigator).first);
        final providers = ProviderScope.containerOf(context);
        final router = providers.read(routerProvider);
        final host = backend.inventory.currentHosts.firstWhere((h) => h.name == 'staging-web');
        final tab = await providers.read(terminalTabsProvider.notifier).open(host);
        router.go(AppRoutes.terminal);
        await settle(tester);
        tab.terminal.write(
          '\x1b[2J\x1b[Hdeploy@staging-web:~\$ uname -sm\r\nLinux x86_64\r\ndeploy@staging-web:~\$ tmux ls\r\ndeploy: 2 windows\r\nlogs: 1 windows\r\ndeploy@staging-web:~\$ ',
        );
        await settle(tester);
        Future<void> shot(String topic) => captureAutomationWorkspace(tester, '$topic-${locale.wireName}');
        if (android) {
          await shot('android-wide-terminal');
        } else {
          await tapKey(tester, 'nav-tmux');
          expect(find.text('deploy'), findsWidgets);
          await shot('ssh-tmux-sessions');
          router.go(AppRoutes.tcpdump);
          await settle(tester);
          await tapKey(tester, 'tcpdump-source');
          await pickHost(tester, 'staging-web');
          await tapKey(tester, 'tcpdump-peer');
          await pickHost(tester, 'raspberry-pi');
          await tester.tap(find.text(context.l10n.captureStart));
          await settle(tester);
          await tester.tap(find.textContaining('PSH').first);
          await settle(tester);
          expect(find.textContaining('HEX / ASCII'), findsOneWidget);
          await shot('tcpdump-packet');
          router.go(AppRoutes.docker);
          await settle(tester);
          await tapKey(tester, 'docker-connect-ssh-web');
          await tapKey(tester, 'docker-view-disk');
          await shot('docker-engine-disk');
          await tapKey(tester, 'docker-view-containers');
          await tester.tap(find.text('shop-api-1').first);
          await settle(tester);
          await tapKey(tester, 'docker-details-kubernetes');
          await tapKey(tester, 'docker-kube-context');
          await tester.tap(find.text(guideKubeConnections.first.name).last);
          await settle(tester);
          await tapKey(tester, 'registry-picker-null');
          await tester.tap(find.text('Build Hub · docker.io').last);
          await settle(tester);
          await tapKey(tester, 'registry-build-mode');
          await enterKey(tester, 'registry-remote-folder', '/srv/shop-api');
          final platform = find.text('linux/arm64');
          expect(platform, findsOneWidget);
          await Scrollable.ensureVisible(tester.element(platform), alignment: 0.76);
          await settle(tester);
          expect(find.byKey(const ValueKey('docker-kube-error')), findsNothing);
          await shot('docker-kube-wizard');
        }
        expect(tester.takeException(), isNull);
      }, variant: TargetPlatformVariant.only(android ? TargetPlatform.android : TargetPlatform.windows));
    }
  }
}
