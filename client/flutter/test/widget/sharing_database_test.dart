import 'dart:convert';

import 'package:consolecrypt/app/app.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/services/database_sharing_service.dart';
import 'package:consolecrypt/core/services/sharing_service.dart';
import 'package:consolecrypt/db/databases_screen.dart';
import 'package:consolecrypt/db/db_models.dart';
import 'package:consolecrypt/db/db_providers.dart';
import 'package:consolecrypt/db/db_service.dart';
import 'package:consolecrypt/sharing/sharing_database.dart';
import 'package:consolecrypt/sharing/sharing_models.dart';
import 'package:consolecrypt/sharing/sharing_providers.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';
import '../helpers/fake_peer_pins.dart';

import '../helpers/test_app.dart';

const _recipient = SharingIdentity(
  instance: 'instance',
  userId: 'recipient',
  deviceId: 'recipient-device',
  encryptionKey: '01',
  signingKey: '02',
  code: 'ABCD-EFGH-IJKL-MNOP',
);

const _canary = 'FORBIDDEN DATABASE PASSWORD';

Map<String, Object?> _data(String routeKind, {bool credentials = false, bool production = true}) => {
  'name': 'Orders',
  'engine': 'postgresql',
  'host': 'orders.db.example.test',
  'port': 5432,
  'database': 'orders',
  'username': 'reporting',
  'tls_mode': 'verify',
  'production': production,
  'route_kind': routeKind,
  'route_namespace': routeKind == 'kubernetes' ? 'data' : null,
  'route_target_kind': routeKind == 'kubernetes' ? 'stateful_set' : null,
  'route_target_name': routeKind == 'kubernetes' ? 'orders-db' : null,
  'route_container': routeKind == 'docker' ? 'orders-db' : null,
  'requires_route': routeKind != 'direct',
  'includes_credentials': credentials,
  'tags': <String>[],
  // A malformed extra field: the preview must never render it.
  'password': _canary,
};

SharingItem _item(
  String id, {
  String routeKind = 'direct',
  bool credentials = false,
  bool owned = false,
  SharingTrust trust = SharingTrust.verified,
}) => SharingItem(
  id: id,
  itemId: 'item-$id',
  kind: SharingKind.dbConnection,
  ownerUserId: 'owner',
  ownerDeviceId: 'owner-device',
  revision: 1,
  epoch: 1,
  owned: owned,
  trust: trust,
  role: SharingRole.reader,
  previewJson: jsonEncode({'kind': 'db_connection', 'data': _data(routeKind, credentials: credentials)}),
);

class _Sharing with FakePeerPins implements SharingService, DatabaseSharingService {
  bool supportsDbConnections = true, supportsSecrets = true;
  String routeKind = 'relay';
  final previews = <bool>[];
  bool? publishedCredentials, copiedCredentials;
  String? stamp;
  List<SharingGrant>? grants;
  Map<String, dynamic>? copiedRoute;
  List<SharingItem> items = [];

  @override
  Future<SharingStatus> status() async => SharingStatus(
    enabled: true,
    instance: 'instance',
    supportsSecrets: supportsSecrets,
    supportsDbConnections: supportsDbConnections,
  );
  @override
  Future<List<SharingItem>> list({bool refresh = false}) async => items;
  @override
  Future<List<SharingIdentity>> discover(String email) async => [_recipient];
  @override
  Future<DbSharingPreview> previewDbConnection(String connectionId, {bool includeCredentials = false}) async {
    previews.add(includeCredentials);
    return DbSharingPreview(
      snapshotStamp: 'stamp-$includeCredentials',
      previewJson: jsonEncode({'kind': 'db_connection', 'data': _data(routeKind, credentials: includeCredentials)}),
    );
  }

  @override
  Future<SharingItem> publishDbConnection(
    String connectionId,
    List<SharingGrant> grants, {
    required String expectedStamp,
    bool includeCredentials = false,
  }) async {
    publishedCredentials = includeCredentials;
    stamp = expectedStamp;
    this.grants = grants;
    return _item('published');
  }

  @override
  Future<void> copyDbConnection(
    String shareId, {
    required Map<String, dynamic> route,
    bool includeCredentials = false,
  }) async {
    copiedCredentials = includeCredentials;
    copiedRoute = route;
  }

  @override
  Future<List<SharingOutboxEntry>> outbox() async => const [];
  @override
  Future<List<SharingOutboxEntry>> flush() async => const [];
  // All unrelated APIs must remain unused by these specialized flows.
  @override
  dynamic noSuchMethod(Invocation invocation) => super.noSuchMethod(invocation);
}

const _connection = DbConnection(
  id: 'orders',
  name: 'Orders (mine)',
  engine: DbEngine.postgresql,
  host: 'orders.db.example.test',
  port: 5432,
  options: DbDriverOptions(engine: DbEngine.postgresql),
);

class _Db implements DbService {
  @override
  Stream<void> changes() => const Stream.empty();
  @override
  Future<List<DbConnection>> connections() async => const [_connection];
  @override
  Future<DbConnectionChoices> connectionChoices() async => const DbConnectionChoices(
    routes: [
      DbRouteChoice(
        route: DbRoute(kind: DbRouteKind.ssh, reference: 'host-1'),
        label: 'bastion',
      ),
      DbRouteChoice(
        route: DbRoute(kind: DbRouteKind.jump, reference: 'jump-1'),
        label: 'via-dmz',
      ),
      DbRouteChoice(
        route: DbRoute(kind: DbRouteKind.kubernetes, reference: 'kube-1'),
        label: 'staging',
      ),
    ],
  );
  @override
  dynamic noSuchMethod(Invocation invocation) => super.noSuchMethod(invocation);
}

class _NoRoutes extends _Db {
  @override
  Future<DbConnectionChoices> connectionChoices() async => const DbConnectionChoices();
}

Future<void> _pump(WidgetTester tester, _Sharing sharing, {DbService? db}) async {
  final backend = testBackend();
  addTearDown(backend.dispose);
  await backend.debugSignInDemoAndUnlock();
  setTestLocale(backend, AppLocale.en);
  tester.view.physicalSize = const Size(1400, 1000);
  tester.view.devicePixelRatio = 1;
  addTearDown(tester.view.reset);
  await tester.pumpWidget(
    ProviderScope(
      overrides: [
        appServicesProvider.overrideWithValue(backend.services),
        sharingServiceProvider.overrideWithValue(sharing),
        dbServiceProvider.overrideWithValue(db ?? _Db()),
      ],
      retry: (_, _) => null,
      child: const ConsoleCryptApp(),
    ),
  );
  await settle(tester);
}

BuildContext _context(WidgetTester tester) => tester.element(find.byType(Navigator).first);

Future<void> _recipientSelection(WidgetTester tester) async {
  await enterKey(tester, 'sharing-email', 'recipient@example.test');
  await tapKey(tester, 'sharing-find');
  await tapKey(tester, 'sharing-give-access-recipient-device');
}

Future<void> _pickRoute(WidgetTester tester, String option) async {
  await tapKey(tester, 'sharing-db-route');
  await tester.tap(find.byKey(ValueKey('sharing-db-route-$option')).last);
  await settle(tester);
}

void main() {
  testWidgets('database sharing reviews metadata, warns about the unshared route and opts in to credentials', (
    tester,
  ) async {
    final sharing = _Sharing();
    await _pump(tester, sharing);
    showSharingDbConnection(_context(tester), connectionId: 'orders').ignore();
    await settle(tester);
    expect(sharing.previews, [false]);
    expect(find.text(_canary), findsNothing);
    expect(find.text('orders.db.example.test'), findsOneWidget);
    expect(find.byKey(const ValueKey('sharing-db-route-notice')), findsOneWidget);
    expect(find.byKey(const ValueKey('sharing-db-production')), findsOneWidget);
    expect(isEnabled(tester, 'sharing-db-publish-confirm'), false);
    await tapKey(tester, 'sharing-db-include-credentials');
    expect(sharing.previews, [false, true]);
    await _recipientSelection(tester);
    expect(isEnabled(tester, 'sharing-db-publish-confirm'), true);
    await tapKey(tester, 'sharing-db-publish-confirm');
    expect(sharing.publishedCredentials, true);
    expect(sharing.stamp, 'stamp-true');
    expect(sharing.grants!.single.verificationId, 'pin-${_recipient.deviceId}');
    expect(tester.takeException(), isNull);
  });

  testWidgets('credentials cannot be offered without the secret capability', (tester) async {
    final sharing = _Sharing()
      ..supportsSecrets = false
      ..routeKind = 'direct';
    await _pump(tester, sharing);
    showSharingDbConnection(_context(tester), connectionId: 'orders').ignore();
    await settle(tester);
    expect(find.byKey(const ValueKey('sharing-db-include-credentials')), findsNothing);
    expect(find.byKey(const ValueKey('sharing-db-route-notice')), findsNothing);
    await _recipientSelection(tester);
    await tapKey(tester, 'sharing-db-publish-confirm');
    expect(sharing.publishedCredentials, false);
    expect(tester.takeException(), isNull);
  });

  testWidgets('a server without database sharing cannot preview or publish', (tester) async {
    final sharing = _Sharing()..supportsDbConnections = false;
    await _pump(tester, sharing);
    showSharingDbConnection(_context(tester), connectionId: 'orders').ignore();
    await settle(tester);
    expect(sharing.previews, isEmpty);
    expect(find.byKey(const ValueKey('sharing-db-unsupported')), findsOneWidget);
    expect(isEnabled(tester, 'sharing-db-publish-confirm'), false);
    expect(find.byKey(const ValueKey('sharing-db-recipients')), findsNothing);
  });

  testWidgets('a relayed connection is copied only through the recipient own relay', (tester) async {
    final sharing = _Sharing();
    await _pump(tester, sharing);
    showSharingCopyDbConnection(_context(tester), _item('relay', routeKind: 'relay', credentials: true)).ignore();
    await settle(tester);
    expect(find.text(_canary), findsNothing);
    expect(isEnabled(tester, 'sharing-db-copy-confirm'), false, reason: 'no silent direct connection');
    await tapKey(tester, 'sharing-db-route');
    expect(find.byKey(const ValueKey('sharing-db-route-')), findsNothing, reason: 'direct is not offered');
    expect(find.byKey(const ValueKey('sharing-db-route-r2')), findsNothing, reason: 'no Kubernetes for a relay');
    await tester.tap(find.byKey(const ValueKey('sharing-db-route-r1')).last);
    await settle(tester);
    await tapKey(tester, 'sharing-db-copy-confirm');
    expect(sharing.copiedRoute, {'kind': 'jump', 'jump_profile_id': 'jump-1'});
    expect(sharing.copiedCredentials, false, reason: 'credentials are imported only on explicit choice');
    expect(tester.takeException(), isNull);
  });

  testWidgets('a forwarded Kubernetes connection keeps its workload and takes the recipient context', (tester) async {
    final sharing = _Sharing();
    await _pump(tester, sharing);
    showSharingCopyDbConnection(_context(tester), _item('kube', routeKind: 'kubernetes', credentials: true)).ignore();
    await settle(tester);
    await _pickRoute(tester, 'r2');
    await tapKey(tester, 'sharing-db-import-credentials');
    await tapKey(tester, 'sharing-db-copy-confirm');
    expect(sharing.copiedRoute, {
      'kind': 'kubernetes',
      'context_id': 'kube-1',
      'namespace': 'data',
      'target_kind': 'stateful_set',
      'target_name': 'orders-db',
    });
    expect(sharing.copiedCredentials, true);
  });

  testWidgets('a Docker connection may use this computer and keeps the shared container', (tester) async {
    final sharing = _Sharing();
    await _pump(tester, sharing);
    showSharingCopyDbConnection(_context(tester), _item('docker', routeKind: 'docker')).ignore();
    await settle(tester);
    expect(find.byKey(const ValueKey('sharing-db-import-credentials')), findsNothing);
    await _pickRoute(tester, 'd');
    await tapKey(tester, 'sharing-db-copy-confirm');
    expect(sharing.copiedRoute, {'kind': 'docker', 'host_id': null, 'container': 'orders-db'});
  });

  testWidgets('without a compatible local route the copy cannot be saved', (tester) async {
    final sharing = _Sharing();
    await _pump(tester, sharing, db: _NoRoutes());
    showSharingCopyDbConnection(_context(tester), _item('relay', routeKind: 'relay')).ignore();
    await settle(tester);
    expect(find.byKey(const ValueKey('sharing-db-no-route')), findsOneWidget);
    expect(isEnabled(tester, 'sharing-db-copy-confirm'), false);
    expect(sharing.copiedRoute, isNull);
  });

  testWidgets('a direct connection defaults to direct on the recipient side', (tester) async {
    final sharing = _Sharing();
    await _pump(tester, sharing);
    showSharingCopyDbConnection(_context(tester), _item('direct')).ignore();
    await settle(tester);
    await tapKey(tester, 'sharing-db-copy-confirm');
    expect(sharing.copiedRoute, {'kind': 'direct'});
  });

  Future<void> pumpDatabases(WidgetTester tester, _Sharing sharing) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    tester.view.physicalSize = const Size(1100, 900);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.reset);
    await tester.pumpWidget(
      ProviderScope(
        overrides: [
          appServicesProvider.overrideWithValue(backend.services),
          sharingServiceProvider.overrideWithValue(sharing),
          dbServiceProvider.overrideWithValue(_Db()),
          dbScopeProvider.overrideWithValue(const DbScope('profile', true)),
          sharingStatusProvider.overrideWith((ref) => sharing.status()),
          sharingItemsProvider.overrideWith((ref) => sharing.list()),
        ],
        child: const MaterialApp(
          locale: Locale('en'),
          localizationsDelegates: [AppLocalizations.delegate, ...GlobalMaterialLocalizations.delegates],
          supportedLocales: AppLocalizations.supportedLocales,
          home: Scaffold(body: DatabasesScreen()),
        ),
      ),
    );
    await settle(tester);
  }

  testWidgets('Databases lists shared connections with a badge and offers Share', (tester) async {
    final sharing = _Sharing()
      ..items = [
        _item('incoming'),
        _item('mine', owned: true),
        _item('invite', trust: SharingTrust.unverified),
        _item('gone', trust: SharingTrust.deleted),
      ];
    await pumpDatabases(tester, sharing);
    expect(find.text('Shared connections · 3 shared connections'), findsOneWidget);
    expect(find.widgetWithText(GlassBadge, 'Shared'), findsNWidgets(2));
    expect(find.widgetWithText(GlassBadge, 'Shared by you'), findsOneWidget);
    expect(find.byKey(const ValueKey('db-shared-gone')), findsNothing);
    expect(find.widgetWithText(GlassButton, 'Verify and accept'), findsOneWidget);
    await tapKey(tester, 'db-menu-orders');
    expect(find.text('Sharing'), findsOneWidget);
    expect(tester.takeException(), isNull);
  });

  testWidgets('an older server keeps Share and the shared section hidden', (tester) async {
    final sharing = _Sharing()
      ..supportsDbConnections = false
      ..items = [_item('incoming')];
    await pumpDatabases(tester, sharing);
    expect(find.byKey(const ValueKey('db-shared-header')), findsNothing);
    expect(find.byKey(const ValueKey('db-shared-incoming')), findsNothing);
    await tapKey(tester, 'db-menu-orders');
    expect(find.text('Sharing'), findsNothing);
    expect(tester.takeException(), isNull);
  });
}
