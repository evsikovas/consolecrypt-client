import 'dart:async';

import 'package:consolecrypt/app/workspace_forwards.dart';
import 'package:consolecrypt/app/workspace_tools_controller.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/mock/mock_backend.dart';
import 'package:consolecrypt/core/mock/mock_docker.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/security/secure_clipboard.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/core/services/terminal_service.dart';
import 'package:consolecrypt/docker/docker_container_list.dart';
import 'package:consolecrypt/docker/docker_format.dart';
import 'package:consolecrypt/docker/docker_host_path.dart';
import 'package:consolecrypt/docker/docker_providers.dart';
import 'package:consolecrypt/docker/docker_screen.dart';
import 'package:consolecrypt/docker/docker_service.dart';
import 'package:consolecrypt/docker/docker_terminal.dart';
import 'package:consolecrypt/sftp/sftp_controller.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/test_app.dart';

class _Docker extends MockDockerService {
  final forwardsStarted = <({String containerId, int remotePort, int localPort})>[];

  @override
  Future<DockerForward> startForward(
    String sessionId,
    String containerId, {
    required int remotePort,
    int localPort = 0,
  }) {
    forwardsStarted.add((containerId: containerId, remotePort: remotePort, localPort: localPort));
    return super.startForward(sessionId, containerId, remotePort: remotePort, localPort: localPort);
  }
}

/// Docker over SSH on the demo inventory host [hostId] (the engine data is
/// the mock's `ssh-web` host).
class _SshDocker extends _Docker {
  _SshDocker(this.hostId);
  final String hostId;

  @override
  Future<List<DockerHost>> hosts() async => [DockerHost(id: hostId, name: 'prod-web-1', kind: DockerHostKind.ssh)];

  @override
  Future<DockerSessionInfo> connect(String hostId) async {
    final info = await super.connect('ssh-web');
    return DockerSessionInfo(
      id: info.id,
      hostId: this.hostId,
      hostName: info.hostName,
      engineVersion: info.engineVersion,
      os: info.os,
      arch: info.arch,
    );
  }
}

/// Records what was copied and how (secrets are cleared automatically).
class _Clipboard extends SecureClipboard {
  final secrets = <String>[];
  final plain = <String>[];
  @override
  Future<void> copySecret(String value, {bool Function()? isCurrent}) async => secrets.add(value);
  @override
  Future<void> copyPlain(String value) async => plain.add(value);
}

class _Terminal implements TerminalService, DockerTerminalService {
  final targets = <DockerTerminalTarget>[];
  @override
  Future<TerminalSessionHandle> openDocker({required DockerTerminalTarget target, required TerminalSize size}) async {
    targets.add(target);
    return TerminalSessionHandle(
      id: TerminalSessionId.generate(),
      output: const Stream.empty(),
      events: Stream.value(const TerminalStateChanged(SessionConnectionState.connected)),
    );
  }

  @override
  Future<TerminalSessionHandle> open({required ObjectId hostId, required TerminalSize size}) =>
      throw UnimplementedError();
  @override
  Future<void> write(TerminalSessionId id, Uint8List data) async {}
  @override
  Future<void> resize(TerminalSessionId id, TerminalSize size) async {}
  @override
  Future<void> close(TerminalSessionId id) async {}
  @override
  Future<void> reconnect(TerminalSessionId id) async {}
  @override
  Future<void> answerHostKey(TerminalSessionId id, HostKeyDecision decision) async {}
  @override
  Future<void> answerPassword(TerminalSessionId id, SecretText? password) async => password?.wipe();
}

Future<void> _pump(
  WidgetTester tester, {
  DockerService? service,
  bool available = true,
  Size size = const Size(1400, 900),
  TerminalService? terminal,
  MockBackend? signedIn,
  ValueNotifier<DockerScope>? scope,
  SecureClipboard? clipboard,
}) async {
  final backend = signedIn ?? testBackend();
  addTearDown(backend.dispose);
  if (terminal != null) await backend.debugSignInDemoAndUnlock();
  final unlocked = terminal != null || signedIn != null;
  tester.view.physicalSize = size;
  tester.view.devicePixelRatio = 1;
  addTearDown(tester.view.reset);
  await tester.pumpWidget(
    ProviderScope(
      overrides: [
        appServicesProvider.overrideWithValue(backend.services),
        dockerServiceProvider.overrideWithValue(available ? service ?? _Docker() : null),
        if (scope == null)
          dockerScopeProvider.overrideWithValue(const DockerScope('profile', true))
        else
          dockerScopeProvider.overrideWith((ref) {
            void changed() => ref.invalidateSelf();
            scope.addListener(changed);
            ref.onDispose(() => scope.removeListener(changed));
            return scope.value;
          }),
        if (clipboard != null) secureClipboardProvider.overrideWithValue(clipboard),
        if (terminal != null) terminalServiceProvider.overrideWithValue(terminal),
        if (unlocked) activeProfileProvider.overrideWithValue(backend.profiles.currentProfiles.active),
      ],
      child: const MaterialApp(
        locale: Locale('en'),
        localizationsDelegates: [AppLocalizations.delegate, ...GlobalMaterialLocalizations.delegates],
        supportedLocales: AppLocalizations.supportedLocales,
        home: Scaffold(body: DockerScreen()),
      ),
    ),
  );
  await settle(tester);
  if (unlocked) {
    final container = ProviderScope.containerOf(tester.element(find.byType(DockerScreen)));
    container.read(activeProfileProvider);
    container.read(vaultStatusProvider);
    await settle(tester);
  }
}

Future<void> _connectLocal(WidgetTester tester) async {
  await tester.tap(find.byKey(const ValueKey('docker-connect-local')));
  await settle(tester);
}

/// Select all, then copy, with [modifier] held.
Future<void> _copyAllWithKeyboard(WidgetTester tester, LogicalKeyboardKey modifier) async {
  await tester.sendKeyDownEvent(modifier);
  await tester.sendKeyEvent(LogicalKeyboardKey.keyA);
  await tester.sendKeyEvent(LogicalKeyboardKey.keyC);
  await tester.sendKeyUpEvent(modifier);
  await settle(tester);
}

/// Full id of the container row showing [name].
String _id(WidgetTester tester, String name) => tester
    .widget<DockerContainerRow>(find.ancestor(of: find.text(name), matching: find.byType(DockerContainerRow)).first)
    .container
    .id;

void main() {
  testWidgets('without a native Docker service the section says it is unavailable', (tester) async {
    await _pump(tester, available: false);
    expect(find.byKey(const ValueKey('docker-unavailable')), findsOneWidget);
    expect(find.text('Docker is unavailable'), findsOneWidget);
  });

  testWidgets('picking a host lists containers grouped by compose project', (tester) async {
    await _pump(tester);
    expect(find.byKey(const ValueKey('docker-host-local')), findsOneWidget);
    expect(find.text('This computer'), findsOneWidget);
    expect(find.text('web-01'), findsOneWidget);
    await tester.enterText(find.byKey(const ValueKey('docker-host-search-input')), 'ci');
    await tester.pump();
    expect(find.text('web-01'), findsNothing);
    expect(find.text('ci-runner'), findsOneWidget);
    await tester.enterText(find.byKey(const ValueKey('docker-host-search-input')), '');
    await tester.pump();

    await _connectLocal(tester);
    expect(find.byKey(const ValueKey('docker-engine-header')), findsOneWidget);
    expect(find.textContaining('Engine 27.3.1'), findsOneWidget);
    expect(find.textContaining('linux/arm64'), findsOneWidget);
    expect(find.textContaining('5 of 8 running'), findsOneWidget);

    // Running compose projects alphabetically, loose containers after them.
    final monitoring = tester.getTopLeft(find.byKey(const ValueKey('docker-group-monitoring')));
    final shop = tester.getTopLeft(find.byKey(const ValueKey('docker-group-shop')));
    final standalone = tester.getTopLeft(find.byKey(const ValueKey('docker-group-')));
    expect(monitoring.dy, lessThan(shop.dy));
    expect(shop.dy, lessThan(standalone.dy));
    expect(tester.widget<Text>(find.byKey(const ValueKey('docker-group-title-'))).data, 'Containers');
    expect(tester.widget<Text>(find.byKey(const ValueKey('docker-group-count-shop'))).data, '4/4 running');
    expect(tester.widget<Text>(find.byKey(const ValueKey('docker-group-count-monitoring'))).data, '1/2 running');
    expect(find.text('(web)'), findsOneWidget);
    expect(find.text('0.0.0.0:8080→80/tcp'), findsOneWidget);
    expect(find.text('127.0.0.1:9090→9090/tcp'), findsOneWidget);
    final api = _id(tester, 'shop-api-1');
    expect(find.byKey(ValueKey('docker-health-$api')), findsOneWidget);
    expect(find.textContaining(api.substring(0, 12)), findsWidgets);

    // Collapse a group.
    await tester.tap(find.byKey(const ValueKey('docker-group-shop')));
    await settle(tester);
    expect(find.text('shop-web-1'), findsNothing);
    await tester.tap(find.byKey(const ValueKey('docker-group-shop')));
    await settle(tester);
    expect(find.text('shop-web-1'), findsOneWidget);

    // Hide stopped containers and filter by image.
    expect(find.text('monitoring-grafana-1'), findsOneWidget);
    await tester.tap(find.byKey(const ValueKey('docker-filter-running')));
    await settle(tester);
    expect(find.text('monitoring-grafana-1'), findsNothing);
    expect(find.text('backup-once'), findsNothing);
    expect(find.text('queue-worker'), findsOneWidget);
    await tester.enterText(find.byKey(const ValueKey('docker-filter-input')), 'postgres');
    await tester.pump();
    expect(find.text('shop-postgres-1'), findsOneWidget);
    expect(find.text('shop-web-1'), findsNothing);
    expect(find.byKey(const ValueKey('docker-group-monitoring')), findsNothing);
    expect(tester.takeException(), isNull);
  });

  testWidgets('a permission error explains the docker group and offers the command to copy', (tester) async {
    final service = _Docker();
    await _pump(tester, service: service);
    await tester.tap(find.byKey(const ValueKey('docker-connect-${MockDockerService.permissionDeniedHostId}')));
    await settle(tester);
    expect(find.byKey(const ValueKey('docker-error-permission')), findsOneWidget);
    expect(find.text('No access to the Docker socket'), findsOneWidget);
    expect(find.textContaining('“docker” group'), findsOneWidget);
    final command = tester.widget<SelectableText>(find.byKey(const ValueKey('docker-permission-command')));
    expect(command.data, startsWith('sudo usermod -aG docker '));
    expect(find.byKey(const ValueKey('docker-permission-copy')), findsOneWidget);
    expect(find.byKey(const ValueKey('docker-engine-header')), findsNothing);
    // Retrying fails the same way and never runs anything on the host.
    await tester.tap(find.byKey(const ValueKey('docker-connect-retry')));
    await settle(tester);
    expect(find.byKey(const ValueKey('docker-error-permission')), findsOneWidget);
    expect(service.actions, isEmpty);
  });

  testWidgets('other connect failures are explained', (tester) async {
    final service = _ThrowingDocker(const DockerException(DockerFailure.notRunning));
    await _pump(tester, service: service);
    await _connectLocal(tester);
    expect(find.byKey(const ValueKey('docker-error-not-running')), findsOneWidget);
    service.error = const DockerException(DockerFailure.connectionFailed, 'ssh: handshake failed');
    await tester.tap(find.byKey(const ValueKey('docker-connect-retry')));
    await settle(tester);
    expect(find.byKey(const ValueKey('docker-error-connection')), findsOneWidget);
    expect(find.textContaining('ssh: handshake failed'), findsOneWidget);
  });

  testWidgets('row actions call the service; kill and remove ask first', (tester) async {
    final service = _Docker();
    await _pump(tester, service: service);
    await _connectLocal(tester);
    final web = _id(tester, 'shop-web-1');
    await tapKey(tester, 'docker-stop-$web');
    expect(service.actions, [(web, DockerAction.stop)]);
    expect(find.byKey(ValueKey('docker-start-$web')), findsOneWidget);

    final grafana = _id(tester, 'monitoring-grafana-1');
    await tapKey(tester, 'docker-menu-button-$grafana');
    await tapKey(tester, 'docker-menu-remove');
    expect(find.text('Remove monitoring-grafana-1?'), findsOneWidget);
    await tester.tap(find.byKey(const ValueKey('confirm-cancel')));
    await settle(tester);
    expect(service.actions.where((a) => a.$2 == DockerAction.remove), isEmpty);
    expect(find.text('monitoring-grafana-1'), findsOneWidget);

    await tapKey(tester, 'docker-menu-button-$grafana');
    await tapKey(tester, 'docker-menu-remove');
    await tester.tap(find.byKey(const ValueKey('confirm-ok')));
    await settle(tester);
    expect(service.actions.last, (grafana, DockerAction.remove));
    expect(find.text('monitoring-grafana-1'), findsNothing);

    final redis = _id(tester, 'shop-redis-1');
    await tapKey(tester, 'docker-menu-button-$redis');
    await tapKey(tester, 'docker-menu-kill');
    expect(find.text('Kill shop-redis-1?'), findsOneWidget);
    await tester.tap(find.byKey(const ValueKey('confirm-ok')));
    await settle(tester);
    expect(service.actions.last, (redis, DockerAction.kill));
    expect(tester.takeException(), isNull);
  });

  testWidgets('details show inspect data, logs with stderr in its own colour, and JSON', (tester) async {
    await _pump(tester);
    await _connectLocal(tester);
    await tester.ensureVisible(find.text('shop-postgres-1'));
    await tester.tap(find.text('shop-postgres-1'));
    await settle(tester);
    expect(find.byKey(const ValueKey('docker-container-details')), findsOneWidget);
    expect(find.text('postgres:16'), findsWidgets);
    final overview = find.byKey(const ValueKey('docker-overview'));
    for (final text in ['172.18.0.2', '/var/lib/postgresql/data', 'POSTGRES_PASSWORD=••••••']) {
      final finder = find.textContaining(text);
      await tester.dragUntilVisible(finder, overview, const Offset(0, -150));
      expect(finder, findsOneWidget);
    }
    expect(find.byKey(const ValueKey('docker-mounts')), findsOneWidget);

    // Secrets appear only after the vault passphrase.
    await tapKey(tester, 'docker-env-reveal');
    await tester.enterText(find.byKey(const ValueKey('docker-reveal-passphrase')), 'wrong');
    await tapKey(tester, 'docker-reveal-submit');
    expect(find.text('Wrong vault passphrase.'), findsOneWidget);
    await tester.enterText(find.byKey(const ValueKey('docker-reveal-passphrase')), MockDockerService.demoPassphrase);
    await tapKey(tester, 'docker-reveal-submit');
    expect(find.byKey(const ValueKey('docker-reveal-dialog')), findsNothing);
    expect(find.textContaining('POSTGRES_PASSWORD=demo-postgres-pass'), findsOneWidget);
    await tapKey(tester, 'docker-env-hide');
    expect(find.textContaining('demo-postgres-pass'), findsNothing);

    final api = _id(tester, 'shop-api-1');
    await tapKey(tester, 'docker-logs-$api');
    expect(find.text('shop-api-1'), findsWidgets);
    expect(find.byKey(const ValueKey('docker-log-lines')), findsOneWidget);
    final stdout = tester.widget<Text>(find.text('Starting service…'));
    final stderr = tester.widget<Text>(find.text('deprecation: config key "workers" is ignored'));
    final palette = GlassTokens.of(tester.element(find.byKey(const ValueKey('docker-log-lines')))).palette;
    expect(stderr.style?.color, palette.danger);
    expect(stdout.style?.color, palette.label);

    await tester.enterText(find.byKey(const ValueKey('docker-log-find-input')), 'deprecation');
    await tester.pump();
    expect(find.text('Starting service…'), findsNothing);
    expect(find.text('1 match'), findsOneWidget);
    await tester.enterText(find.byKey(const ValueKey('docker-log-find-input')), '');
    await tester.pump();

    await tester.tap(find.byKey(const ValueKey('docker-log-timestamps')));
    await settle(tester);
    expect(find.textContaining('2026-10-21T09:00:00'), findsWidgets);

    await tester.tap(find.byKey(const ValueKey('docker-log-clear')));
    await tester.pump();
    expect(find.byKey(const ValueKey('docker-log-lines')), findsNothing);

    await tester.tap(find.byKey(const ValueKey('docker-tab-json')));
    await settle(tester);
    expect(find.byKey(const ValueKey('docker-inspect-json')), findsOneWidget);
    expect(find.byKey(const ValueKey('docker-json-copy')), findsOneWidget);

    await tester.tap(find.byKey(const ValueKey('docker-details-close')));
    await settle(tester);
    expect(find.byKey(const ValueKey('docker-container-details')), findsNothing);
    expect(tester.takeException(), isNull);
  });

  testWidgets(
    'inspect JSON hides secrets until the passphrase; the full JSON is copied as a secret and dropped on lock',
    (tester) async {
      final scope = ValueNotifier(const DockerScope('profile', true));
      final clipboard = _Clipboard();
      await _pump(tester, scope: scope, clipboard: clipboard);
      await _connectLocal(tester);
      await tester.ensureVisible(find.text('shop-redis-1'));
      await tester.tap(find.text('shop-redis-1'));
      await settle(tester);
      // The overview's command line comes from the masked inspect.
      expect(find.textContaining('redis-server --requirepass ••••••'), findsOneWidget);
      expect(find.textContaining('demo-redis-pass'), findsNothing);

      await tester.tap(find.byKey(const ValueKey('docker-tab-json')));
      await settle(tester);
      expect(find.byKey(const ValueKey('docker-json-masked')), findsOneWidget);
      await tester.tap(find.byTooltip('Show full value'));
      await settle(tester);
      String raw() => tester
          .widgetList<SelectableText>(
            find.descendant(
              of: find.byKey(const ValueKey('docker-inspect-json')),
              matching: find.byType(SelectableText),
            ),
          )
          .map((text) => text.data ?? '')
          .join();
      expect(raw(), contains('"--requirepass",\n      "••••••"'));
      expect(raw(), isNot(contains('demo-redis-pass')));
      // The masked JSON is ordinary text.
      await tapKey(tester, 'docker-json-copy');
      expect(clipboard.plain.single, isNot(contains('demo-redis-pass')));

      await tapKey(tester, 'docker-json-reveal');
      expect(find.byKey(const ValueKey('docker-json-reveal-dialog')), findsOneWidget);
      await tester.enterText(find.byKey(const ValueKey('docker-reveal-passphrase')), 'wrong');
      await tapKey(tester, 'docker-reveal-submit');
      expect(find.text('Wrong vault passphrase.'), findsOneWidget);
      await tester.enterText(find.byKey(const ValueKey('docker-reveal-passphrase')), MockDockerService.demoPassphrase);
      await tapKey(tester, 'docker-reveal-submit');
      expect(find.byKey(const ValueKey('docker-json-hide')), findsOneWidget);
      await tester.tap(find.byTooltip('Show full value'));
      await settle(tester);
      expect(raw(), contains('demo-redis-pass'));
      await tapKey(tester, 'docker-json-copy');
      expect(clipboard.secrets.single, contains('demo-redis-pass'));
      expect(clipboard.plain, hasLength(1));

      // Locking the vault drops the full JSON.
      scope.value = const DockerScope('profile', false);
      await settle(tester);
      expect(find.textContaining('demo-redis-pass'), findsNothing);
    },
  );

  testWidgets('the full JSON is dropped when the card closes or reloads', (tester) async {
    await _pump(tester);
    await _connectLocal(tester);
    await tester.ensureVisible(find.text('shop-api-1'));
    await tester.tap(find.text('shop-api-1'));
    await settle(tester);
    // Overview: the URL password and the token label are masked.
    final overview = find.byKey(const ValueKey('docker-overview'));
    for (final text in ['DATABASE_URL=postgres://shop:••••••@postgres:5432/shop', 'com.example.deploy.token']) {
      final finder = find.textContaining(text);
      await tester.dragUntilVisible(finder, overview, const Offset(0, -150));
      expect(finder, findsWidgets);
    }
    expect(find.textContaining('demo-'), findsNothing);
    await tester.tap(find.byKey(const ValueKey('docker-tab-json')));
    await settle(tester);
    await tapKey(tester, 'docker-json-reveal');
    await tester.enterText(find.byKey(const ValueKey('docker-reveal-passphrase')), MockDockerService.demoPassphrase);
    await tapKey(tester, 'docker-reveal-submit');
    expect(find.byKey(const ValueKey('docker-json-full-view')), findsOneWidget);
    await tapKey(tester, 'docker-details-refresh');
    expect(find.byKey(const ValueKey('docker-json-masked-view')), findsOneWidget);
    expect(find.byKey(const ValueKey('docker-json-reveal')), findsOneWidget);
    await tester.tap(find.byKey(const ValueKey('docker-details-close')));
    await settle(tester);
    await tester.tap(find.text('shop-api-1'));
    await settle(tester);
    await tester.tap(find.byKey(const ValueKey('docker-tab-json')));
    await settle(tester);
    expect(find.byKey(const ValueKey('docker-json-masked-view')), findsOneWidget);
  });

  testWidgets('copied log text with a secret goes to the clipboard as a secret', (tester) async {
    final clipboard = _Clipboard();
    await _pump(tester, clipboard: clipboard);
    await _connectLocal(tester);
    final api = _id(tester, 'shop-api-1');
    await tapKey(tester, 'docker-logs-$api');
    // Logs are shown as they are.
    expect(find.textContaining('postgres://shop:demo-postgres-pass@'), findsOneWidget);
    await tapKey(tester, 'docker-log-copy');
    expect(clipboard.secrets.single, contains('demo-postgres-pass'));
    expect(clipboard.plain, isEmpty);

    await tester.enterText(find.byKey(const ValueKey('docker-log-find-input')), 'GET /api');
    await tester.pump();
    await tapKey(tester, 'docker-log-copy');
    expect(clipboard.plain.single, startsWith('GET /api/orders/'));

    // A selection copied with the keyboard takes the same path.
    await tester.enterText(find.byKey(const ValueKey('docker-log-find-input')), 'connecting');
    await tester.pump();
    await tester.tap(find.textContaining('connecting to'));
    await tester.pump();
    await _copyAllWithKeyboard(tester, LogicalKeyboardKey.controlLeft);
    await settle(tester);
    expect(clipboard.secrets, hasLength(2));
    expect(clipboard.secrets.last, contains('demo-postgres-pass'));
  });

  testWidgets('on macOS Cmd+C on selected log lines takes the secret path too', (tester) async {
    final clipboard = _Clipboard();
    await _pump(tester, clipboard: clipboard);
    await _connectLocal(tester);
    await tapKey(tester, 'docker-logs-${_id(tester, 'shop-api-1')}');
    await tester.tap(find.textContaining('connecting to'));
    await tester.pump();
    await _copyAllWithKeyboard(tester, LogicalKeyboardKey.metaLeft);
    expect(clipboard.secrets.single, contains('demo-postgres-pass'));
    expect(clipboard.plain, isEmpty);
  }, variant: TargetPlatformVariant.only(TargetPlatform.macOS));

  testWidgets('the first container terminal of a profile says it gives full access, once', (tester) async {
    final terminal = _Terminal();
    await _pump(tester, terminal: terminal);
    await _connectLocal(tester);
    final web = _id(tester, 'shop-web-1');
    await tapKey(tester, 'docker-terminal-$web');
    expect(find.byKey(const ValueKey('docker-terminal-hint')), findsOneWidget);
    expect(
      find.text('The terminal gives full access to the container, including its environment variables.'),
      findsOneWidget,
    );
    await tester.tap(find.byKey(const ValueKey('docker-terminal-open')));
    await settle(tester);
    await tapKey(tester, 'docker-terminal-$web');
    expect(find.byKey(const ValueKey('docker-terminal-dialog')), findsOneWidget);
    expect(find.byKey(const ValueKey('docker-terminal-hint')), findsNothing);
  });

  testWidgets('forward dialog is prefilled with the container ports and starts a forward', (tester) async {
    final service = _Docker();
    await _pump(tester, service: service);
    await _connectLocal(tester);
    final postgres = _id(tester, 'shop-postgres-1');
    await tapKey(tester, 'docker-menu-button-$postgres');
    await tapKey(tester, 'docker-menu-forward');
    expect(find.byKey(const ValueKey('docker-forward-dialog')), findsOneWidget);
    expect(tester.widget<TextField>(find.byKey(const ValueKey('docker-forward-remote'))).controller!.text, '5432');
    expect(tester.widget<TextField>(find.byKey(const ValueKey('docker-forward-local'))).controller!.text, '0');
    expect(find.byKey(const ValueKey('docker-forward-port-5432')), findsOneWidget);
    await tester.enterText(find.byKey(const ValueKey('docker-forward-local')), '15432');
    await tester.pump();
    await tester.tap(find.byKey(const ValueKey('docker-forward-start')));
    await settle(tester);
    expect(service.forwardsStarted, [(containerId: postgres, remotePort: 5432, localPort: 15432)]);
    expect(find.text('127.0.0.1:15432'), findsOneWidget);
    await tester.tap(find.byKey(const ValueKey('docker-forward-done')));
    await settle(tester);

    // The shared right panel (with the Kubernetes forwards) opens.
    final container = ProviderScope.containerOf(tester.element(find.byType(DockerScreen)));
    expect(container.read(workspaceToolsProvider).selected, WorkspaceTool.forwards);
    expect((await service.forwards()).single.containerName, 'shop-postgres-1');
    expect(tester.takeException(), isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.macOS));

  testWidgets('the shared forwards panel lists and stops Docker forwards', (tester) async {
    final service = _Docker();
    final session = await service.connect(DockerHost.localId);
    final container = (await service.containers(session.id)).firstWhere((c) => c.name == 'shop-postgres-1');
    final forward = await service.startForward(session.id, container.id, remotePort: 5432);
    final backend = testBackend();
    addTearDown(backend.dispose);
    await tester.pumpWidget(
      ProviderScope(
        overrides: [
          appServicesProvider.overrideWithValue(backend.services),
          dockerServiceProvider.overrideWithValue(service),
          dockerScopeProvider.overrideWithValue(const DockerScope('profile', true)),
        ],
        child: const MaterialApp(
          locale: Locale('en'),
          localizationsDelegates: [AppLocalizations.delegate, ...GlobalMaterialLocalizations.delegates],
          supportedLocales: AppLocalizations.supportedLocales,
          home: Scaffold(body: WorkspaceForwardsPanel()),
        ),
      ),
    );
    await settle(tester);
    expect(find.text('Docker'), findsOneWidget);
    expect(find.text('→ shop-postgres-1:5432'), findsOneWidget);
    await tester.tap(find.byKey(ValueKey('docker-forward-stop-${forward.id}')));
    await settle(tester);
    expect(await service.forwards(), isEmpty);
    expect(tester.takeException(), isNull);
  });

  testWidgets('terminal asks for the shell and opens a container terminal tab', (tester) async {
    final terminal = _Terminal();
    await _pump(tester, terminal: terminal);
    await _connectLocal(tester);
    final web = _id(tester, 'shop-web-1');
    await tapKey(tester, 'docker-terminal-$web');
    expect(find.byKey(const ValueKey('docker-terminal-dialog')), findsOneWidget);
    expect(find.text('Detect automatically (bash, ash or sh)'), findsOneWidget);
    await tester.tap(find.byKey(const ValueKey('docker-terminal-shell')));
    await settle(tester);
    await tester.tap(find.text('/bin/bash').last);
    await settle(tester);
    await tester.tap(find.byKey(const ValueKey('docker-terminal-open')));
    await settle(tester);
    expect(terminal.targets, hasLength(1));
    final target = terminal.targets.single;
    expect(target.containerId, web);
    expect(target.containerName, 'shop-web-1');
    expect(target.command, ['/bin/bash']);
    expect(target.sessionId, isNotEmpty);
    expect(find.byKey(const ValueKey('docker-terminal-dialog')), findsNothing);

    // The default is the auto-detect marker.
    await tapKey(tester, 'docker-terminal-$web');
    await tester.tap(find.byKey(const ValueKey('docker-terminal-open')));
    await settle(tester);
    expect(terminal.targets.last.command, [dockerAutoShell]);
  });

  testWidgets('narrow layout pushes details and forwards without overflow', (tester) async {
    await _pump(tester, size: const Size(390, 844));
    expect(tester.takeException(), isNull);
    await _connectLocal(tester);
    expect(tester.takeException(), isNull);
    await tester.dragUntilVisible(
      find.text('shop-web-1'),
      find.byKey(const ValueKey('docker-containers')),
      const Offset(0, -200),
    );
    await settle(tester);
    await tester.tap(find.text('shop-web-1'));
    await settle(tester);
    expect(find.byKey(const ValueKey('docker-container-details')), findsOneWidget);
    await tester.tap(find.byKey(const ValueKey('docker-tab-logs')));
    await settle(tester);
    expect(find.byKey(const ValueKey('docker-log-lines')), findsOneWidget);
    await tester.tap(find.byKey(const ValueKey('docker-details-close')));
    await settle(tester);
    expect(find.byKey(const ValueKey('docker-container-details')), findsNothing);
    await tester.tap(find.byKey(const ValueKey('docker-forwards-toggle')));
    await settle(tester);
    expect(find.byKey(const ValueKey('docker-forwards-panel')), findsOneWidget);
    await tester.tap(find.byKey(const ValueKey('docker-forwards-close')));
    await settle(tester);
    expect(find.byKey(const ValueKey('docker-forwards-panel')), findsNothing);
    // Scroll through the whole list.
    await tester.drag(find.byKey(const ValueKey('docker-containers')), const Offset(0, -2000));
    await settle(tester);
    expect(tester.takeException(), isNull);
  });

  group('host paths open the folder', () {
    const nginx = '/srv/shop/nginx.conf';
    const shopVolume = '/var/lib/docker/volumes/shop_data/_data';

    Future<void> openWebDetails(WidgetTester tester) async {
      await tester.ensureVisible(find.text('shop-web-1'));
      await tester.tap(find.text('shop-web-1'));
      await settle(tester);
      await tester.dragUntilVisible(
        find.byKey(const ValueKey('docker-mounts')),
        find.byKey(const ValueKey('docker-overview')),
        const Offset(0, -150),
      );
      await settle(tester);
    }

    SftpState sftp(WidgetTester tester) =>
        ProviderScope.containerOf(tester.element(find.byType(DockerScreen))).read(sftpControllerProvider);

    testWidgets('SSH: a bind file opens its folder in SFTP of that host with the file selected; a volume its '
        'mount point, unreadable folders say permission denied', (tester) async {
      final backend = testBackend();
      await backend.debugSignInDemoAndUnlock();
      final host = backend.inventory.currentHosts.firstWhere((h) => h.name == 'prod-web-1');
      await _pump(tester, service: _SshDocker(host.id.value), signedIn: backend);
      await tapKey(tester, 'docker-connect-${host.id.value}');
      await openWebDetails(tester);

      final link = find.byKey(const ValueKey('docker-path-$nginx'));
      expect(link, findsOneWidget);
      expect(find.byTooltip('Open in SFTP'), findsWidgets);
      await tester.tap(link);
      await settle(tester);
      var state = sftp(tester);
      expect(state.host?.id, host.id);
      expect(state.remotePath, '/srv/shop', reason: 'a file opens its parent folder');
      expect(state.selection, {nginx});

      // Volumes view: the mount point, in the same SFTP session.
      final session = state.session;
      await tapKey(tester, 'docker-view-volumes');
      await tapKey(tester, 'docker-path-$shopVolume');
      state = sftp(tester);
      expect(state.session, session);
      expect(state.remotePath, shopVolume);
      expect(state.currentFailure?.code, AppErrorCode.forbidden, reason: '/var/lib/docker is root-only (710)');
      expect(tester.takeException(), isNull);
    });

    testWidgets('local Docker Desktop: volumes are not clickable and say why; binds reveal in Finder', (tester) async {
      await _pump(tester);
      await _connectLocal(tester);
      await openWebDetails(tester);
      expect(find.byKey(const ValueKey('docker-path-$nginx')), findsOneWidget);
      expect(find.byTooltip('Show in Finder'), findsWidgets);
      await tester.tap(find.byKey(const ValueKey('docker-details-close')));
      await settle(tester);

      await tapKey(tester, 'docker-view-volumes');
      expect(find.byKey(const ValueKey('docker-path-$shopVolume')), findsNothing);
      final disabled = find.byKey(const ValueKey('docker-path-disabled-$shopVolume'));
      expect(disabled, findsOneWidget);
      final tooltip = tester.widget<Tooltip>(find.ancestor(of: disabled, matching: find.byType(Tooltip)).first);
      expect(tooltip.message, contains('Docker virtual machine'));
      await tester.tap(disabled);
      await settle(tester);
      expect(sftp(tester).host, isNull, reason: 'nothing is opened');
    }, variant: TargetPlatformVariant.only(TargetPlatform.macOS));

    testWidgets('local Docker on Linux: volume mount points are real local folders', (tester) async {
      await _pump(tester);
      await _connectLocal(tester);
      await tapKey(tester, 'docker-view-volumes');
      expect(find.byKey(const ValueKey('docker-path-$shopVolume')), findsOneWidget);
      expect(find.byKey(const ValueKey('docker-path-disabled-$shopVolume')), findsNothing);
    }, variant: TargetPlatformVariant.only(TargetPlatform.linux));

    test('Docker Desktop bind sources map to paths on this computer', () {
      expect(dockerLocalPath('/host_mnt/Users/me/app'), '/Users/me/app');
      expect(dockerLocalPath('/run/desktop/mnt/host/c/Users/me/app', windows: true), r'C:\Users\me\app');
      expect(dockerLocalPath('/opt/x/postgres-init.sql', windows: false), '/opt/x/postgres-init.sql');
    });
  });

  testWidgets('disconnecting returns to the host picker', (tester) async {
    await _pump(tester);
    await _connectLocal(tester);
    await tester.tap(find.byKey(const ValueKey('docker-disconnect')));
    await settle(tester);
    expect(find.byKey(const ValueKey('docker-host-local')), findsOneWidget);
  });

  testWidgets('the sidebar opens the Docker section of the app', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    await pumpApp(tester, backend);
    await tapKey(tester, 'nav-docker');
    expect(find.byType(DockerScreen), findsOneWidget);
    await _connectLocal(tester);
    expect(find.text('shop-web-1'), findsOneWidget);
    await tapKey(tester, 'nav-hosts');
    await tapKey(tester, 'nav-docker');
    // The engine session survives switching sections.
    expect(find.byKey(const ValueKey('docker-engine-header')), findsOneWidget);
    expect(tester.takeException(), isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.macOS));

  group('formatting', () {
    test('ports show the published binding', () {
      expect(
        formatDockerPort(const DockerPort(privatePort: 80, publicPort: 8080, protocol: 'tcp', ip: '0.0.0.0')),
        '0.0.0.0:8080→80/tcp',
      );
      expect(formatDockerPort(const DockerPort(privatePort: 53, protocol: 'udp')), '53/udp');
      expect(
        formatDockerPort(const DockerPort(privatePort: 80, publicPort: 80, protocol: 'tcp', ip: '::')),
        '[::]:80→80/tcp',
      );
    });

    testWidgets('volumes view lists volumes with the containers using them', (tester) async {
      await _pump(tester);
      await _connectLocal(tester);
      await tapKey(tester, 'docker-view-volumes');
      expect(find.byKey(const ValueKey('docker-volumes')), findsOneWidget);
      expect(find.text('shop_data'), findsOneWidget);
      expect(find.text('orphaned-cache'), findsOneWidget);
      expect(find.text('not used'), findsOneWidget);
      expect(find.byKey(const ValueKey('docker-state-filter')), findsNothing);
      await tapKey(tester, 'docker-view-containers');
      expect(find.byKey(const ValueKey('docker-state-filter')), findsOneWidget);
    });

    test('groups sort projects and keep standalone containers last', () {
      DockerContainer c(String name, {String? project, DockerContainerState state = DockerContainerState.running}) =>
          DockerContainer(id: name, name: name, image: 'img', state: state, composeProject: project);
      final groups = groupDockerContainers([
        c('solo'),
        c('b1', project: 'beta'),
        c('a1', project: 'alpha', state: DockerContainerState.exited),
        c('a2', project: 'alpha'),
      ]);
      expect(groups.map((g) => g.project), ['alpha', 'beta', null]);
      expect((groups.first.running, groups.first.total), (1, 2));
      final running = groupDockerContainers([
        c('a1', project: 'alpha', state: DockerContainerState.exited),
      ], filter: DockerStateFilter.running);
      expect(running, isEmpty);
    });

    test('running groups and containers come first, stopped ones last', () {
      DockerContainer c(String name, {String? project, DockerContainerState state = DockerContainerState.running}) =>
          DockerContainer(id: name, name: name, image: 'img', state: state, composeProject: project);
      final groups = groupDockerContainers([
        c('z-old', project: 'archive', state: DockerContainerState.exited),
        c('solo-stopped', state: DockerContainerState.exited),
        c('solo-up'),
        c('b-stopped', project: 'shop', state: DockerContainerState.exited),
        c('a-up', project: 'shop'),
      ]);
      expect(groups.map((g) => g.project), ['shop', null, 'archive']);
      expect(groups.first.containers.map((c) => c.name), ['a-up', 'b-stopped']);
      expect(groups[1].containers.map((c) => c.name), ['solo-up', 'solo-stopped']);
      final stopped = groupDockerContainers([
        c('up', project: 'shop'),
        c('down', project: 'shop', state: DockerContainerState.exited),
      ], filter: DockerStateFilter.stopped);
      expect(stopped.single.containers.map((c) => c.name), ['down']);
    });
  });
}

class _ThrowingDocker extends MockDockerService {
  _ThrowingDocker(this.error);
  DockerException error;
  @override
  Future<DockerSessionInfo> connect(String hostId) async => throw error;
}
