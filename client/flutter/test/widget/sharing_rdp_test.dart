import 'dart:convert';

import 'package:consolecrypt/app/app.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/services/rdp_sharing_service.dart';
import 'package:consolecrypt/core/services/sharing_service.dart';
import 'package:consolecrypt/sharing/sharing_models.dart';
import 'package:consolecrypt/sharing/sharing_providers.dart';
import 'package:consolecrypt/sharing/sharing_rdp.dart';
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

Map<String, Object?> _data({bool password = false}) => {
  'name': 'Office PC',
  'address': 'office.example.test',
  'port': 3389,
  'username': 'Администратор',
  'domain': 'LAB',
  'tags': <String>[],
  'notes': null,
  'includes_password': password,
};

SharingItem _item({bool password = false}) => SharingItem(
  id: 'card',
  itemId: 'item-card',
  kind: SharingKind.host,
  ownerUserId: 'owner',
  ownerDeviceId: 'owner-device',
  revision: 1,
  epoch: 1,
  owned: false,
  trust: SharingTrust.verified,
  role: SharingRole.reader,
  previewJson: jsonEncode({'kind': 'rdp_host', 'data': _data(password: password)}),
);

class _Sharing with FakePeerPins implements SharingService, RdpSharingService {
  bool supportsSecrets = true;
  final previews = <(bool, bool)>[];
  bool? publishedPassword, copiedPassword;
  String? stamp;
  List<SharingGrant>? grants;

  @override
  Future<SharingStatus> status() async =>
      SharingStatus(enabled: true, instance: 'instance', supportsSecrets: supportsSecrets);
  @override
  Future<List<SharingItem>> list({bool refresh = false}) async => const [];
  @override
  Future<List<SharingIdentity>> discover(String email) async => [_recipient];
  @override
  Future<RdpSharingPreview> previewRdpHost(String hostId, {bool includePassword = false, bool includeNotes = false}) async {
    previews.add((includePassword, includeNotes));
    return RdpSharingPreview(
      snapshotStamp: 'stamp-$includePassword-$includeNotes',
      previewJson: jsonEncode({'kind': 'rdp_host', 'data': _data(password: includePassword)}),
    );
  }

  @override
  Future<SharingItem> publishRdpHost(
    String hostId,
    List<SharingGrant> grants, {
    required String expectedStamp,
    bool includePassword = false,
    bool includeNotes = false,
  }) async {
    publishedPassword = includePassword;
    stamp = expectedStamp;
    this.grants = grants;
    return _item(password: includePassword);
  }

  @override
  Future<void> copyRdpHost(String shareId, {bool includePassword = false}) async => copiedPassword = includePassword;
  @override
  Future<List<SharingOutboxEntry>> outbox() async => const [];
  @override
  Future<List<SharingOutboxEntry>> flush() async => const [];
  @override
  dynamic noSuchMethod(Invocation invocation) => super.noSuchMethod(invocation);
}

Future<void> _pump(WidgetTester tester, _Sharing sharing) async {
  final backend = testBackend();
  addTearDown(backend.dispose);
  await backend.debugSignInDemoAndUnlock();
  tester.view.physicalSize = const Size(1400, 1000);
  tester.view.devicePixelRatio = 1;
  addTearDown(tester.view.reset);
  await tester.pumpWidget(
    ProviderScope(
      overrides: [
        appServicesProvider.overrideWithValue(backend.services),
        sharingServiceProvider.overrideWithValue(sharing),
      ],
      retry: (_, _) => null,
      child: const ConsoleCryptApp(),
    ),
  );
  await settle(tester);
}

BuildContext _context(WidgetTester tester) => tester.element(find.byType(Navigator).first);

void main() {
  testWidgets('an RDP card is reviewed without the password; notes and password only when ticked', (tester) async {
    final sharing = _Sharing();
    await _pump(tester, sharing);
    showSharingRdpHost(_context(tester), hostId: 'office').ignore();
    await settle(tester);
    expect(sharing.previews, [(false, false)]);
    expect(find.textContaining('office.example.test'), findsWidgets);
    expect(isEnabled(tester, 'sharing-rdp-publish-confirm'), false);
    await tapKey(tester, 'sharing-rdp-include-password');
    await tapKey(tester, 'sharing-rdp-include-notes');
    expect(sharing.previews.last, (true, true));
    expect(find.textContaining('get the password itself'), findsOneWidget);
    await enterKey(tester, 'sharing-email', 'recipient@example.test');
    await tapKey(tester, 'sharing-find');
    await tapKey(tester, 'sharing-give-access-recipient-device');
    await tapKey(tester, 'sharing-rdp-publish-confirm');
    expect(sharing.publishedPassword, true);
    expect(sharing.stamp, 'stamp-true-true');
    expect(sharing.grants!.single.verificationId, 'pin-${_recipient.deviceId}');
    expect(tester.takeException(), isNull);
  });

  testWidgets('without the secret capability the password is not offered', (tester) async {
    final sharing = _Sharing()..supportsSecrets = false;
    await _pump(tester, sharing);
    showSharingRdpHost(_context(tester), hostId: 'office').ignore();
    await settle(tester);
    expect(find.byKey(const ValueKey('sharing-rdp-include-password')), findsNothing);
    expect(tester.takeException(), isNull);
  });

  testWidgets('a received card is saved with the password only when ticked', (tester) async {
    final sharing = _Sharing();
    await _pump(tester, sharing);
    final item = _item(password: true);
    expect(item.isRdp, isTrue);
    showSharingCopyRdpHost(_context(tester), item).ignore();
    await settle(tester);
    await tapKey(tester, 'sharing-rdp-copy-password');
    await tapKey(tester, 'sharing-rdp-copy-confirm');
    expect(sharing.copiedPassword, true);

    showSharingCopyRdpHost(_context(tester), _item()).ignore();
    await settle(tester);
    expect(find.byKey(const ValueKey('sharing-rdp-copy-password')), findsNothing);
    expect(find.textContaining('asked when connecting'), findsOneWidget);
    await tapKey(tester, 'sharing-rdp-copy-confirm');
    expect(sharing.copiedPassword, false);
    expect(tester.takeException(), isNull);
  });
}
