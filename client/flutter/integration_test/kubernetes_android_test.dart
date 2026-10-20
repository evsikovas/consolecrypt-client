// Opt-in smoke against an explicitly provisioned disposable cluster. The
// kubeconfig is copied into app-private storage by the runner, never compiled
// into the APK. A namespace marker is checked before exec/forward operations.
import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:consolecrypt/core/bridge/rust_app_services.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/services/terminal_service.dart';
import 'package:consolecrypt/kubernetes/kube_browser.dart';
import 'package:consolecrypt/kubernetes/kube_browser_service.dart';
import 'package:consolecrypt/kubernetes/kube_forward_service.dart';
import 'package:consolecrypt/kubernetes/kube_service.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:integration_test/integration_test.dart';
import 'package:material_ui/material_ui.dart';

const _configPath = String.fromEnvironment('CC_KUBE_SMOKE_CONFIG');
const _externalStop = bool.fromEnvironment('CC_KUBE_SMOKE_NOTIFICATION_STOP');
const _namespace = 'cc-acceptance';

void main() {
  IntegrationTestWidgetsFlutterBinding.ensureInitialized();
  testWidgets('Android native Kubernetes import, TLS, resources, logs, PTY and foreground forward', (tester) async {
    if (!Platform.isAndroid || _configPath.isEmpty) return;
    const platform = MethodChannel('consolecrypt/android');
    final privateRoot = (await platform.invokeMethod<String>('dataDirectory'))!;
    final config = File(_configPath);
    // Wait for the runner to populate the private fixture after APK install.
    final configDeadline = DateTime.now().add(const Duration(minutes: 2));
    while (!config.existsSync()) {
      if (DateTime.now().isAfter(configDeadline)) fail('private acceptance kubeconfig not supplied');
      await Future<void>.delayed(const Duration(milliseconds: 200));
    }
    final root = await Directory(privateRoot).createTemp('kube-it-');
    final ready = File('$privateRoot/kube-smoke-ready.json');
    final rust = await RustAppServices.open(
      RustCoreOptions(
        dataDir: '${root.path}/data',
        keychainService: 'io.consolecrypt.kube.it-${DateTime.now().microsecondsSinceEpoch}',
        fastKdfForTests: true,
        backgroundSync: false,
        autoStartTunnels: false,
        hookAppExit: false,
      ),
    );
    final services = rust.services;
    KubeSession? session;
    try {
      await services.profiles.createLocalProfile(name: 'Kubernetes Android acceptance');
      await services.vault.createVault(name: 'Acceptance', passphrase: SecretText('it-${ObjectId.generate().value}'));
      await services.vault.confirmRecoveryKitSaved();
      final kube = services.kubernetes!;
      final secret = SecretText(await config.readAsString());
      late KubeImportPreview preview;
      try {
        preview = await kube.preview(secret);
      } finally {
        secret.wipe();
        await config.delete();
      }
      expect(preview.entries, hasLength(1));
      expect(preview.entries.single.error, isNull);
      await kube.commit(preview.ticket, [KubeImportChoice(index: preview.entries.single.index, production: false)]);
      final connection = (await kube.inventory()).single;
      final browser = kube as KubeBrowserService;
      session = await browser.connect(connection.id);
      final kinds = await browser.discover(session.id);
      final namespaces = kinds.singleWhere((kind) => kind.group.isEmpty && kind.resource == 'namespaces');
      final marker = await browser.getResource(session.id, KubeResourceRef(api: namespaces, name: _namespace));
      final metadata = marker.object['metadata'] as Map;
      final labels = metadata['labels'] as Map;
      expect(
        labels['consolecrypt.dev/acceptance'],
        '0.6.2',
        reason: 'only the explicitly labelled disposable namespace may be exercised',
      );
      final podKind = kinds.singleWhere((kind) => kind.group.isEmpty && kind.resource == 'pods');
      final pods = await browser.listResources(session.id, podKind, namespace: _namespace);
      expect(pods.items.map((pod) => pod.name), contains('fixture-shell'));
      final pod = KubeResourceRef(api: podKind, namespace: _namespace, name: 'fixture-shell');
      expect(await browser.logs(session.id, pod), isNotEmpty);

      final terminal = await services.terminal.openTarget(
        target: KubernetesTerminalTarget(
          sessionId: session.id,
          contextName: connection.name,
          namespace: _namespace,
          pod: 'fixture-shell',
          tty: false,
          command: const ['/bin/sh', '-c', 'printf CC_ANDROID_PTY'],
        ),
        size: TerminalSize.initial,
      );
      final output = StringBuffer();
      final terminalDone = Completer<void>();
      final subscription = terminal.output.listen((bytes) {
        output.write(utf8.decode(bytes, allowMalformed: true));
        if (output.toString().contains('CC_ANDROID_PTY') && !terminalDone.isCompleted) terminalDone.complete();
      });
      try {
        await terminalDone.future.timeout(const Duration(seconds: 30));
      } finally {
        await services.terminal.close(terminal.id);
        await subscription.cancel();
      }

      // Render the actual production browser while the native-only assertions
      // use their independently revocable session.
      await tester.pumpWidget(
        ProviderScope(
          overrides: [appServicesProvider.overrideWithValue(services)],
          child: MaterialApp(
            locale: const Locale('en'),
            localizationsDelegates: const [AppLocalizations.delegate, ...GlobalMaterialLocalizations.delegates],
            supportedLocales: AppLocalizations.supportedLocales,
            home: Scaffold(
              body: KubeBrowser(connection: connection, service: browser, onClose: () {}),
            ),
          ),
        ),
      );
      await tester.pump(const Duration(seconds: 2));
      final forwards = kube as KubeForwardService;
      final forward = await forwards.startForward(
        session.id,
        namespace: _namespace,
        pod: 'fixture-shell',
        remotePort: 8080,
      );
      expect(forward.address, startsWith('127.0.0.1:'));
      final http = HttpClient();
      try {
        final response = await (await http.getUrl(Uri.parse('http://${forward.address}/')))
            .close()
            .timeout(const Duration(seconds: 20));
        expect(response.statusCode, 200);
        await response.drain<void>();
      } finally {
        http.close(force: true);
      }
      if (_externalStop) {
        // Host smoke runner backgrounds the app, verifies the Android
        // notification, and presses its real Stop action.
        await ready.writeAsString(jsonEncode({'stage': 'notification', 'address': forward.address}));
        final deadline = DateTime.now().add(const Duration(minutes: 2));
        while ((await forwards.forwards()).any((item) => item.id == forward.id && item.state == 'running')) {
          if (DateTime.now().isAfter(deadline)) fail('notification Stop was not exercised');
          await Future<void>.delayed(const Duration(milliseconds: 200));
        }
      } else {
        await forwards.stopForward(session.id, forward.id);
      }
      await expectLater(
        Socket.connect('127.0.0.1', int.parse(forward.address.split(':').last), timeout: const Duration(seconds: 2)),
        throwsA(isA<SocketException>()),
      );
      await services.vault.lock();
      await expectLater(browser.listResources(session.id, podKind, namespace: _namespace), throwsA(isA<Exception>()));
      await tester.pumpWidget(const SizedBox.shrink());
      await tester.pump();
    } finally {
      if (ready.existsSync()) await ready.delete();
      if (session != null) {
        try {
          await (services.kubernetes! as KubeBrowserService).disconnect(session.id);
        } catch (_) {}
      }
      for (final profile in services.profiles.currentProfiles.profiles) {
        await services.profiles.delete(profile.id);
      }
      await rust.close();
      await root.delete(recursive: true);
    }
  }, timeout: const Timeout(Duration(minutes: 6)));
}
