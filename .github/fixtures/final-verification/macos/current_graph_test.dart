// Explicit opt-in product boundary proof. Never uses normal app bootstrap,
// personal profiles, OS keychain, production servers or automatic sync.
import 'dart:convert';
import 'dart:io';

import 'package:consolecrypt/app/theme.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/monitoring/monitoring_accepted_query.dart';
import 'package:consolecrypt/monitoring/monitoring_accepted_query_view.dart';
import 'package:consolecrypt/monitoring/monitoring_chart_model.dart';
import 'package:consolecrypt/monitoring/monitoring_models.dart';
import 'package:consolecrypt/monitoring/monitoring_service.dart';
import 'package:consolecrypt/monitoring/monitoring_time_window.dart';
import 'package:consolecrypt/src/rust/api/app.dart' as native;
import 'package:consolecrypt/src/rust/api/error.dart';
import 'package:consolecrypt/src/rust/api/monitoring.dart' as monitoring;
import 'package:consolecrypt/src/rust/api/profiles.dart' as profiles;
import 'package:consolecrypt/src/rust/frb_generated.dart';
import 'package:crypto/crypto.dart';
import 'package:flutter_rust_bridge/flutter_rust_bridge_for_generated.dart' show ExternalLibrary;
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

Json decodeObject(String value) => Map<String, dynamic>.from(jsonDecode(value) as Map);

// Bound only after the actual final application DLL has been built.
const String? approvedDllSha256 = null;

void main() {
  testWidgets('current verified DLL -> saved Native source -> real HTTP -> production chart/stale/noData/lock', (
    tester,
  ) async {
    final dllPath = Platform.environment['CC_OWNED_MONITORING_DLL'];
    final expectedSha = Platform.environment['CC_OWNED_MONITORING_DLL_SHA256'];
    expect(dllPath, isNotNull, reason: 'Explicit current-source DLL input is required.');
    expect(approvedDllSha256, isNotNull, reason: 'Actual final DLL binding is required.');
    expect(expectedSha, approvedDllSha256);
    final dll = File(dllPath!).absolute;
    expect(Platform.isMacOS, true);
    final proofPath = Platform.environment['CC_OWNED_MONITORING_ROOT'];
    expect(proofPath, isNotNull, reason: 'The original owner supplies one fresh private fixture root.');
    final proofRoot = Directory(proofPath!).absolute;
    expect(proofRoot.existsSync(), true);
    expect(proofRoot.resolveSymbolicLinksSync(), proofRoot.path);
    late Directory own;
    var ownCreated = false;
    HttpServer? server;
    var initialized = false;
    var rustInitialized = false;
    var requests = 0;
    var mode = 'matrix';
    final received = <Map<String, String>>[];
    final fixtureErrors = <String>[];
    late MonitoringSource source;
    late AppCoreMonitoringService service;
    late QueryOutcome first;
    final scope = Object();
    const query = MonitoringQuery(expression: 'owned_fixture_metric', start: 1000, end: 1090, stepSeconds: 15);
    final window = MonitoringTimeWindow.fromEpoch(1000, 1090, 15);
    late MonitoringAcceptedQueryState state;
    final report = <String, Object?>{'fixture': 'synthetic Prometheus HTTP wire, not PromQL evaluator'};
    addTearDown(() async {
      if (initialized) await native.coreShutdown();
      if (rustInitialized) RustLib.dispose();
      await server?.close(force: true);
      // Only this newly created fixture directory; no caller-controlled cleanup path.
      if (ownCreated && own.existsSync()) {
        expect(own.parent.absolute.path, proofRoot.absolute.path);
        expect(own.uri.pathSegments.lastWhere((e) => e.isNotEmpty), startsWith('owned-'));
        expect(Directory(own.resolveSymbolicLinksSync()).parent.path, proofRoot.resolveSymbolicLinksSync());
        own.deleteSync(recursive: true);
      }
    });
    await tester.runAsync(() async {
      final actual = await sha256.bind(dll.openRead()).first;
      expect(actual.toString(), expectedSha);
      report['nativeSha256'] = actual.toString();
      report['sourceAbi'] = 1141874734;
      own = await proofRoot.createTemp('owned-');
      ownCreated = true;
      await RustLib.init(externalLibrary: ExternalLibrary.open(dll.path));
      rustInitialized = true;
      // Explicit generated ABI assertion; RustLib.init also verifies the loaded library handshake.
      // ignore: invalid_use_of_internal_member
      expect(RustLib.instance.rustContentHash, 1141874734);
      expect(native.coreIsInitialized(), false);
      final info = await native.coreInit(
        config: native.CoreConfig(
          dataDir: own.path,
          secureStore: native.SecureStoreChoice.memory,
          kdf: native.KdfChoice.floor,
          clientVersion: '1.2.0+67027',
          deviceName: 'Owned current graph fixture',
          backgroundSync: false,
          autoStartTunnels: false,
          logDirectives: 'off',
        ),
      );
      initialized = true;
      expect(info.alreadyInitialized, false);
      expect(Directory(info.dataDir).absolute.path, own.absolute.path);
      expect(jsonDecode(await profiles.profilesList()), isEmpty);
      // Synthetic local passphrase remains in this disposable process/root only.
      await profiles.profilesCreateLocal(
        displayName: 'Owned monitoring boundary',
        passphrase: utf8.encode('synthetic-fixture-passphrase-25931'),
      );
      server = await HttpServer.bind(InternetAddress.loopbackIPv4, 0);
      server!.listen((request) async {
        requests++;
        if (request.method != 'POST') fixtureErrors.add('unexpected method');
        received.add(Uri.splitQueryString(await utf8.decoder.bind(request).join()));
        if (request.uri.path != '/fixture/api/v1/query_range') {
          fixtureErrors.add('unexpected path');
        }
        if (request.headers.value(HttpHeaders.authorizationHeader) != null) {
          fixtureErrors.add('unexpected auth');
        }
        request.response.headers.contentType = ContentType.json;
        if (mode == 'error') {
          request.response.statusCode = 503;
          request.response.write(
            jsonEncode({'status': 'error', 'errorType': 'unavailable', 'error': 'owned unavailable'}),
          );
        } else {
          final result = mode == 'empty'
              ? <Object>[]
              : mode == 'histogram'
              ? [
                  {
                    'metric': {'job': 'owned-fixture'},
                    'histograms': [
                      [
                        1000,
                        {'count': '1', 'sum': '1'},
                      ],
                    ],
                  },
                ]
              : [
                  {
                    'metric': {'job': 'owned-fixture', 'instance': 'synthetic'},
                    'values': [
                      [1000, '0'],
                      [1015, '2'],
                      [1030, 'NaN'],
                      [1045, '4'],
                      [1090, '5'],
                    ],
                  },
                ];
          request.response.write(
            jsonEncode({
              'status': 'success',
              'data': {'resultType': 'matrix', 'result': result},
              if (mode == 'matrix') 'warnings': ['synthetic partial fixture'],
            }),
          );
        }
        await request.response.close();
      });
      final configuration = MonitoringSource(
        id: '',
        name: 'Owned loopback',
        endpoint: 'http://127.0.0.1:${server!.port}/fixture',
      );
      final catalog = decodeObject(
        await monitoring.monitoringSourceCreate(
          expectedCatalogRevision: 0,
          configurationJson: jsonEncode(configuration.toJson()),
        ),
      );
      source = MonitoringSource.fromRecord(object((catalog['sources'] as List).single));
      expect(source.id, matches(RegExp(r'^[a-f0-9-]{36}$')));
      expect(source.revision, greaterThan(0));
      service = AppCoreMonitoringService(
        read: (request) async => decodeObject(await monitoring.monitoringRead(requestJson: jsonEncode(request))),
        cancelOperation: (operationId) => monitoring.monitoringCancel(operationId: operationId),
      );
      first = await service.query(source, query);
      expect(first.status, 'success');
      expect(first.partial, true);
      final samples = first.data!.series.single.samples;
      expect(samples.first.value, 0);
      expect(samples[2].kind, SampleKind.nan);
      expect(samples[2].value, isNull);
      final projection = MonitoringChartProjection(
        series: monitoringChartSeries(first.data!.series),
        requestedWindow: window,
        effectiveStep: first.effectiveStepSeconds,
      );
      expect(projection.segments(projection.series.single).map((s) => s.length), [2, 1, 1]);
      expect(received.single, containsPair('query', query.expression));
      expect(double.parse(received.single['start']!), 1000);
      expect(double.parse(received.single['end']!), 1090);
      expect(double.parse(received.single['step']!), 15);
      final identity = MonitoringQueryIdentity(scope: scope, source: source, query: query);
      state = MonitoringAcceptedQueryState.empty.begin(identity, 1).complete(1, first, DateTime.utc(2026, 11, 12));
      report['realReadAndGapProjection'] = true;
    });
    Future<void> showState() async {
      tester.view.physicalSize = const Size(1100, 900);
      tester.view.devicePixelRatio = 1;
      await tester.pumpWidget(
        MaterialApp(
          theme: AppTheme.dark(platform: TargetPlatform.windows),
          locale: const Locale('ru'),
          supportedLocales: AppLocalizations.supportedLocales,
          localizationsDelegates: const [AppLocalizations.delegate, ...GlobalMaterialLocalizations.delegates],
          home: Scaffold(
            body: SingleChildScrollView(
              child: MonitoringAcceptedQueryView(state: state, requestedWindow: window),
            ),
          ),
        ),
      );
      await tester.pumpAndSettle();
      expect(tester.takeException(), isNull);
    }

    addTearDown(tester.view.reset);
    await showState();
    expect(find.byKey(const ValueKey('monitoring-chart')), findsOneWidget);
    await tester.runAsync(() async {
      mode = 'error';
      final identity = MonitoringQueryIdentity(scope: scope, source: source, query: query);
      state = state.begin(identity, 2);
      try {
        state = state.complete(2, await service.query(source, query), DateTime.utc(2026, 11, 12, 0, 1));
      } catch (error) {
        state = state.fail(2, error);
      }
      expect(state.phase, MonitoringAcceptedQueryPhase.stale);
      expect(identical(state.accepted!.outcome, first), true);
    });
    await showState();
    expect(find.byKey(const ValueKey('monitoring-previous-stale')), findsOneWidget);
    expect(find.byKey(const ValueKey('monitoring-chart')), findsOneWidget);
    report['actualTransientErrorRenderedStale'] = true;
    await tester.runAsync(() async {
      mode = 'empty';
      final identity = MonitoringQueryIdentity(scope: scope, source: source, query: query);
      state = state
          .begin(identity, 3)
          .complete(3, await service.query(source, query), DateTime.utc(2026, 11, 12, 0, 2));
      expect(state.phase, MonitoringAcceptedQueryPhase.fresh);
      expect(state.accepted!.outcome.data!.empty, true);
      report['successfulEmptyReplacesOld'] = true;
    });
    await showState();
    expect(find.byKey(const ValueKey('monitoring-previous-stale')), findsNothing);
    expect(find.byKey(const ValueKey('monitoring-chart')), findsNothing);
    await tester.runAsync(() async {
      mode = 'histogram';
      final identity = MonitoringQueryIdentity(scope: scope, source: source, query: query);
      state = state.begin(identity, 4);
      try {
        state = state.complete(4, await service.query(source, query), DateTime.utc(2026, 11, 12, 0, 3));
      } catch (error) {
        state = state.fail(4, error);
      }
      expect(
        state.error,
        isA<MonitoringFailure>().having((e) => e.kind, 'typed result refusal', MonitoringErrorCode.unsupportedResult),
      );
      expect(state.phase, MonitoringAcceptedQueryPhase.failed);
      expect(state.accepted, isNull, reason: 'Unsupported histograms cannot be treated as transient measurements.');
      report['nativeHistogramRefusedWithoutStaleCoercion'] = true;
      // A saved source revision change must also reject before transport.
      final beforeChange = requests;
      final replacement = MonitoringSource(id: source.id, name: 'Owned revised source', endpoint: source.endpoint);
      await monitoring.monitoringSourceUpdate(
        id: source.id,
        expectedCatalogRevision: 1,
        expectedSourceRevision: source.revision,
        configurationJson: jsonEncode(replacement.toJson()),
      );
      await expectLater(service.query(source, query), throwsA(isA<BridgeError>()));
      expect(requests, beforeChange);
      report['staleSourceRevisionDeniedBeforeHttp'] = true;
      await profiles.vaultLock();
      final before = requests;
      await expectLater(service.query(source, query), throwsA(isA<BridgeError>()));
      expect(requests, before, reason: 'Locked Native must reject before any HTTP request.');
      expect(fixtureErrors, isEmpty);
      report['lockedNativeDeniedBeforeHttp'] = true;
      expect(requests, 4);
      report['requests'] = requests;
    });
    await showState();
    expect(find.byKey(const ValueKey('monitoring-previous-stale')), findsNothing);
    expect(find.byKey(const ValueKey('monitoring-chart')), findsNothing);
    // Structured booleans/hashes only: no recovery kit, passphrase or profile DTO.
    // ignore: avoid_print
    print('OWNED_CURRENT_MONITORING_GRAPH_PROOF ${jsonEncode(report)}');
  }, timeout: const Timeout(Duration(minutes: 3)));
}
