import 'dart:async';
import 'dart:convert';

import 'package:consolecrypt/app/app.dart';
import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/mock/mock_backend.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/services/app_services.dart';
import 'package:consolecrypt/sharing/sharing_dialogs.dart';
import 'package:consolecrypt/sharing/sharing_models.dart';
import 'package:consolecrypt/sharing/sharing_providers.dart';
import 'package:consolecrypt/sharing/sharing_screen.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/test_app.dart';

// Synthetic public identities only. Actual cryptographic proofs are tested by
// the HTTP/SQLCipher AppCore tests; these exercise the production Flutter UI.
const _owner = SharingIdentity(
  instance: 'instance',
  userId: 'owner',
  deviceId: 'owner-device',
  encryptionKey: 'public-x-owner',
  signingKey: 'public-ed-owner',
  code: 'OWNER VERIFIED CODE',
);
const _reader = SharingIdentity(
  instance: 'instance',
  userId: 'reader',
  deviceId: 'reader-device',
  encryptionKey: 'public-x-reader',
  signingKey: 'public-ed-reader',
  code: 'READER VERIFIED CODE',
);
const _editor = SharingIdentity(
  instance: 'instance',
  userId: 'editor',
  deviceId: 'editor-device',
  encryptionKey: 'public-x-editor',
  signingKey: 'public-ed-editor',
  code: 'EDITOR VERIFIED CODE',
);
const _otherOwnerDevice = SharingIdentity(
  instance: 'instance',
  userId: 'owner',
  deviceId: 'other-owner-device',
  encryptionKey: 'public-x-other-owner',
  signingKey: 'public-ed-other-owner',
  code: 'OTHER OWNER DEVICE CODE',
);

String _hostPreview({bool notes = false}) => jsonEncode({
  'kind': 'host',
  'data': {
    'name': 'Shared endpoint',
    'address': 'example.test',
    'port': 22,
    'username': 'operator',
    if (notes) 'notes': 'Explicit notes',
    'tags': <String>[],
  },
});
String _snippetPreview() => jsonEncode({
  'kind': 'snippet',
  'data': {
    'name': 'Received command',
    'description': '',
    'template': 'echo explicit-only',
    'snippet_type': 'shell',
    'shell': null,
    'variables': <Object>[],
    'tags': <String>[],
  },
});

class _Sharing implements SharingService {
  int publications = 0, accepts = 0, flushes = 0, discoveries = 0, edits = 0;
  bool failFlush = false;
  SharingIdentity currentIdentity = _owner;
  List<SharingGrant>? publishedGrants, rotatedGrants;
  List<SharingItem> items = [];
  Completer<String>? delayedPreview;
  Completer<List<SharingItem>>? delayedList;
  @override
  Future<List<SharingOutboxEntry>> edit(String id, String projectionJson) async {
    edits++;
    return [SharingOutboxEntry('edit', id, false, null)];
  }

  @override
  Future<void> delete(String id) async {}
  @override
  Future<List<SharingOutboxEntry>> outbox() async => [];
  @override
  Future<SharingStatus> status() async => SharingStatus(enabled: true, instance: 'instance', identity: currentIdentity);
  @override
  Future<List<SharingIdentity>> discover(String email) async {
    discoveries++;
    return [_reader, _editor];
  }

  @override
  Future<String> preview(SharingKind kind, String objectId, {bool includeNotes = false}) async =>
      delayedPreview == null ? _hostPreview(notes: includeNotes) : delayedPreview!.future;
  @override
  Future<SharingItem> publish(String projectionJson, List<SharingGrant> grants) async {
    publications++;
    publishedGrants = grants;
    return _item(owned: true, preview: projectionJson);
  }

  @override
  Future<List<SharingOutboxEntry>> flush() async {
    flushes++;
    if (failFlush) {
      throw const AppException(AppErrorCode.serverUnreachable, 'Network unavailable');
    }
    return [];
  }

  @override
  Future<List<SharingItem>> list({bool refresh = false}) async => delayedList == null ? items : delayedList!.future;
  @override
  Future<SharingInvitation> inspect(String id) async => SharingInvitation(_item(), _owner);
  @override
  Future<SharingItem> accept(String id, String confirmedOwnerCode) async {
    expect(confirmedOwnerCode, _owner.code);
    accepts++;
    items = [_item(preview: _snippetPreview())];
    return items.single;
  }

  @override
  Future<List<SharingOutboxEntry>> rotate(String id, List<SharingGrant> grants) async {
    // Rust adds the original owner itself and rejects duplicate owner input.
    expect(grants.any((g) => g.identity.deviceId == _owner.deviceId), false);
    rotatedGrants = grants;
    return [];
  }

  @override
  Future<SharingItem> reconcile(String id, String confirmedOwnerCode) => accept(id, confirmedOwnerCode);
  @override
  Future<List<SharingOutboxEntry>> discardPending(String mutationId) async => [];
  @override
  Future<String> previewGroup(String groupId, String childrenJson) async =>
      throw UnsupportedError('Fixture has no group');
  @override
  Future<String> previewSecret(String credentialId, {bool passphrase = false}) async =>
      throw UnsupportedError('Fixture has no secret');
  @override
  Future<SharingItem> publishSecret(String credentialId, List<SharingGrant> grants, {bool passphrase = false}) async =>
      throw UnsupportedError('Fixture has no secret');
  @override
  Future<SecretText> revealSecret(String shareId) async => throw UnsupportedError('Fixture has no secret');
  @override
  Future<Host> copyHost(String shareId, {String? credentialId}) async => throw UnsupportedError('Fixture has no copy');
  @override
  Future<Snippet> copySnippet(String shareId) async => throw UnsupportedError('Fixture has no copy');
  @override
  Future<Host> refreshBoundHost(
    String hostId, {
    bool confirmEndpointChange = false,
    String? expectedAddress,
    int? expectedPort,
  }) async => throw UnsupportedError('Fixture has no binding');
  @override
  Future<Host> detachHost(String hostId) async => throw UnsupportedError('Fixture has no binding');
  @override
  Future<List<SharingOutboxEntry>> editSecret(String shareId, SecretText value) async =>
      throw UnsupportedError('Fixture has no secret');
  @override
  Future<Credential> copySecretCredential(String shareId) async => throw UnsupportedError('Fixture has no secret');
}

SharingItem _item({bool owned = false, String? preview, SharingRole role = SharingRole.reader}) => SharingItem(
  id: 'share',
  itemId: 'item',
  kind: SharingKind.snippet,
  ownerUserId: 'owner',
  ownerDeviceId: 'owner-device',
  revision: 1,
  epoch: 1,
  owned: owned,
  trust: preview == null ? SharingTrust.unverified : SharingTrust.verified,
  role: role,
  previewJson: preview,
  members: [_owner, _reader, _editor],
  memberRoles: [
    SharingGrant(_owner, SharingRole.editor, _owner.code),
    SharingGrant(_reader, SharingRole.reader, _reader.code),
    SharingGrant(_editor, SharingRole.editor, _editor.code),
  ],
);

Future<void> _pump(
  WidgetTester tester,
  MockBackend backend,
  _Sharing sharing, {
  AppLocale locale = AppLocale.en,
  Size size = const Size(1600, 1000),
}) async {
  setTestLocale(backend, locale);
  tester.view.physicalSize = size;
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

Future<void> _openSharing(WidgetTester tester) async {
  final container = ProviderScope.containerOf(tester.element(find.byType(ConsoleCryptApp)), listen: false);
  container.read(routerProvider).go(AppRoutes.sharing);
  await settle(tester);
}

Future<void> _openPublish(WidgetTester tester) async {
  showSharingPublish(
    tester.element(find.byType(Navigator).first),
    kind: SharingKind.host,
    objectId: 'personal-host',
  ).ignore();
  await settle(tester);
}

Finder _buttonIn(Type dialog, String label) => find.descendant(
  of: find.byType(dialog),
  matching: find.byWidgetPredicate((w) => w is GlassButton && w.label == label),
);
bool _enabled(WidgetTester tester, Finder finder) => tester.widget<GlassButton>(finder).onPressed != null;

void main() {
  testWidgets('publish requires independent device confirmation and failed dispatch does not duplicate publication', (
    tester,
  ) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    final sharing = _Sharing()..failFlush = true;
    await _pump(tester, backend, sharing);
    await _openPublish(tester);
    expect(find.text('example.test'), findsOneWidget);
    expect(isEnabled(tester, 'sharing-publish-confirm'), false);
    await enterKey(tester, 'sharing-email', 'colleague@example.test');
    await tapKey(tester, 'sharing-find');
    expect(isEnabled(tester, 'sharing-publish-confirm'), false);
    await tapKey(tester, 'sharing-verify-reader-device');
    expect(isEnabled(tester, 'sharing-publish-confirm'), true);
    await tapKey(tester, 'sharing-publish-confirm');
    expect(sharing.publications, 1);
    expect(sharing.flushes, 1);
    expect(sharing.publishedGrants!.single.role, SharingRole.reader);
    expect(sharing.publishedGrants!.single.confirmedCode, _reader.code);
    expect(find.byType(SharingPublishDialog), findsNothing);
    expect(tester.takeException(), isNull);
  });

  testWidgets('only confirmed devices are published with their explicitly selected roles', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    final sharing = _Sharing();
    await _pump(tester, backend, sharing);
    await _openPublish(tester);
    await enterKey(tester, 'sharing-email', 'colleague@example.test');
    await tapKey(tester, 'sharing-find');
    await tapKey(tester, 'sharing-verify-editor-device');
    final picker = find.byType(GlassSelect<SharingRole>);
    await tester.ensureVisible(picker);
    await tester.tap(picker);
    await settle(tester);
    await tester.tap(find.text('Edit').last);
    await settle(tester);
    await tapKey(tester, 'sharing-publish-confirm');
    expect(sharing.publishedGrants!.single.identity.deviceId, _editor.deviceId);
    expect(sharing.publishedGrants!.single.role, SharingRole.editor);
    expect(sharing.publishedGrants!.single.confirmedCode, _editor.code);
    expect(tester.takeException(), isNull);
  });

  testWidgets('received snippet requires owner confirmation and is never automatically executed or imported', (
    tester,
  ) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    final count = backend.snippets.currentSnippets.length;
    final sharing = _Sharing()
      ..currentIdentity = _reader
      ..items = [_item()];
    await _pump(tester, backend, sharing);
    await _openSharing(tester);
    await tester.tap(find.text('Verify and accept'));
    await settle(tester);
    expect(sharing.accepts, 0);
    expect(find.text(_owner.code), findsOneWidget);
    final accept = _buttonIn(SharingAcceptDialog, 'Verify and accept');
    expect(_enabled(tester, accept), false);
    await tapKey(tester, 'sharing-owner-confirmed');
    await tester.tap(accept);
    await settle(tester);
    expect(sharing.accepts, 1);
    expect(find.text('echo explicit-only'), findsOneWidget);
    expect(backend.snippets.currentSnippets.length, count);
    expect(find.text('Edit shared item'), findsNothing);
    expect(find.text('Stop sharing'), findsNothing);
    expect(tester.takeException(), isNull);
  });

  testWidgets('revoke excludes owner and preserves remaining recipient role and code', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    final sharing = _Sharing()..items = [_item(owned: true, preview: _snippetPreview(), role: SharingRole.editor)];
    await _pump(tester, backend, sharing);
    await _openSharing(tester);
    await tapKey(tester, 'sharing-owned');
    final readerRow = find.ancestor(of: find.text(_reader.deviceId), matching: find.byType(ListTile));
    final revoke = find.descendant(of: readerRow, matching: find.byType(GlassIconButton));
    await tester.ensureVisible(revoke);
    await tester.tap(revoke);
    await settle(tester);
    await tapKey(tester, 'confirm-ok');
    expect(sharing.rotatedGrants, isNotNull);
    expect(sharing.rotatedGrants!.single.identity.deviceId, _editor.deviceId);
    expect(sharing.rotatedGrants!.single.role, SharingRole.editor);
    expect(sharing.rotatedGrants!.single.confirmedCode, _editor.code);
    expect(tester.takeException(), isNull);
  });

  testWidgets('another device of owner account cannot manage access', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    final sharing = _Sharing()
      ..currentIdentity = _otherOwnerDevice
      ..items = [_item(owned: true, preview: _snippetPreview())];
    await _pump(tester, backend, sharing);
    await _openSharing(tester);
    await tapKey(tester, 'sharing-owned');
    expect(find.byIcon(Icons.person_remove_outlined), findsNothing);
    expect(find.text('Stop sharing'), findsNothing);
    expect(find.text('Manage access on the owner device that created this item.'), findsOneWidget);
    expect(tester.takeException(), isNull);
  });

  testWidgets('locking vault closes share composer and drops plaintext preview', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    final sharing = _Sharing();
    await _pump(tester, backend, sharing);
    await _openPublish(tester);
    expect(find.text('example.test'), findsOneWidget);
    await backend.services.vault.lock();
    await settle(tester);
    expect(find.text('example.test', skipOffstage: false), findsNothing);
    expect(find.byType(SharingPublishDialog, skipOffstage: false), findsNothing);
    expect(sharing.publications, 0);
    expect(tester.takeException(), isNull);
  });

  testWidgets('late preview completion after lock cannot restore plaintext or publication', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    final completion = Completer<String>();
    final sharing = _Sharing()..delayedPreview = completion;
    await _pump(tester, backend, sharing);
    await _openPublish(tester);
    await backend.services.vault.lock();
    await settle(tester);
    completion.complete(_hostPreview());
    await settle(tester);
    expect(find.text('example.test', skipOffstage: false), findsNothing);
    expect(find.byType(SharingPublishDialog, skipOffstage: false), findsNothing);
    expect(sharing.publications, 0);
    expect(tester.takeException(), isNull);
  });

  testWidgets('a quick lock and unlock of the same profile invalidates an old recipient confirmation', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    final sharing = _Sharing();
    await _pump(tester, backend, sharing);
    await _openPublish(tester);
    await enterKey(tester, 'sharing-email', 'colleague@example.test');
    await tapKey(tester, 'sharing-find');
    await tapKey(tester, 'sharing-verify-reader-device');
    final oldProfile = backend.profiles.currentProfiles.activeId;
    // Both lifecycle transitions occur before the next Flutter frame. The
    // profile UUID remains unchanged, but its old trust decision must expire.
    await backend.services.vault.lock();
    await backend.debugSignInDemoAndUnlock();
    await settle(tester);
    expect(backend.profiles.currentProfiles.activeId, oldProfile);
    expect(find.byType(SharingPublishDialog, skipOffstage: false), findsNothing);
    expect(find.text('example.test', skipOffstage: false), findsNothing);
    expect(sharing.publications, 0);
    expect(tester.takeException(), isNull);
  });

  testWidgets('profile change closes previously confirmed publication', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    final oldProfile = backend.profiles.currentProfiles.activeId;
    final sharing = _Sharing();
    await _pump(tester, backend, sharing);
    await _openPublish(tester);
    await enterKey(tester, 'sharing-email', 'colleague@example.test');
    await tapKey(tester, 'sharing-find');
    await tapKey(tester, 'sharing-verify-reader-device');
    await backend.debugCreateUnlockedLocalProfile(name: 'Separate profile');
    await settle(tester);
    expect(backend.profiles.currentProfiles.activeId, isNot(oldProfile));
    expect(find.text('example.test', skipOffstage: false), findsNothing);
    expect(find.byType(SharingPublishDialog, skipOffstage: false), findsNothing);
    expect(sharing.publications, 0);
    expect(tester.takeException(), isNull);
  });

  testWidgets('late share list from old profile never appears in a new local profile', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    final completion = Completer<List<SharingItem>>();
    final sharing = _Sharing()..delayedList = completion;
    await _pump(tester, backend, sharing);
    await _openSharing(tester);
    await backend.debugCreateUnlockedLocalProfile(name: 'Separate profile');
    await settle(tester);
    completion.complete([_item(preview: _snippetPreview())]);
    await settle(tester);
    await _openSharing(tester);
    expect(find.text('echo explicit-only', skipOffstage: false), findsNothing);
    expect(sharing.accepts, 0);
    expect(tester.takeException(), isNull);
  });

  testWidgets('preview reload cannot leave an invisible confirmed recipient', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    final sharing = _Sharing();
    await _pump(tester, backend, sharing);
    await _openPublish(tester);
    await enterKey(tester, 'sharing-email', 'colleague@example.test');
    await tapKey(tester, 'sharing-find');
    await tapKey(tester, 'sharing-verify-reader-device');
    final notes = find.widgetWithText(CheckboxListTile, 'Include host notes');
    await tester.ensureVisible(notes);
    await tester.tap(notes);
    await settle(tester);
    final verification = find.byKey(const ValueKey('sharing-verify-reader-device'));
    if (verification.evaluate().isEmpty) {
      expect(isEnabled(tester, 'sharing-publish-confirm'), false);
    } else {
      final selected = tester.widget<CheckboxListTile>(verification).value == true;
      expect(isEnabled(tester, 'sharing-publish-confirm'), selected);
    }
    expect(tester.takeException(), isNull);
  });

  testWidgets('queued edit closes composer even when dispatch fails, preventing another edit', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    final sharing = _Sharing()
      ..failFlush = true
      ..currentIdentity = _editor
      ..items = [_item(preview: _snippetPreview(), role: SharingRole.editor)];
    await _pump(tester, backend, sharing);
    await _openSharing(tester);
    await tester.tap(find.text('Edit shared item'));
    await settle(tester);
    await tester.tap(_buttonIn(SharingEditDialog, 'Save'));
    await settle(tester);
    expect(sharing.edits, 1);
    expect(sharing.flushes, 1);
    expect(find.byType(SharingEditDialog), findsNothing);
    expect(tester.takeException(), isNull);
  });

  testWidgets('Russian Android phone verifies and accepts without layout errors', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    final sharing = _Sharing()
      ..currentIdentity = _reader
      ..items = [_item()];
    await _pump(tester, backend, sharing, locale: AppLocale.ru, size: const Size(412, 915));
    await _openSharing(tester);
    await tester.tap(find.text('Проверить и принять'));
    await settle(tester);
    final accept = _buttonIn(SharingAcceptDialog, 'Проверить и принять');
    expect(_enabled(tester, accept), false);
    await tapKey(tester, 'sharing-owner-confirmed');
    await tester.tap(accept);
    await settle(tester);
    expect(sharing.accepts, 1);
    expect(find.text('echo explicit-only'), findsOneWidget);
    expect(tester.takeException(), isNull);
  }, variant: const TargetPlatformVariant({TargetPlatform.android}));

  testWidgets('unsupported sharing service does not discover or publish', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    await pumpApp(tester, backend);
    await _openSharing(tester);
    expect(find.text('Sharing is unavailable on this server.'), findsOneWidget);
    expect(find.byKey(const ValueKey('sharing-publish-confirm')), findsNothing);
    expect(tester.takeException(), isNull);
  });
}
