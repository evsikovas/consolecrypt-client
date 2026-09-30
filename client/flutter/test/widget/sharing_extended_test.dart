import 'dart:async';
import 'dart:convert';
import 'dart:io';
import 'dart:ui' as ui;

import 'package:consolecrypt/app/app.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/mock/mock_backend.dart';
import 'package:consolecrypt/core/mock/mock_cloud.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/services/sharing_service.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/sharing/sharing_collections.dart';
import 'package:consolecrypt/sharing/sharing_models.dart';
import 'package:consolecrypt/sharing/sharing_providers.dart';
import 'package:consolecrypt/sharing/sharing_screen.dart';
import 'package:consolecrypt/sharing/sharing_secrets.dart';
import 'package:flutter/foundation.dart';
import 'package:flutter/rendering.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/test_app.dart';

// Optional local QA captures never contain a revealed secret or real account.
const _captureVisualQa = bool.fromEnvironment('CC_SHARING_VISUAL_QA');

Future<void> _loadVisualQaFonts(WidgetTester tester) async {
  if (!_captureVisualQa || defaultTargetPlatform != TargetPlatform.iOS) return;
  await tester.runAsync(() async {
    final sdk = Platform.environment['CC_WIDGET_QA_FLUTTER_ROOT'];
    if (sdk == null) throw StateError('Set CC_WIDGET_QA_FLUTTER_ROOT for local font loading.');
    for (final entry in {
      '.AppleSystemUIFont': '/System/Library/Fonts/SFNS.ttf',
      'Menlo': '/System/Library/Fonts/SFNSMono.ttf',
      'monospace': '/System/Library/Fonts/SFNSMono.ttf',
      'MaterialIcons': '$sdk/bin/cache/artifacts/material_fonts/MaterialIcons-Regular.otf',
    }.entries) {
      final loader = FontLoader(entry.key);
      loader.addFont(File(entry.value).readAsBytes().then(ByteData.sublistView));
      await loader.load();
    }
  });
}

Future<void> _visualQa(WidgetTester tester, String name) async {
  if (!_captureVisualQa || defaultTargetPlatform != TargetPlatform.iOS) return;
  final boundary = tester.renderObject<RenderRepaintBoundary>(find.byKey(const ValueKey('sharing-test-surface')));
  await tester.runAsync(() async {
    final rendered = await boundary.toImage(pixelRatio: 2);
    try {
      final png = await rendered.toByteData(format: ui.ImageByteFormat.png);
      if (png == null) throw StateError('Widget QA image is unavailable.');
      await File('/private/tmp/consolecrypt-sharing-$name.png').writeAsBytes(png.buffer.asUint8List());
    } finally {
      rendered.dispose();
    }
  });
}

// Synthetic public device identity; secret contents are generated at runtime.
const _recipient = SharingIdentity(
  instance: 'instance',
  userId: 'recipient',
  deviceId: 'recipient-device',
  encryptionKey: 'public-x-recipient',
  signingKey: 'public-ed-recipient',
  code: 'PUBLIC COMPARISON CODE',
);

SharingItem _item(
  String id,
  SharingKind kind, {
  SharingTrust trust = SharingTrust.verified,
  SharingRole role = SharingRole.reader,
  String? secretKind = 'password',
  List<Map<String, Object?>> children = const [],
}) => SharingItem(
  id: id,
  itemId: 'item-$id',
  kind: kind,
  ownerUserId: 'owner',
  ownerDeviceId: 'owner-device',
  revision: 1,
  epoch: 1,
  owned: false,
  trust: trust,
  role: role,
  previewJson: jsonEncode({
    'kind': kind.name,
    'data': {
      'name': 'Name $id',
      if (kind == SharingKind.group) ...{
        'children': children,
        'tags': ['visible-tag'],
      },
      if (kind == SharingKind.secret && secretKind != null) 'secret_kind': secretKind,
    },
  }),
);

Credential _credential() {
  final now = DateTime.now().toUtc();
  return Credential(
    id: ObjectId.generate(),
    name: 'Stored test credential',
    kind: CredentialKind.sshPrivateKey,
    secretId: ObjectId.generate(),
    passphraseSecretId: ObjectId.generate(),
    createdAt: now,
    updatedAt: now,
  );
}

class _Sharing implements SharingService {
  _Sharing({SecretText? source}) : source = source ?? SecretText('runtime-${ObjectId.generate().value}');
  final SecretText source;
  bool supportsGroups = true, supportsSecrets = true, denyReveals = false;
  List<SharingItem> items = [];
  List<Map<String, dynamic>>? selectedChildren;
  String? publication, credentialId, groupEdit;
  List<SharingGrant>? grants;
  bool? passphrase;
  int secretPublications = 0, genericPublications = 0, reveals = 0, imports = 0, edits = 0;
  SecretText? returnedSecret, edited;
  Completer<SecretText>? revealPending;
  @override
  Future<SharingStatus> status() async => SharingStatus(
    enabled: true,
    instance: 'instance',
    supportsGroups: supportsGroups,
    supportsSecrets: supportsSecrets,
  );
  @override
  Future<List<SharingItem>> list({bool refresh = false}) async => items;
  @override
  Future<List<SharingIdentity>> discover(String email) async => [_recipient];
  @override
  Future<String> previewGroup(String groupId, String childrenJson) async {
    selectedChildren = (jsonDecode(childrenJson) as List).cast<Map<String, dynamic>>();
    return jsonEncode({
      'kind': 'group',
      'data': {'name': 'Shared collection', 'tags': <String>[], 'children': selectedChildren},
    });
  }

  @override
  Future<SharingItem> publish(String projectionJson, List<SharingGrant> value) async {
    genericPublications++;
    publication = projectionJson;
    grants = value;
    return _item('published-group', SharingKind.group);
  }

  @override
  Future<List<SharingOutboxEntry>> edit(String id, String projectionJson) async {
    groupEdit = projectionJson;
    return const [];
  }

  @override
  Future<String> previewSecret(String id, {bool passphrase = false}) async {
    credentialId = id;
    this.passphrase = passphrase;
    // Deliberately malformed extra field: the preview must never render it.
    return jsonEncode({
      'kind': 'secret',
      'data': {
        'name': 'Masked source',
        'secret_kind': passphrase ? 'ssh_key_passphrase' : 'ssh_private_key',
        'value': 'FORBIDDEN EXTRA FIELD',
      },
    });
  }

  @override
  Future<SharingItem> publishSecret(String id, List<SharingGrant> value, {bool passphrase = false}) async {
    secretPublications++;
    credentialId = id;
    this.passphrase = passphrase;
    grants = value;
    return _item('published-secret', SharingKind.secret);
  }

  @override
  Future<SecretText> revealSecret(String id) async {
    reveals++;
    if (denyReveals) throw StateError('unavailable');
    if (revealPending != null) return revealPending!.future;
    final bytes = source.exposeBytes();
    try {
      return returnedSecret = SecretText.fromBytes(bytes);
    } finally {
      bytes.fillRange(0, bytes.length, 0);
    }
  }

  @override
  Future<List<SharingOutboxEntry>> editSecret(String id, SecretText value) async {
    edits++;
    final bytes = value.exposeBytes();
    try {
      edited = SecretText.fromBytes(bytes);
    } finally {
      bytes.fillRange(0, bytes.length, 0);
    }
    return const [];
  }

  @override
  Future<Credential> copySecretCredential(String id) async {
    imports++;
    return _credential();
  }

  @override
  Future<List<SharingOutboxEntry>> flush() async => const [];
  // All unrelated APIs must remain unused by these specialized flows.
  @override
  dynamic noSuchMethod(Invocation invocation) => super.noSuchMethod(invocation);
}

Future<MockBackend> _pump(WidgetTester tester, _Sharing sharing, {Size size = const Size(1600, 1100)}) async {
  addTearDown(() {
    sharing.source.wipe();
    sharing.returnedSecret?.wipe();
    sharing.edited?.wipe();
  });
  final backend = testBackend();
  addTearDown(backend.dispose);
  await backend.debugSignInDemoAndUnlock();
  setTestLocale(backend, AppLocale.en);
  if (_captureVisualQa) {
    await backend.services.settings.updateLocal(
      backend.services.settings.currentLocal.copyWith(themeMode: AppThemeMode.dark),
    );
  }
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
      child: const RepaintBoundary(key: ValueKey('sharing-test-surface'), child: ConsoleCryptApp()),
    ),
  );
  await settle(tester);
  return backend;
}

BuildContext _context(WidgetTester tester) => tester.element(find.byType(Navigator).first);

Future<void> _recipientSelection(WidgetTester tester) async {
  await enterKey(tester, 'sharing-email', 'recipient@example.test');
  await tapKey(tester, 'sharing-find');
  await tapKey(tester, 'sharing-verify-recipient-device');
}

void main() {
  testWidgets('Windows synced sidebar renders and opens the sharing navigation item', (tester) async {
    await _pump(tester, _Sharing());
    await tapKey(tester, 'nav-sharing');
    expect(find.byType(SharingScreen), findsOneWidget);
    expect(tester.takeException(), isNull);
  }, variant: const TargetPlatformVariant({TargetPlatform.windows}));

  testWidgets('phone publication and a long revealed secret remain scrollable without overflow', (tester) async {
    await _loadVisualQaFonts(tester);
    final generated = List.generate(80, (_) => ObjectId.generate().value).join('\n');
    final sharing = _Sharing(source: SecretText(generated));
    await _pump(tester, sharing, size: const Size(412, 915));
    showSharingSecretPublish(_context(tester), credential: _credential()).ignore();
    await settle(tester);
    await _visualQa(tester, 'publish-top-ios');
    await _recipientSelection(tester);
    FocusManager.instance.primaryFocus?.unfocus();
    await tester.pump();
    expect(tester.takeException(), isNull);
    await _visualQa(tester, 'publish-ios');
    Navigator.of(tester.element(find.byType(SharingSecretPublishDialog))).pop();
    await settle(tester);
    showSharingSecret(_context(tester), _item('key', SharingKind.secret, secretKind: 'ssh_private_key')).ignore();
    await settle(tester);
    await _visualQa(tester, 'masked-ios');
    await tapKey(tester, 'sharing-secret-reveal');
    final secret = sharing.returnedSecret!;
    expect(secret.isWiped, false);
    expect(tester.takeException(), isNull);
    Navigator.of(tester.element(find.byType(SharingSecretDialog))).pop();
    await settle(tester);
    expect(secret.isWiped, true);
  }, variant: const TargetPlatformVariant({TargetPlatform.iOS, TargetPlatform.android}));

  testWidgets('collection publication contains only explicitly selected independent share references', (tester) async {
    final sharing = _Sharing()
      ..items = [_item('host', SharingKind.host), _item('blocked', SharingKind.snippet, trust: SharingTrust.blocked)];
    await _pump(tester, sharing);
    showSharingGroupPublish(_context(tester), groupId: 'personal-group').ignore();
    await settle(tester);
    expect(sharing.selectedChildren, isEmpty);
    expect(find.byKey(const ValueKey('sharing-child-blocked')), findsNothing);
    expect(isEnabled(tester, 'sharing-group-publish'), false);
    await tapKey(tester, 'sharing-child-host');
    expect(sharing.selectedChildren, [
      {'share_id': 'host', 'item_id': 'item-host', 'kind': 'host'},
    ]);
    await _recipientSelection(tester);
    await tapKey(tester, 'sharing-group-publish');
    expect(sharing.genericPublications, 1);
    final body = jsonDecode(sharing.publication!) as Map<String, dynamic>;
    expect((body['data'] as Map)['children'], sharing.selectedChildren);
    expect(sharing.grants!.single.identity.deviceId, _recipient.deviceId);
    expect(sharing.secretPublications, 0);
    expect(tester.takeException(), isNull);
  });

  testWidgets('unsupported collection capability prevents preview and recipients', (tester) async {
    final sharing = _Sharing()..supportsGroups = false;
    await _pump(tester, sharing);
    showSharingGroupPublish(_context(tester), groupId: 'personal-group').ignore();
    await settle(tester);
    expect(sharing.selectedChildren, isNull);
    expect(find.byKey(const ValueKey('sharing-email')), findsNothing);
    expect(isEnabled(tester, 'sharing-group-publish'), false);
  });

  testWidgets(
    'group Editor changes explicit references while preserving tags and unresolved IDs, without ACL changes',
    (tester) async {
      final sharing = _Sharing()..items = [_item('host', SharingKind.host), _item('nested', SharingKind.group)];
      await _pump(tester, sharing);
      final missing = <String, Object?>{'share_id': 'missing', 'item_id': 'exact-missing-id', 'kind': 'snippet'};
      showSharingGroupEdit(
        _context(tester),
        _item('edited', SharingKind.group, role: SharingRole.editor, children: [missing]),
      ).ignore();
      await settle(tester);
      expect(find.byKey(const ValueKey('sharing-email')), findsNothing);
      expect(find.byKey(const ValueKey('sharing-child-nested')), findsNothing);
      expect(find.byKey(const ValueKey('sharing-unresolved-missing')), findsOneWidget);
      await enterKey(tester, 'sharing-group-name', 'Explicit new name');
      await tapKey(tester, 'sharing-child-host');
      await tapKey(tester, 'sharing-group-publish');
      final body = jsonDecode(sharing.groupEdit!) as Map<String, dynamic>;
      expect(body.keys.toSet(), {'kind', 'data'});
      final data = body['data'] as Map<String, dynamic>;
      expect(data.keys.toSet(), {'name', 'tags', 'children'});
      expect(data['name'], 'Explicit new name');
      expect(data['tags'], ['visible-tag']);
      expect(data['children'], [
        {'share_id': 'host', 'item_id': 'item-host', 'kind': 'host'},
        missing,
      ]);
      expect(sharing.genericPublications, 0);
      expect(sharing.grants, isNull);
      expect(tester.takeException(), isNull);
    },
  );

  testWidgets('group Reader cannot edit and nested legacy references are not silently dropped', (tester) async {
    final sharing = _Sharing();
    await _pump(tester, sharing);
    showSharingGroupEdit(_context(tester), _item('reader', SharingKind.group)).ignore();
    await settle(tester);
    expect(isEnabled(tester, 'sharing-group-publish'), false);
    Navigator.of(tester.element(find.byType(SharingGroupPublishDialog))).pop();
    await settle(tester);
    showSharingGroupEdit(
      _context(tester),
      _item(
        'nested',
        SharingKind.group,
        role: SharingRole.editor,
        children: [
          {'share_id': 'other', 'item_id': 'item-other', 'kind': 'group'},
        ],
      ),
    ).ignore();
    await settle(tester);
    expect(isEnabled(tester, 'sharing-group-publish'), false);
    expect(sharing.groupEdit, isNull);
  });

  testWidgets('collection cannot authorize missing, blocked or substituted child IDs', (tester) async {
    final sharing = _Sharing();
    await _pump(tester, sharing);
    final refs = <Map<String, Object?>>[
      {'share_id': 'good', 'item_id': 'item-good', 'kind': 'host'},
      {'share_id': 'blocked', 'item_id': 'item-blocked', 'kind': 'host'},
      {'share_id': 'substituted', 'item_id': 'wrong-item', 'kind': 'host'},
      {'share_id': 'missing', 'item_id': 'item-missing', 'kind': 'secret'},
    ];
    final opened = <String>[];
    showAppDialog<void>(
      _context(tester),
      secure: true,
      builder: (_) => GlassDialog(
        title: 'Collection test',
        content: SharingCollectionView(
          group: _item('group', SharingKind.group, children: refs),
          items: [
            _item('good', SharingKind.host),
            _item('blocked', SharingKind.host, trust: SharingTrust.blocked),
            _item('substituted', SharingKind.host),
          ],
          onOpen: (item) => opened.add(item.id),
        ),
      ),
    ).ignore();
    await settle(tester);
    expect(find.byIcon(Icons.lock_outline), findsNWidgets(3));
    await tapKey(tester, 'sharing-reference-good');
    expect(opened, ['good']);
    for (final id in ['blocked', 'substituted', 'missing']) {
      final tile = tester.widget<ListTile>(find.byKey(ValueKey('sharing-reference-$id')));
      expect(tile.onTap, isNull);
    }
  });

  testWidgets('secret publication is masked and requires both explicit consent and verified recipients', (
    tester,
  ) async {
    final sharing = _Sharing();
    await _pump(tester, sharing);
    final source = _credential();
    showSharingSecretPublish(_context(tester), credential: source).ignore();
    await settle(tester);
    expect(find.text('FORBIDDEN EXTRA FIELD'), findsNothing);
    expect(sharing.reveals, 0);
    await _recipientSelection(tester);
    expect(isEnabled(tester, 'sharing-secret-publish'), false);
    await tapKey(tester, 'sharing-secret-confirmed');
    expect(isEnabled(tester, 'sharing-secret-publish'), true);
    await tapKey(tester, 'sharing-secret-publish');
    expect(sharing.secretPublications, 1);
    expect(sharing.genericPublications, 0);
    expect(sharing.credentialId, source.id.value);
    expect(sharing.passphrase, false);
    expect(sharing.grants!.single.confirmedCode, _recipient.code);
    expect(tester.takeException(), isNull);
  });

  testWidgets('key passphrase selection is independent of primary key publication', (tester) async {
    final sharing = _Sharing();
    await _pump(tester, sharing);
    final source = _credential();
    showSharingSecretPublish(_context(tester), credential: source, passphrase: true).ignore();
    await settle(tester);
    expect(sharing.passphrase, true);
    await _recipientSelection(tester);
    await tapKey(tester, 'sharing-secret-confirmed');
    await tapKey(tester, 'sharing-secret-publish');
    expect(sharing.secretPublications, 1);
    expect(sharing.passphrase, true);
    expect(sharing.genericPublications, 0);
  });

  testWidgets('secret capability off cannot fetch or publish a secret', (tester) async {
    final sharing = _Sharing()..supportsSecrets = false;
    await _pump(tester, sharing);
    showSharingSecretPublish(_context(tester), credential: _credential()).ignore();
    await settle(tester);
    expect(sharing.credentialId, isNull);
    expect(find.byKey(const ValueKey('sharing-email')), findsNothing);
    expect(isEnabled(tester, 'sharing-secret-publish'), false);
  });

  testWidgets('revealed secret is wiped on hide and automatically after twenty seconds', (tester) async {
    final sharing = _Sharing();
    await _pump(tester, sharing);
    showSharingSecret(_context(tester), _item('secret', SharingKind.secret)).ignore();
    await settle(tester);
    expect(sharing.reveals, 0);
    expect(isEnabled(tester, 'sharing-secret-copy'), false);
    await tapKey(tester, 'sharing-secret-reveal');
    final first = sharing.returnedSecret!;
    expect(tester.widget<Text>(find.byKey(const ValueKey('sharing-secret-value'))).data == first.expose(), true);
    await tapKey(tester, 'sharing-secret-reveal');
    expect(first.isWiped, true);
    await tapKey(tester, 'sharing-secret-reveal');
    final second = sharing.returnedSecret!;
    await tester.pump(const Duration(seconds: 21));
    expect(second.isWiped, true);
    expect(isEnabled(tester, 'sharing-secret-copy'), false);
  });

  testWidgets('revealed secret is wiped and dialog closes when vault locks', (tester) async {
    final sharing = _Sharing();
    final backend = await _pump(tester, sharing);
    showSharingSecret(_context(tester), _item('secret', SharingKind.secret)).ignore();
    await settle(tester);
    await tapKey(tester, 'sharing-secret-reveal');
    final secret = sharing.returnedSecret!;
    await backend.services.vault.lock();
    await settle(tester);
    expect(secret.isWiped, true);
    expect(find.byType(SharingSecretDialog), findsNothing);
    expect(tester.takeException(), isNull);
  });

  testWidgets('copy uses timed secret clipboard and wipes the display buffer', (tester) async {
    final sharing = _Sharing();
    await _pump(tester, sharing);
    String? clipboard;
    tester.binding.defaultBinaryMessenger.setMockMethodCallHandler(SystemChannels.platform, (call) async {
      if (call.method == 'Clipboard.setData') clipboard = (call.arguments as Map)['text'] as String?;
      if (call.method == 'Clipboard.getData') return {'text': clipboard};
      return null;
    });
    addTearDown(() => tester.binding.defaultBinaryMessenger.setMockMethodCallHandler(SystemChannels.platform, null));
    showSharingSecret(_context(tester), _item('secret', SharingKind.secret)).ignore();
    await settle(tester);
    await tapKey(tester, 'sharing-secret-reveal');
    final secret = sharing.returnedSecret!;
    final sameValue = secret.expose();
    await tapKey(tester, 'sharing-secret-copy');
    expect(sharing.reveals, 2);
    expect(clipboard == sameValue, true);
    expect(secret.isWiped, true);
    expect(isEnabled(tester, 'sharing-secret-copy'), false);
    await tester.pump(const Duration(seconds: 31));
    await settle(tester);
    expect(clipboard?.isEmpty, true);
  });

  testWidgets('blocked shared secret cannot reveal its plaintext', (tester) async {
    final sharing = _Sharing();
    await _pump(tester, sharing);
    showSharingSecret(_context(tester), _item('secret', SharingKind.secret, trust: SharingTrust.blocked)).ignore();
    await settle(tester);
    expect(isEnabled(tester, 'sharing-secret-reveal'), false);
    expect(sharing.reveals, 0);
  });

  testWidgets('late secret reveal after quick lock and unlock is discarded and wiped', (tester) async {
    final sharing = _Sharing()..revealPending = Completer<SecretText>();
    final backend = await _pump(tester, sharing);
    showSharingSecret(_context(tester), _item('secret', SharingKind.secret)).ignore();
    await settle(tester);
    await tapKey(tester, 'sharing-secret-reveal');
    await backend.services.vault.lock();
    await backend.services.vault.unlockWithPassphrase(SecretText(MockCloud.demoPassphrase));
    await settle(tester);
    final late = SecretText('runtime-${ObjectId.generate().value}');
    sharing.revealPending!.complete(late);
    await settle(tester);
    expect(late.isWiped, true);
    expect(find.byType(SharingSecretDialog), findsNothing);
    expect(tester.takeException(), isNull);
  });

  testWidgets('revoked access cannot copy the previously displayed secret', (tester) async {
    final sharing = _Sharing();
    await _pump(tester, sharing);
    var clipboardWrites = 0;
    tester.binding.defaultBinaryMessenger.setMockMethodCallHandler(SystemChannels.platform, (call) async {
      if (call.method == 'Clipboard.setData') clipboardWrites++;
      return null;
    });
    addTearDown(() => tester.binding.defaultBinaryMessenger.setMockMethodCallHandler(SystemChannels.platform, null));
    showSharingSecret(_context(tester), _item('secret', SharingKind.secret)).ignore();
    await settle(tester);
    await tapKey(tester, 'sharing-secret-reveal');
    final displayed = sharing.returnedSecret!;
    sharing.denyReveals = true;
    await tapKey(tester, 'sharing-secret-copy');
    expect(sharing.reveals, 2);
    expect(displayed.isWiped, true);
    expect(clipboardWrites, 0);
    expect(tester.takeException(), isNull);
  });

  testWidgets('secret import requires confirmation and passphrase cannot become an automatic credential', (
    tester,
  ) async {
    final sharing = _Sharing();
    await _pump(tester, sharing);
    showSharingSecret(_context(tester), _item('secret', SharingKind.secret)).ignore();
    await settle(tester);
    await tapKey(tester, 'sharing-secret-import');
    expect(sharing.imports, 0);
    await tapKey(tester, 'sharing-secret-import-confirm');
    expect(sharing.imports, 1);
    Navigator.of(tester.element(find.byType(SharingSecretDialog))).pop();
    await settle(tester);
    showSharingSecret(
      _context(tester),
      _item('passphrase', SharingKind.secret, secretKind: 'ssh_key_passphrase'),
    ).ignore();
    await settle(tester);
    expect(find.byKey(const ValueKey('sharing-secret-import')), findsNothing);
    expect(sharing.reveals, 0);
  });

  testWidgets('reader and unknown secret type cannot edit', (tester) async {
    final sharing = _Sharing();
    await _pump(tester, sharing);
    showSharingSecretEdit(_context(tester), _item('reader', SharingKind.secret)).ignore();
    await settle(tester);
    expect(isEnabled(tester, 'sharing-secret-edit-save'), false);
    expect(tester.widget<TextField>(find.byKey(const ValueKey('sharing-secret-edit-value'))).enabled, false);
    Navigator.of(tester.element(find.byType(SharingSecretEditDialog))).pop();
    await settle(tester);
    showSharingSecretEdit(
      _context(tester),
      _item('unknown', SharingKind.secret, role: SharingRole.editor, secretKind: null),
    ).ignore();
    await settle(tester);
    expect(tester.widget<TextField>(find.byKey(const ValueKey('sharing-secret-edit-value'))).enabled, false);
    expect(sharing.edits, 0);
  });

  testWidgets('editor replacement needs explicit confirmation and bypasses generic JSON', (tester) async {
    final sharing = _Sharing();
    await _pump(tester, sharing);
    showSharingSecretEdit(_context(tester), _item('editor', SharingKind.secret, role: SharingRole.editor)).ignore();
    await settle(tester);
    final replacement = SecretText('runtime-${ObjectId.generate().value}');
    addTearDown(replacement.wipe);
    await enterKey(tester, 'sharing-secret-edit-value', replacement.expose());
    expect(isEnabled(tester, 'sharing-secret-edit-save'), false);
    await tapKey(tester, 'sharing-secret-edit-confirmed');
    await tapKey(tester, 'sharing-secret-edit-save');
    expect(sharing.edits, 1);
    expect(sharing.edited!.constantTimeEquals(replacement), true);
    sharing.edited!.wipe();
    expect(sharing.genericPublications, 0);
    expect(find.byType(SharingSecretEditDialog), findsNothing);
    expect(tester.takeException(), isNull);
  });
}
