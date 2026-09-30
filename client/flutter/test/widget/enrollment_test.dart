import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:consolecrypt/app/app.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/mock/mock_backend.dart';
import 'package:consolecrypt/core/mock/mock_cloud.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/services/enrollment_service.dart';
import 'package:consolecrypt/core/services/file_dialog_service.dart';
import 'package:consolecrypt/sharing/enrollment_models.dart';
import 'package:consolecrypt/sharing/enrollment_owner.dart';
import 'package:consolecrypt/sharing/enrollment_pairing.dart';
import 'package:consolecrypt/sharing/sharing_models.dart';
import 'package:consolecrypt/sharing/sharing_providers.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/test_app.dart';

// Synthetic public identity, never an SSH credential or private key fixture.
const _target = SharingIdentity(
  instance: 'instance',
  userId: 'target-user',
  deviceId: 'target-device',
  encryptionKey: 'public-x-target',
  signingKey: 'public-ed-target',
  code: 'PUBLIC DEVICE CODE',
);

const _editor = SharingIdentity(
  instance: 'instance',
  userId: 'target-user',
  deviceId: 'editor-anchor',
  encryptionKey: 'public-x-editor',
  signingKey: 'public-ed-editor',
  code: 'PUBLIC EDITOR CODE',
);

SharingItem _ownedItem() => SharingItem(
  id: 'shared-item',
  itemId: 'item',
  kind: SharingKind.host,
  ownerUserId: 'owner',
  ownerDeviceId: 'owner-device',
  revision: 1,
  epoch: 1,
  owned: true,
  trust: SharingTrust.verified,
  memberRoles: [
    SharingGrant(_target, SharingRole.reader, _target.code),
    SharingGrant(_editor, SharingRole.editor, _editor.code),
  ],
);

EnrollmentGrant _grant({EnrollmentGrantCreate? from, bool revoked = false}) => EnrollmentGrant(
  shareId: 'shared-item',
  id: 'permission',
  anchor: from?.anchor ?? _target,
  roleCeiling: from?.roleCeiling ?? SharingRole.reader,
  mode: from?.mode ?? EnrollmentMode.manual,
  state: revoked ? EnrollmentGrantState.revoked : EnrollmentGrantState.active,
  expires: from?.expires ?? DateTime.now().toUtc().add(const Duration(hours: 1)),
  maxAdmissions: from?.maxAdmissions ?? 1,
  admitted: 0,
);

String _packet(String stage) => jsonEncode({'public_stage': stage, 'nonce': ObjectId.generate().value});

EnrollmentPairing _pairing(String packet, {bool expired = false, bool missingTarget = false}) {
  final source = (jsonDecode(packet) as Map)['public_stage'] == 'source';
  return EnrollmentPairing(
    shareId: 'shared-item',
    grantId: 'permission',
    bundle: packet,
    expires: DateTime.now().toUtc().add(Duration(hours: expired ? -1 : 1)),
    requestId: source ? null : 'request',
    code: source ? null : 'PUBLIC FULL REQUEST COMPARISON CODE',
    target: source || missingTarget ? null : _target,
    requestedRole: source ? null : SharingRole.reader,
  );
}

EnrollmentRequest _request(EnrollmentRequestState state) => EnrollmentRequest(
  shareId: 'shared-item',
  grantId: 'permission',
  id: 'request',
  target: _target,
  role: SharingRole.reader,
  code: 'PUBLIC FULL REQUEST COMPARISON CODE',
  state: state,
  expires: DateTime.now().toUtc().add(const Duration(hours: 1)),
);

class _Enrollment extends UnavailableEnrollmentService {
  int reviews = 0, preparations = 0, endorsements = 0, submissions = 0, responses = 0, reloads = 0;
  int creations = 0, revocations = 0;
  int restorations = 0;
  EnrollmentGrantCreate? created;
  List<EnrollmentGrant> permissions = [];
  SharingRole? requestedRole;
  String? confirmedCode;
  bool expired = false, missingTarget = false;
  Completer<EnrollmentPairing>? reviewPending;
  List<EnrollmentRequest> pending = [_request(EnrollmentRequestState.challenged)];
  @override
  Future<List<EnrollmentGrant>> grants(String shareId) async => permissions;

  @override
  Future<List<EnrollmentRequest>> requests(String shareId) async => const [];

  @override
  Future<EnrollmentGrant> createGrant(String shareId, EnrollmentGrantCreate create) async {
    creations++;
    created = create;
    return _grant(from: create);
  }

  @override
  Future<void> revokeGrant(String shareId, String grantId) async {
    revocations++;
    permissions = [_grant(revoked: true)];
  }

  @override
  Future<EnrollmentPairing> inspectPairing(String bundle) async {
    reviews++;
    if (reviewPending != null) return reviewPending!.future;
    return _pairing(bundle, expired: expired, missingTarget: missingTarget);
  }

  @override
  Future<EnrollmentPairing> prepareTarget(String bundle, SharingRole role) async {
    preparations++;
    requestedRole = role;
    return _pairing(_packet('target'));
  }

  @override
  Future<EnrollmentPairing> endorseTarget(String bundle, String code) async {
    endorsements++;
    confirmedCode = code;
    return _pairing(_packet('endorsed'));
  }

  @override
  Future<EnrollmentRequest> submitTarget(String bundle, String code) async {
    submissions++;
    confirmedCode = code;
    return _request(EnrollmentRequestState.pending);
  }

  @override
  Future<void> restorePairing(String bundle, String code) async {
    restorations++;
    confirmedCode = code;
  }

  @override
  Future<List<EnrollmentRequest>> pendingRequests() async {
    reloads++;
    return pending;
  }

  @override
  Future<EnrollmentRequest> respond(String shareId, String requestId) async {
    responses++;
    pending = [_request(EnrollmentRequestState.responded)];
    return pending.single;
  }
}

class _Files implements FileDialogService {
  String? openPath, savePath;
  int finished = 0;
  @override
  Future<String?> chooseOpenFile({List<String> extensions = const []}) async => openPath;
  @override
  Future<String?> chooseSaveFile({required String suggestedName, List<String> extensions = const []}) async => savePath;
  @override
  Future<bool> finishSaveFile(String path) async {
    finished++;
    return true;
  }

  @override
  dynamic noSuchMethod(Invocation invocation) => super.noSuchMethod(invocation);
}

Future<MockBackend> _pump(
  WidgetTester tester,
  _Enrollment enrollment, {
  Size size = const Size(1500, 1050),
  _Files? files,
  bool? ownerCapability,
  AppLocale locale = AppLocale.en,
}) async {
  final backend = testBackend();
  addTearDown(backend.dispose);
  await backend.debugSignInDemoAndUnlock();
  setTestLocale(backend, locale);
  tester.view.physicalSize = size;
  tester.view.devicePixelRatio = 1;
  addTearDown(tester.view.reset);
  await tester.pumpWidget(
    ProviderScope(
      overrides: [
        appServicesProvider.overrideWithValue(backend.services),
        enrollmentServiceProvider.overrideWithValue(enrollment),
        if (files != null) fileDialogServiceProvider.overrideWithValue(files),
        if (ownerCapability != null) ...[
          sharingStatusProvider.overrideWith(
            (ref) async => SharingStatus(enabled: true, supportsOwnerOnlineEnrollment: ownerCapability),
          ),
          sharingItemsProvider.overrideWith((ref) async => const []),
          sharingOutboxProvider.overrideWith((ref) async => const []),
        ],
      ],
      retry: (_, _) => null,
      child: const ConsoleCryptApp(),
    ),
  );
  await settle(tester);
  return backend;
}

BuildContext _context(WidgetTester tester) => tester.element(find.byType(Navigator).first);

Future<void> _review(WidgetTester tester, String packet) async {
  await enterKey(tester, 'enrollment-packet', packet);
  await tapKey(tester, 'enrollment-review');
}

Future<T> _io<T>(WidgetTester tester, Future<T> Function() work) async => (await tester.runAsync(work)) as T;

Future<void> _tapFile(WidgetTester tester, String key) async {
  await tester.runAsync(() async {
    await tester.tap(find.byKey(ValueKey(key)));
    await Future<void>.delayed(const Duration(milliseconds: 100));
  });
  await settle(tester);
}

Future<void> _anchor(WidgetTester tester, String id) async {
  final picker = find.byType(GlassSelect<String?>);
  await tester.ensureVisible(picker);
  await tester.tap(picker);
  await settle(tester);
  await tester.tap(find.text(id).last);
  await settle(tester);
}

void main() {
  testWidgets('restoring local request state requires whole code and a fresh session without creating access', (
    tester,
  ) async {
    final enrollment = _Enrollment();
    final backend = await _pump(tester, enrollment);
    final packet = _packet('endorsed');
    showEnrollmentPairing(_context(tester), EnrollmentPairingFlow.restore).ignore();
    await settle(tester);
    await _review(tester, packet);
    expect(find.text('PUBLIC FULL REQUEST COMPARISON CODE'), findsOneWidget);
    expect(isEnabled(tester, 'enrollment-continue'), false);
    await tapKey(tester, 'enrollment-code-confirmed');
    await backend.services.vault.lock();
    await backend.services.vault.unlockWithPassphrase(SecretText(MockCloud.demoPassphrase));
    await settle(tester);
    expect(find.byType(EnrollmentPairingDialog), findsNothing);
    expect(enrollment.restorations, 0);
    showEnrollmentPairing(_context(tester), EnrollmentPairingFlow.restore).ignore();
    await settle(tester);
    await _review(tester, packet);
    expect(isEnabled(tester, 'enrollment-continue'), false);
    await tapKey(tester, 'enrollment-code-confirmed');
    await tapKey(tester, 'enrollment-continue');
    expect(enrollment.restorations, 1);
    expect(enrollment.confirmedCode, 'PUBLIC FULL REQUEST COMPARISON CODE');
    expect(enrollment.creations + enrollment.preparations + enrollment.endorsements + enrollment.submissions, 0);
    expect(find.byType(EnrollmentPairingDialog), findsNothing);
    expect(tester.takeException(), isNull);
  });

  testWidgets('owner grant defaults are manual Reader quota one and require unchecked explicit confirmation', (
    tester,
  ) async {
    final enrollment = _Enrollment();
    await _pump(tester, enrollment, ownerCapability: true);
    showEnrollmentOwner(_context(tester), _ownedItem()).ignore();
    await settle(tester);
    await tapKey(tester, 'enrollment-new-grant');
    expect(isEnabled(tester, 'enrollment-grant-create'), false);
    await _anchor(tester, _target.deviceId);
    expect(tester.widget<GlassSelect<SharingRole>>(find.byType(GlassSelect<SharingRole>)).value, SharingRole.reader);
    expect(tester.widget<CheckboxListTile>(find.byKey(const ValueKey('enrollment-grant-confirmed'))).value, false);
    expect(isEnabled(tester, 'enrollment-grant-create'), false);
    expect(enrollment.creations, 0);
    await tapKey(tester, 'enrollment-grant-confirmed');
    await tapKey(tester, 'enrollment-grant-create');
    expect(enrollment.created!.mode, EnrollmentMode.manual);
    expect(enrollment.created!.roleCeiling, SharingRole.reader);
    expect(enrollment.created!.maxAdmissions, 1);
    expect(enrollment.created!.anchor.deviceId, _target.deviceId);
    expect(enrollment.creations, 1);
    expect(tester.takeException(), isNull);
  });

  testWidgets('Automatic enrollment is an explicit opt-in and a Reader anchor cannot offer Editor', (tester) async {
    final enrollment = _Enrollment();
    await _pump(tester, enrollment, ownerCapability: true);
    showEnrollmentOwner(_context(tester), _ownedItem()).ignore();
    await settle(tester);
    await tapKey(tester, 'enrollment-new-grant');
    await _anchor(tester, _target.deviceId);
    final readerRoles = tester.widget<GlassSelect<SharingRole>>(find.byType(GlassSelect<SharingRole>));
    expect(readerRoles.items.map((item) => item.value), [SharingRole.reader]);
    final automatic = find.widgetWithText(CheckboxListTile, 'Automatically accept confirmed devices');
    expect(tester.widget<CheckboxListTile>(automatic).value, false);
    await _anchor(tester, _editor.deviceId);
    final picker = find.byType(GlassSelect<SharingRole>);
    await tester.ensureVisible(picker);
    await tester.tap(picker);
    await settle(tester);
    await tester.tap(find.text('Edit').last);
    await settle(tester);
    await tester.ensureVisible(automatic);
    await tester.tap(automatic);
    await settle(tester);
    expect(enrollment.creations, 0);
    expect(isEnabled(tester, 'enrollment-grant-create'), false);
    await tapKey(tester, 'enrollment-grant-confirmed');
    await tapKey(tester, 'enrollment-grant-create');
    expect(enrollment.created!.mode, EnrollmentMode.automatic);
    expect(enrollment.created!.roleCeiling, SharingRole.editor);
    expect(enrollment.created!.anchor.deviceId, _editor.deviceId);
    expect(tester.takeException(), isNull);
  });

  testWidgets('capability-off owner can revoke an existing permission but cannot create another', (tester) async {
    final enrollment = _Enrollment()..permissions = [_grant()];
    await _pump(tester, enrollment, ownerCapability: false);
    showEnrollmentOwner(_context(tester), _ownedItem()).ignore();
    await settle(tester);
    expect(isEnabled(tester, 'enrollment-new-grant'), false);
    expect(find.text(_target.deviceId), findsOneWidget);
    final disable = find.byWidgetPredicate(
      (widget) => widget is GlassIconButton && widget.tooltip == 'Disable permission',
    );
    expect(disable, findsOneWidget);
    await tester.tap(disable);
    await settle(tester);
    expect(enrollment.revocations, 0);
    await tapKey(tester, 'confirm-ok');
    expect(enrollment.revocations, 1);
    expect(enrollment.creations, 0);
    expect(find.byType(EnrollmentOwnerDialog), findsOneWidget);
    expect(tester.takeException(), isNull);
  });

  testWidgets('quick lock-unlock cannot preserve the confirmed owner grant composer', (tester) async {
    final enrollment = _Enrollment();
    final backend = await _pump(tester, enrollment, ownerCapability: true);
    showEnrollmentOwner(_context(tester), _ownedItem()).ignore();
    await settle(tester);
    await tapKey(tester, 'enrollment-new-grant');
    await _anchor(tester, _target.deviceId);
    await tapKey(tester, 'enrollment-grant-confirmed');
    expect(isEnabled(tester, 'enrollment-grant-create'), true);
    await backend.services.vault.lock();
    await backend.services.vault.unlockWithPassphrase(SecretText(MockCloud.demoPassphrase));
    await settle(tester);
    expect(find.byType(EnrollmentCreateDialog), findsNothing);
    expect(find.byType(EnrollmentOwnerDialog), findsNothing);
    expect(enrollment.creations, 0);
    expect(tester.takeException(), isNull);
  });

  testWidgets('Russian phone owner can grant limited access without layout overflow', (tester) async {
    final enrollment = _Enrollment();
    await _pump(tester, enrollment, ownerCapability: true, locale: AppLocale.ru, size: const Size(412, 915));
    showEnrollmentOwner(_context(tester), _ownedItem()).ignore();
    await settle(tester);
    await tapKey(tester, 'enrollment-new-grant');
    await _anchor(tester, _target.deviceId);
    await tapKey(tester, 'enrollment-grant-confirmed');
    await tapKey(tester, 'enrollment-grant-create');
    expect(enrollment.created!.mode, EnrollmentMode.manual);
    expect(enrollment.creations, 1);
    expect(tester.takeException(), isNull);
  }, variant: const TargetPlatformVariant({TargetPlatform.iOS, TargetPlatform.android}));

  testWidgets('prepare inspects a source packet and defaults to Reader without accepting access', (tester) async {
    final enrollment = _Enrollment();
    await _pump(tester, enrollment);
    showEnrollmentPairing(_context(tester), EnrollmentPairingFlow.prepare).ignore();
    await settle(tester);
    expect(isEnabled(tester, 'enrollment-continue'), false);
    await _review(tester, _packet('source'));
    expect(find.byKey(const ValueKey('enrollment-code-confirmed')), findsNothing);
    await tapKey(tester, 'enrollment-continue');
    expect(enrollment.preparations, 1);
    expect(enrollment.requestedRole, SharingRole.reader);
    expect(enrollment.endorsements + enrollment.submissions, 0);
    expect(find.byType(EnrollmentPackageDialog), findsOneWidget);
    expect(enrollment.reviews, 2);
    expect(tester.takeException(), isNull);
  });

  testWidgets('endorse displays the whole request code and requires an explicit comparison', (tester) async {
    final enrollment = _Enrollment();
    await _pump(tester, enrollment);
    showEnrollmentPairing(_context(tester), EnrollmentPairingFlow.endorse).ignore();
    await settle(tester);
    await _review(tester, _packet('target'));
    expect(find.text('PUBLIC FULL REQUEST COMPARISON CODE'), findsOneWidget);
    expect(find.text(_target.deviceId), findsOneWidget);
    expect(isEnabled(tester, 'enrollment-continue'), false);
    await tapKey(tester, 'enrollment-code-confirmed');
    await tapKey(tester, 'enrollment-continue');
    expect(enrollment.endorsements, 1);
    expect(enrollment.confirmedCode, 'PUBLIC FULL REQUEST COMPARISON CODE');
    expect(enrollment.submissions, 0);
    expect(find.byType(EnrollmentPackageDialog), findsOneWidget);
    expect(tester.takeException(), isNull);
  });

  testWidgets('submit requires independent code confirmation and only sends a pending request', (tester) async {
    final enrollment = _Enrollment();
    await _pump(tester, enrollment);
    showEnrollmentPairing(_context(tester), EnrollmentPairingFlow.submit).ignore();
    await settle(tester);
    await _review(tester, _packet('endorsed'));
    expect(isEnabled(tester, 'enrollment-continue'), false);
    await tapKey(tester, 'enrollment-code-confirmed');
    await tapKey(tester, 'enrollment-continue');
    expect(enrollment.submissions, 1);
    expect(enrollment.confirmedCode, 'PUBLIC FULL REQUEST COMPARISON CODE');
    expect(enrollment.preparations + enrollment.endorsements, 0);
    expect(find.byType(EnrollmentPairingDialog), findsNothing);
    expect(tester.takeException(), isNull);
  });

  testWidgets('editing inspected input invalidates the preview and prior comparison decision', (tester) async {
    final enrollment = _Enrollment();
    await _pump(tester, enrollment);
    showEnrollmentPairing(_context(tester), EnrollmentPairingFlow.submit).ignore();
    await settle(tester);
    await _review(tester, _packet('endorsed'));
    await tapKey(tester, 'enrollment-code-confirmed');
    expect(isEnabled(tester, 'enrollment-continue'), true);
    final input = tester.widget<TextField>(find.byKey(const ValueKey('enrollment-packet'))).controller!;
    input.selection = const TextSelection.collapsed(offset: 3);
    await tester.pump();
    expect(isEnabled(tester, 'enrollment-continue'), true);
    await enterKey(tester, 'enrollment-packet', _packet('other'));
    expect(isEnabled(tester, 'enrollment-continue'), false);
    expect(find.byKey(const ValueKey('enrollment-full-code')), findsNothing);
    await tapKey(tester, 'enrollment-review');
    expect(tester.widget<CheckboxListTile>(find.byKey(const ValueKey('enrollment-code-confirmed'))).value, false);
    expect(enrollment.submissions, 0);
  });

  testWidgets('late review after text replacement cannot restore the old preview', (tester) async {
    final enrollment = _Enrollment()..reviewPending = Completer<EnrollmentPairing>();
    await _pump(tester, enrollment);
    showEnrollmentPairing(_context(tester), EnrollmentPairingFlow.endorse).ignore();
    await settle(tester);
    final oldPacket = _packet('target');
    await _review(tester, oldPacket);
    await enterKey(tester, 'enrollment-packet', _packet('replaced'));
    enrollment.reviewPending!.complete(_pairing(oldPacket));
    await settle(tester);
    expect(find.byKey(const ValueKey('enrollment-full-code')), findsNothing);
    expect(isEnabled(tester, 'enrollment-continue'), false);
    expect(enrollment.endorsements, 0);
  });

  testWidgets('quick lock and unlock invalidate an in-flight review of the same profile', (tester) async {
    final enrollment = _Enrollment()..reviewPending = Completer<EnrollmentPairing>();
    final backend = await _pump(tester, enrollment);
    showEnrollmentPairing(_context(tester), EnrollmentPairingFlow.endorse).ignore();
    await settle(tester);
    final packet = _packet('target');
    await _review(tester, packet);
    await backend.services.vault.lock();
    await backend.services.vault.unlockWithPassphrase(SecretText(MockCloud.demoPassphrase));
    enrollment.reviewPending!.complete(_pairing(packet));
    await settle(tester);
    expect(find.byType(EnrollmentPairingDialog), findsNothing);
    expect(enrollment.endorsements, 0);
    expect(tester.takeException(), isNull);
  });

  testWidgets('malformed, oversize, expired and target-less packets cannot acquire approval', (tester) async {
    final enrollment = _Enrollment();
    await _pump(tester, enrollment);
    showEnrollmentPairing(_context(tester), EnrollmentPairingFlow.submit).ignore();
    await settle(tester);
    await _review(tester, '[]');
    expect(enrollment.reviews, 0);
    await _review(tester, jsonEncode({'public_stage': List.filled(270000, 'ж').join()}));
    expect(enrollment.reviews, 0);
    enrollment.expired = true;
    await _review(tester, _packet('endorsed'));
    expect(isEnabled(tester, 'enrollment-continue'), false);
    enrollment.expired = false;
    enrollment.missingTarget = true;
    await _review(tester, _packet('endorsed'));
    expect(find.byKey(const ValueKey('enrollment-code-confirmed')), findsNothing);
    expect(isEnabled(tester, 'enrollment-continue'), false);
    expect(enrollment.submissions, 0);
  });

  testWidgets('package copy and save export only a verified bounded public JSON packet', (tester) async {
    final enrollment = _Enrollment();
    final directory = await _io(tester, () => Directory.systemTemp.createTemp('consolecrypt-enrollment-test-'));
    addTearDown(() => directory.delete(recursive: true));
    final files = _Files()..savePath = '${directory.path}/public-package.json';
    await _pump(tester, enrollment, files: files);
    String? clipboard;
    tester.binding.defaultBinaryMessenger.setMockMethodCallHandler(SystemChannels.platform, (call) async {
      if (call.method == 'Clipboard.setData') clipboard = (call.arguments as Map)['text'] as String?;
      return null;
    });
    addTearDown(() => tester.binding.defaultBinaryMessenger.setMockMethodCallHandler(SystemChannels.platform, null));
    final packet = _packet('source');
    showEnrollmentPackage(_context(tester), _pairing(packet)).ignore();
    await settle(tester);
    expect(enrollment.reviews, 1);
    await tapKey(tester, 'enrollment-package-copy');
    expect(clipboard, packet);
    await _tapFile(tester, 'enrollment-package-save');
    expect(await _io(tester, () => File(files.savePath!).readAsString()), packet);
    expect(files.finished, 1);
    expect(enrollment.preparations + enrollment.endorsements + enrollment.submissions, 0);
    expect(tester.takeException(), isNull);
  });

  testWidgets('file input is size-bounded and remains unapproved until reviewed and compared', (tester) async {
    final directory = await _io(tester, () => Directory.systemTemp.createTemp('consolecrypt-enrollment-test-'));
    addTearDown(() => directory.delete(recursive: true));
    final packet = _packet('target');
    final file = File('${directory.path}/input.json');
    await _io(tester, () => file.writeAsString(packet));
    final files = _Files()..openPath = file.path;
    final enrollment = _Enrollment();
    await _pump(tester, enrollment, files: files);
    showEnrollmentPairing(_context(tester), EnrollmentPairingFlow.endorse).ignore();
    await settle(tester);
    await _tapFile(tester, 'enrollment-open');
    expect(tester.widget<TextField>(find.byKey(const ValueKey('enrollment-packet'))).controller!.text, packet);
    expect(enrollment.reviews, 0);
    expect(isEnabled(tester, 'enrollment-continue'), false);
    await tapKey(tester, 'enrollment-review');
    await tapKey(tester, 'enrollment-code-confirmed');
    await _io(tester, () => file.writeAsBytes(List.filled(maxEnrollmentPacketBytes + 1, 32)));
    await _tapFile(tester, 'enrollment-open');
    // A rejected file leaves the original input unchanged and grants nothing.
    expect(tester.widget<TextField>(find.byKey(const ValueKey('enrollment-packet'))).controller!.text, packet);
    expect(enrollment.endorsements, 0);
    expect(tester.takeException(), isNull);
  });

  testWidgets('pending request responds to a challenge, reloads status and never manually accepts', (tester) async {
    final enrollment = _Enrollment();
    await _pump(tester, enrollment);
    showEnrollmentPending(_context(tester)).ignore();
    await settle(tester);
    expect(find.text('Awaiting device response'), findsOneWidget);
    await tapKey(tester, 'enrollment-respond-request');
    expect(enrollment.responses, 1);
    expect(enrollment.reloads, 2);
    expect(find.text('Device keys verified'), findsOneWidget);
    expect(enrollment.submissions + enrollment.endorsements, 0);
    expect(tester.takeException(), isNull);
  });

  testWidgets('expired and blocked pending requests cannot respond', (tester) async {
    final expired = _request(EnrollmentRequestState.expired);
    final enrollment = _Enrollment()..pending = [expired, _request(EnrollmentRequestState.blocked)];
    await _pump(tester, enrollment);
    showEnrollmentPending(_context(tester)).ignore();
    await settle(tester);
    expect(find.byKey(const ValueKey('enrollment-respond-request')), findsNothing);
    expect(enrollment.responses, 0);
  });

  testWidgets('phone can compare, confirm and submit without layout overflow', (tester) async {
    final enrollment = _Enrollment();
    await _pump(tester, enrollment, size: const Size(412, 915));
    showEnrollmentPairing(_context(tester), EnrollmentPairingFlow.submit).ignore();
    await settle(tester);
    await _review(tester, _packet('endorsed'));
    await tapKey(tester, 'enrollment-code-confirmed');
    await tapKey(tester, 'enrollment-continue');
    expect(enrollment.submissions, 1);
    expect(tester.takeException(), isNull);
  }, variant: const TargetPlatformVariant({TargetPlatform.iOS, TargetPlatform.android}));
}
