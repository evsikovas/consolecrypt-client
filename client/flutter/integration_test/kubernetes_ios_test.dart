// Native iOS lifecycle regression against an isolated HTTPS/WebSocket fixture.
// Certificates are generated for this CI run; no production kubeconfig is used.
import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:consolecrypt/core/bridge/rust_app_services.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/kubernetes/kube_browser.dart';
import 'package:consolecrypt/kubernetes/kube_browser_service.dart';
import 'package:consolecrypt/kubernetes/kube_forward_service.dart';
import 'package:consolecrypt/kubernetes/kube_inventory_service.dart';
import 'package:consolecrypt/kubernetes/kube_mutation_service.dart';
import 'package:consolecrypt/kubernetes/kube_navigation.dart';
import 'package:consolecrypt/kubernetes/kube_service.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:integration_test/integration_test.dart';
import 'package:material_ui/material_ui.dart';

const _certificate = String.fromEnvironment('CC_KUBE_TLS_CERT');
const _key = String.fromEnvironment('CC_KUBE_TLS_KEY');

void main() {
  IntegrationTestWidgetsFlutterBinding.ensureInitialized();
  testWidgets('iOS native Kubernetes TLS, PROD, masking and background revocation', (tester) async {
    if (!Platform.isIOS) return;
    expect(_certificate, isNotEmpty, reason: 'run with the generated diagnostic TLS fixture');
    final fixture = await _Fixture.start();
    const platform = MethodChannel('consolecrypt/ios');
    final privateRoot = (await platform.invokeMethod<String>('dataDirectory'))!;
    final root = await Directory(privateRoot).createTemp('kube-ios-it-');
    final rust = await RustAppServices.open(
      RustCoreOptions(
        dataDir: '${root.path}/data',
        keychainService: 'io.consolecrypt.kube.ios-it-${DateTime.now().microsecondsSinceEpoch}',
        fastKdfForTests: true,
        backgroundSync: false,
        autoStartTunnels: false,
        hookAppExit: false,
      ),
    );
    final services = rust.services;
    try {
      await services.profiles.createLocalProfile(name: 'iOS Kubernetes regression');
      await services.vault.createVault(name: 'Fixture', passphrase: SecretText('it-${ObjectId.generate().value}'));
      await services.vault.confirmRecoveryKitSaved();
      final kube = services.kubernetes!;
      final yaml = SecretText(
        jsonEncode({
          'apiVersion': 'v1',
          'kind': 'Config',
          'clusters': [
            {
              'name': 'fixture',
              'cluster': {
                'server': 'https://127.0.0.1:${fixture.server.port}',
                'certificate-authority-data': base64Encode(utf8.encode(_certificate)),
              },
            },
          ],
          'users': [
            {
              'name': 'fixture',
              'user': {'token': 'synthetic-ios-token'},
            },
          ],
          'contexts': [
            {
              'name': 'fixture',
              'context': {'cluster': 'fixture', 'user': 'fixture', 'namespace': 'default'},
            },
          ],
          'current-context': 'fixture',
        }),
      );
      late KubeImportPreview preview;
      try {
        preview = await kube.preview(yaml);
      } finally {
        yaml.wipe();
      }
      expect(preview.entries.single.error, isNull);
      await kube.commit(preview.ticket, [KubeImportChoice(index: preview.entries.single.index)]);
      var connection = (await kube.inventory()).single;
      final browser = kube as KubeBrowserService;
      final production = await browser.connect(connection.id);
      final kinds = await browser.discover(production.id);
      final podKind = kinds.singleWhere((kind) => kind.resource == 'pods');
      final pod = KubeResourceRef(api: podKind, namespace: 'default', name: 'fixture-pod');
      final mutations = kube as KubeMutationService;
      expect((await mutations.writeAccess(production.id)).canRequestWindow, isFalse);
      await expectLater(mutations.requestWriteWindow(production.id, connection.name), throwsA(isA<Exception>()));
      await expectLater(
        mutations.deleteResource(production.id, pod, uid: 'pod-uid', resourceVersion: '1'),
        throwsA(isA<Exception>()),
      );
      expect(fixture.mutations, 0);
      final secretKind = kinds.singleWhere((kind) => kind.resource == 'secrets');
      final secret = await browser.getResource(
        production.id,
        KubeResourceRef(api: secretKind, namespace: 'default', name: 'fixture-secret'),
      );
      expect(secret.yaml, isNot(contains('synthetic-secret-value')));
      expect(secret.yaml, isNot(contains(base64Encode(utf8.encode('synthetic-secret-value')))));
      await browser.disconnect(production.id);
      await (kube as KubeInventoryManagementService).updateContext(
        connection.id,
        name: connection.name,
        namespace: 'default',
        production: false,
        allowExecInProd: false,
        allowPortForwardInProd: false,
        recordEvents: false,
      );
      connection = (await kube.inventory()).single;
      await tester.pumpWidget(
        ProviderScope(
          overrides: [appServicesProvider.overrideWithValue(services)],
          child: MaterialApp(
            locale: const Locale('en'),
            supportedLocales: AppLocalizations.supportedLocales,
            localizationsDelegates: const [AppLocalizations.delegate, ...GlobalMaterialLocalizations.delegates],
            home: Scaffold(
              body: KubeBrowser(connection: connection, service: browser, onClose: () {}),
            ),
          ),
        ),
      );
      final container = ProviderScope.containerOf(tester.element(find.byType(KubeBrowser)));
      await _until(tester, () => container.read(kubeActiveNavigationProvider)?.resources.isNotEmpty == true);
      final sessionId = container.read(kubeActiveNavigationProvider)!.owner;
      final forwards = kube as KubeForwardService;
      final forward = await forwards.startForward(sessionId, namespace: 'default', pod: 'fixture-pod', remotePort: 80);
      final port = int.parse(forward.address.split(':').last);
      final socket = await Socket.connect('127.0.0.1', port);
      try {
        socket.add(utf8.encode('ios-forward'));
        expect(utf8.decode(await socket.first.timeout(const Duration(seconds: 15))), 'ios-forward');
      } finally {
        socket.destroy();
      }
      tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.inactive);
      tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.hidden);
      tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.paused);
      await _untilAsync(tester, () async => (await forwards.forwards()).isEmpty);
      await expectLater(
        Socket.connect('127.0.0.1', port, timeout: const Duration(seconds: 2)),
        throwsA(isA<SocketException>()),
      );
      await expectLater(browser.listResources(sessionId, podKind, namespace: 'default'), throwsA(isA<Exception>()));
      tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.hidden);
      tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.inactive);
      tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.resumed);
      await _until(tester, () {
        final active = container.read(kubeActiveNavigationProvider);
        return active != null && active.owner != sessionId && active.resources.isNotEmpty;
      });
      final resumed = container.read(kubeActiveNavigationProvider)!.owner;
      await services.vault.lock();
      await expectLater(browser.listResources(resumed, podKind, namespace: 'default'), throwsA(isA<Exception>()));
      await tester.pumpWidget(const SizedBox.shrink());
      await tester.pump();
      expect(tester.takeException(), isNull);
    } finally {
      await tester.pumpWidget(const SizedBox.shrink());
      for (final profile in services.profiles.currentProfiles.profiles) {
        await services.profiles.delete(profile.id);
      }
      await rust.close();
      await fixture.close();
      await root.delete(recursive: true);
    }
  });
}

Future<void> _until(WidgetTester tester, bool Function() condition) => _untilAsync(tester, () async => condition());
Future<void> _untilAsync(WidgetTester tester, Future<bool> Function() condition) async {
  final deadline = DateTime.now().add(const Duration(seconds: 30));
  while (!await condition()) {
    if (DateTime.now().isAfter(deadline)) fail('native Kubernetes UI did not reach the expected state');
    await tester.pump(const Duration(milliseconds: 100));
  }
  await tester.pump();
}

class _Fixture {
  _Fixture(this.server);
  final HttpServer server;
  final _sockets = <WebSocket>{};
  int mutations = 0;
  static final pod = <String, dynamic>{
    'apiVersion': 'v1',
    'kind': 'Pod',
    'metadata': {'name': 'fixture-pod', 'namespace': 'default', 'uid': 'pod-uid', 'resourceVersion': '1'},
    'spec': {
      'containers': [
        {'name': 'web', 'image': 'fixture:1'},
      ],
    },
    'status': {'phase': 'Running'},
  };
  static Future<_Fixture> start() async {
    final tls = SecurityContext()
      ..useCertificateChainBytes(utf8.encode(_certificate))
      ..usePrivateKeyBytes(utf8.encode(_key));
    final fixture = _Fixture(await HttpServer.bindSecure(InternetAddress.loopbackIPv4, 0, tls));
    fixture.server.listen((request) => unawaited(fixture._handle(request)));
    return fixture;
  }

  Future<void> _handle(HttpRequest request) async {
    try {
      if (request.headers.value(HttpHeaders.authorizationHeader) != 'Bearer synthetic-ios-token') {
        request.response.statusCode = 401;
        await request.response.close();
        return;
      }
      if (WebSocketTransformer.isUpgradeRequest(request)) {
        final socket = await WebSocketTransformer.upgrade(
          request,
          protocolSelector: (protocols) =>
              protocols.contains('v4.channel.k8s.io') ? 'v4.channel.k8s.io' : protocols.firstOrNull,
        );
        _sockets.add(socket);
        socket.add([0, 80, 0]);
        socket.add([1, 80, 0]);
        await for (final message in socket) {
          if (message is List<int> && message.isNotEmpty && message.first == 0) socket.add(message);
        }
        _sockets.remove(socket);
        return;
      }
      if (request.method != 'GET') mutations++;
      final path = request.uri.path;
      final Object result;
      if (path == '/version') {
        result = {'gitVersion': 'v1.32.0'};
      } else if (path == '/api') {
        result = {
          'kind': 'APIVersions',
          'apiVersion': 'v1',
          'versions': ['v1'],
          'serverAddressByClientCIDRs': <Object>[],
        };
      } else if (path == '/apis') {
        result = {'kind': 'APIGroupList', 'apiVersion': 'v1', 'groups': <Object>[]};
      } else if (path == '/api/v1') {
        result = {
          'kind': 'APIResourceList',
          'apiVersion': 'v1',
          'groupVersion': 'v1',
          'resources': [
            for (final type in [('pods', 'Pod', true), ('secrets', 'Secret', true), ('namespaces', 'Namespace', false)])
              {
                'name': type.$1,
                'kind': type.$2,
                'namespaced': type.$3,
                'singularName': '',
                'verbs': ['get', 'list'],
              },
          ],
        };
      } else if (path == '/api/v1/namespaces') {
        result = {
          'apiVersion': 'v1',
          'kind': 'NamespaceList',
          'metadata': {'resourceVersion': '1'},
          'items': [
            {
              'apiVersion': 'v1',
              'kind': 'Namespace',
              'metadata': {'name': 'default', 'uid': 'namespace-uid', 'resourceVersion': '1'},
            },
          ],
        };
      } else if (path.endsWith('/secrets/fixture-secret')) {
        result = {
          'apiVersion': 'v1',
          'kind': 'Secret',
          'metadata': {'name': 'fixture-secret', 'namespace': 'default', 'uid': 'secret-uid', 'resourceVersion': '1'},
          'data': {'password': base64Encode(utf8.encode('synthetic-secret-value'))},
        };
      } else if (path.endsWith('/pods/fixture-pod')) {
        result = pod;
      } else if (path.endsWith('/pods')) {
        result = {
          'apiVersion': 'v1',
          'kind': 'PodList',
          'metadata': {'resourceVersion': '1'},
          'items': [pod],
        };
      } else {
        request.response.statusCode = 404;
        result = {'apiVersion': 'v1', 'kind': 'Status', 'status': 'Failure', 'reason': 'NotFound', 'code': 404};
      }
      request.response.headers.contentType = ContentType.json;
      request.response.write(jsonEncode(result));
      await request.response.close();
    } on Object {
      // Native cancellation closes fixture sockets during lock/background.
      try {
        await request.response.close();
      } on Object {
        /* already closed */
      }
    }
  }

  Future<void> close() async {
    for (final socket in _sockets.toList()) {
      await socket.close();
    }
    await server.close(force: true);
  }
}
