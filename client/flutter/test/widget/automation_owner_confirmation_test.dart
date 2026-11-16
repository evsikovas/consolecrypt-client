import 'dart:async';

import 'package:consolecrypt/app/theme.dart';
import 'package:consolecrypt/automation/automation_owner_confirmation.dart';
import 'package:consolecrypt/automation/automation_owner_confirmation_dialog.dart';
import 'package:consolecrypt/automation/automation_service.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/step_up.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

const reviewId = '01010101-0101-4101-8101-010101010101';
const jobId = '02020202-0202-4202-8202-020202020202';
final observed = DateTime.utc(2026, 11, 13, 10);
AutomationJson review() {
  final now = observed.millisecondsSinceEpoch ~/ 1000;
  return {'review_id': reviewId, 'policy': {'production': true}, 'expires_at': now + 120,
    'owner_confirmation': {'policy': 'ownerConfirmationVaultPassphraseV1', 'kind': 'ownerConfirmation',
      'required': true, 'manifest_digest': 'ab' * 32, 'job_id': jobId, 'issued_at': now, 'expires_at': now + 60}};
}

class OwnerService implements AutomationProductionOwnerService {
  final ids = <String>[];
  final proofs = <StepUpProof>[];
  Future<AutomationJson> Function()? response;
  @override
  Future<AutomationJson> confirmOwnerRun(String id, {required StepUpProof proof}) async {
    ids.add(id); proofs.add(proof);
    return response?.call() ?? {'job_id': jobId};
  }
}

Future<void> open(WidgetTester tester, OwnerService service, {
  bool Function()? original, DateTime Function()? utcNow,
}) async {
  tester.view.physicalSize = const Size(700, 850);
  tester.view.devicePixelRatio = 1;
  addTearDown(tester.view.reset);
  await tester.pumpWidget(MaterialApp(
    theme: AppTheme.dark(platform: TargetPlatform.windows),
    localizationsDelegates: const [AppLocalizations.delegate, ...GlobalMaterialLocalizations.delegates],
    supportedLocales: AppLocalizations.supportedLocales,
    home: Builder(builder: (context) => Scaffold(body: TextButton(
      onPressed: () async {
        final result = await showDialog<AutomationOwnerConfirmationResult>(context: context,
          builder: (_) => AutomationOwnerConfirmationDialog(service: service,
            challenge: AutomationOwnerConfirmation.fromReview(review())!,
            original: original ?? () => true, utcNow: utcNow ?? () => observed));
        if (context.mounted && result != null) {
          ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text(result.job != null ? 'JOB_ACCEPTED' : 'JOB_FAILED')));
        }
      }, child: const Text('OPEN_OWNER')))),
  ));
  await tester.tap(find.text('OPEN_OWNER'));
  await tester.pumpAndSettle();
}

Future<void> enter(WidgetTester tester) async {
  await tester.enterText(find.descendant(of: find.byKey(const ValueKey('automation-owner-passphrase')),
    matching: find.byType(EditableText)), 'synthetic-vault-passphrase');
  await tester.pump();
}

void main() {
  test('closed owner challenge retains exact original IDs/digest/expiry', () {
    final challenge = AutomationOwnerConfirmation.fromReview(review())!;
    expect(challenge.reviewId, reviewId);
    expect(challenge.jobId, jobId);
    expect(challenge.manifestDigest, 'ab' * 32);
    expect(challenge.expired(observed), isFalse);
    expect(challenge.expired(observed.add(const Duration(seconds: 60))), isTrue);
  });
  test('missing capability metadata never becomes normal production confirmation', () {
    for (final mutate in <void Function(AutomationJson)>[
      (r) => r.remove('owner_confirmation'),
      (r) => (r['policy'] as Map)['production'] = false,
      (r) => (r['owner_confirmation'] as Map)['required'] = false,
      (r) => (r['owner_confirmation'] as Map)['kind'] = 'secondReviewer',
      (r) => (r['owner_confirmation'] as Map)['policy'] = 'futurePolicy',
      (r) => (r['owner_confirmation'] as Map)['future'] = true,
      (r) => (r['owner_confirmation'] as Map)['issued_at'] = 1.0,
      (r) => (r['owner_confirmation'] as Map)['manifest_digest'] = 'AB' * 32,
      (r) => (r['owner_confirmation'] as Map)['job_id'] = '00000000-0000-0000-0000-000000000000',
      (r) => (r['owner_confirmation'] as Map)['expires_at'] = (r['expires_at'] as int) + 1,
      (r) => r['review_id'] = 'another-review',
    ]) {
      final value = review(); mutate(value);
      expect(AutomationOwnerConfirmation.fromReview(value), isNull);
    }
  });
  testWidgets('cancel never calls Native, and UI offers no OS proof', (tester) async {
    final service = OwnerService(); await open(tester, service); await enter(tester);
    expect(find.byKey(const ValueKey('step-up-presence')), findsNothing);
    await tester.tap(find.byKey(const ValueKey('automation-owner-cancel'))); await tester.pumpAndSettle();
    expect(service.ids, isEmpty);
    expect(find.byKey(const ValueKey('automation-owner-confirmation-dialog')), findsNothing);
  });
  testWidgets('explicit action submits only opaque review ID and wipes proof', (tester) async {
    final service = OwnerService(); await open(tester, service); await enter(tester);
    await tester.tap(find.byKey(const ValueKey('automation-owner-confirm'))); await tester.pumpAndSettle();
    expect(service.ids, [reviewId]);
    expect((service.proofs.single as PassphraseProof).passphrase.isWiped, isTrue);
    expect(find.text('JOB_ACCEPTED'), findsOneWidget);
    expect(find.text('synthetic-vault-passphrase'), findsNothing);
    expect(tester.takeException(), isNull);
  });
  testWidgets('wrong passphrase retries SAME review without remint or expiry extension', (tester) async {
    final service = OwnerService();
    service.response = () async {
      if (service.ids.length == 1) throw const AppException(AppErrorCode.wrongPassphrase, 'synthetic');
      return {'job_id': jobId};
    };
    await open(tester, service); await enter(tester);
    await tester.tap(find.byKey(const ValueKey('automation-owner-confirm'))); await tester.pumpAndSettle();
    expect(find.text('Wrong vault passphrase.'), findsOneWidget);
    expect((service.proofs.first as PassphraseProof).passphrase.isWiped, isTrue);
    await enter(tester); await tester.tap(find.byKey(const ValueKey('automation-owner-confirm'))); await tester.pumpAndSettle();
    expect(service.ids, [reviewId, reviewId]);
    expect(find.text('JOB_ACCEPTED'), findsOneWidget);
  });
  testWidgets('owner retirement while awaiting Native ignores late reply and wipes proof', (tester) async {
    var original = true; final pending = Completer<AutomationJson>(); final service = OwnerService()..response = () => pending.future;
    await open(tester, service, original: () => original); await enter(tester);
    await tester.tap(find.byKey(const ValueKey('automation-owner-confirm'))); await tester.pump();
    original = false; await tester.pump(const Duration(seconds: 1)); await tester.pumpAndSettle();
    pending.complete({'job_id': jobId}); await tester.pumpAndSettle();
    expect(service.ids, [reviewId]);
    expect((service.proofs.single as PassphraseProof).passphrase.isWiped, isTrue);
    expect(find.text('JOB_ACCEPTED'), findsNothing);
    expect(find.byKey(const ValueKey('automation-owner-confirmation-dialog')), findsNothing);
    expect(tester.takeException(), isNull);
  });
  testWidgets('original expiry blocks dispatch even after passphrase entry', (tester) async {
    var now = observed; final service = OwnerService();
    await open(tester, service, utcNow: () => now); await enter(tester);
    now = observed.add(const Duration(seconds: 61));
    await tester.tap(find.byKey(const ValueKey('automation-owner-confirm'))); await tester.pumpAndSettle();
    expect(service.ids, isEmpty);
    expect(find.byKey(const ValueKey('automation-owner-confirmation-dialog')), findsNothing);
  });
  testWidgets('late completion closes its own route and preserves another dialog above it', (tester) async {
    final pending = Completer<AutomationJson>(); final service = OwnerService()..response = () => pending.future;
    await open(tester, service); await enter(tester);
    await tester.tap(find.byKey(const ValueKey('automation-owner-confirm'))); await tester.pump();
    final context = tester.element(find.byKey(const ValueKey('automation-owner-confirmation-dialog')));
    unawaited(showDialog<void>(context: context, builder: (_) => const AlertDialog(
      key: ValueKey('unrelated-owner-dialog'), content: Text('UNRELATED_DIALOG'))));
    // Native is deliberately still pending; its progress indicator animates.
    await tester.pump(const Duration(milliseconds: 400));
    pending.complete({'job_id': jobId}); await tester.pumpAndSettle();
    expect(find.byKey(const ValueKey('unrelated-owner-dialog')), findsOneWidget);
    expect(find.byKey(const ValueKey('automation-owner-confirmation-dialog')), findsNothing);
    expect((service.proofs.single as PassphraseProof).passphrase.isWiped, isTrue);
    expect(tester.takeException(), isNull);
  });
  testWidgets('other Native errors retire instead of silently retrying or normal-confirm fallback', (tester) async {
    final service = OwnerService()..response = () async => throw const AppException(AppErrorCode.conflict, 'synthetic');
    await open(tester, service); await enter(tester);
    await tester.tap(find.byKey(const ValueKey('automation-owner-confirm'))); await tester.pumpAndSettle();
    expect(service.ids, [reviewId]);
    expect(find.text('JOB_FAILED'), findsOneWidget);
    expect((service.proofs.single as PassphraseProof).passphrase.isWiped, isTrue);
  });
}
