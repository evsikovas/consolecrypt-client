import 'dart:async';
import 'dart:convert';

import 'package:consolecrypt/automation/automation_portable_import_controller.dart';
import 'package:consolecrypt/automation/automation_portable_import_session.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:flutter_test/flutter_test.dart';

import '../helpers/portable_import_fixture.dart' as f;

void main() {
  test('open/edit/mount data does not choose file, register, preview, accept or install', () async {
    final x = f.Fixture();
    addTearDown(x.dispose);
    x.controller.setRequestedName('Fresh import');
    expect(x.files.opens, 0);
    expect(x.gateway.calls, isEmpty);
    expect(x.controller.installed, isNull);
  });
  test('missing optional Native capability is unavailable with no picker or fake success', () async {
    final x = f.Fixture(available: false);
    addTearDown(x.dispose);
    await x.controller.chooseSource();
    final password = SecretText('synthetic');
    await x.controller.prepare(password);
    expect(password.isWiped, isTrue);
    expect(x.files.opens, 0);
    expect(x.gateway.calls, isEmpty);
    expect(x.controller.issue, PortableImportIssue.unavailable);
    expect(x.controller.installed, isNull);
  });
  test('explicit picker reads only trusted file dialog; cancelled picker creates no Native session', () async {
    final x = f.Fixture();
    addTearDown(x.dispose);
    x.files.selection = null;
    await x.select();
    expect(x.files.opens, 1);
    expect(x.controller.selectedSource, isEmpty);
    expect(x.gateway.calls, isEmpty);
  });
  test('late picker after original hub boundary never adopts reopened scope or starts prepare', () async {
    final x = f.Fixture();
    addTearDown(x.dispose);
    x.files.pending = Completer<String?>();
    x.controller.setRequestedName('Fresh import');
    final picker = x.controller.chooseSource();
    x.events.boundary();
    x.files.pending!.complete('/owned/selected.ccproject');
    await picker;
    expect(x.controller.current, isFalse);
    expect(x.controller.selectedSource, isEmpty);
    expect(x.gateway.calls, isEmpty);
  });
  test('input edit while picker pending denies late selected file without another request', () async {
    final x = f.Fixture();
    addTearDown(x.dispose);
    x.files.pending = Completer<String?>();
    x.controller.setRequestedName('Old');
    final picker = x.controller.chooseSource();
    x.controller.setRequestedName('New');
    x.files.pending!.complete('/owned/selected.ccproject');
    await picker;
    expect(x.controller.selectedSource, isEmpty);
    expect(x.gateway.calls, isEmpty);
  });
  test('Ready completes BEFORE real joined, retains session and cannot claim imported project', () async {
    final x = f.Fixture();
    addTearDown(x.dispose);
    final op = await x.prepareReady();
    expect(x.controller.phase, PortableImportPhase.ready);
    expect(op.retired.isCompleted, isFalse);
    expect(op.session.previews, 1);
    expect(op.session.accepts, 0);
    expect(x.controller.installed, isNull);
    expect(x.controller.canAccept, isTrue);
  });
  test('password copied synchronously and original wiped while registration remains pending', () async {
    final x = f.Fixture();
    addTearDown(x.dispose);
    await x.select();
    final password = SecretText('synthetic');
    final work = x.controller.prepare(password);
    final op = x.gateway.calls.single;
    expect(password.isWiped, isTrue);
    expect(op.password.isWiped, isFalse);
    x.controller.cancel();
    expect(op.password.isWiped, isTrue);
    op.acknowledge();
    op.retire();
    await work;
    expect(op.session.previews, 0);
    expect(op.session.accepts, 0);
  });
  test('reentrant original owner loss before register returns still cancels late handle before preview', () async {
    final x = f.Fixture();
    addTearDown(x.dispose);
    await x.select();
    x.gateway.beforeReturn = () {
      x.original = false;
      x.lifetime.retire();
    };
    final work = x.controller.prepare(SecretText('synthetic'));
    final op = x.gateway.calls.single;
    expect(op.cancels, greaterThanOrEqualTo(1));
    op.acknowledge();
    op.retire();
    await work;
    expect(op.session.previews, 0);
    expect(x.controller.ready, isNull);
  });
  test('raw name edit even trim-equivalent retires Ready; no automatic reprepare or acceptance', () async {
    final x = f.Fixture();
    addTearDown(x.dispose);
    final op = await x.prepareReady();
    x.controller.setRequestedName('Fresh import ');
    expect(x.controller.ready, isNull);
    expect(x.controller.canAccept, isFalse);
    expect(op.cancels, 1);
    expect(x.gateway.calls.length, 1);
    x.controller.setRequestedName('Fresh import  ');
    op.retire();
    await f.flush();
    expect(x.controller.phase, PortableImportPhase.idle);
    expect(op.session.accepts, 0);
    expect(x.gateway.calls.length, 1);
  });
  test('password/source edit invalidates Ready and preserves source/native ownership boundaries', () async {
    final x = f.Fixture();
    addTearDown(x.dispose);
    final op = await x.prepareReady();
    x.controller.passwordEdited();
    expect(x.controller.ready, isNull);
    expect(op.cancels, 1);
    op.retire();
    await f.flush();
    x.controller.clearSource();
    expect(x.controller.canPrepare, isFalse);
    expect(x.gateway.calls.length, 1);
  });
  test('actual Native Ready retirement expires UI; remount/read does not renew hint or reenable accept', () async {
    final x = f.Fixture();
    addTearDown(x.dispose);
    final op = await x.prepareReady();
    op.retire();
    await f.flush();
    expect(x.controller.phase, PortableImportPhase.expired);
    expect(x.controller.canAccept, isFalse);
    expect(x.controller.ready, isNull);
    await x.controller.accept();
    expect(op.session.accepts, 0);
    expect(x.gateway.calls.length, 1);
  });
  test('unknown capability reason or foreign requestedName/scope cannot expose Ready', () async {
    for (final kind in ['reason', 'echo', 'scope', 'version']) {
      final x = f.Fixture();
      addTearDown(x.dispose);
      await x.select();
      final work = x.controller.prepare(SecretText('synthetic'));
      final op = x.gateway.calls.single;
      op.acknowledge();
      await f.flush();
      final json = f.readyJson(op.intent);
      if (kind == 'reason') {
        json['publicationSupported'] = false;
        json['publicationReason'] = 'future_policy';
      }
      if (kind == 'echo') json['requestedName'] = 'Different input';
      if (kind == 'scope') json['scope'] = {...f.scope().toJson(), 'notificationScopeId': f.projectId};
      if (kind == 'version') json['version'] = 2;
      op.session.previewResult.complete(jsonEncode(json));
      await f.flush();
      expect(op.cancels, greaterThanOrEqualTo(1));
      op.retire();
      await work;
      expect(x.controller.ready, isNull);
      expect(x.controller.installed, isNull);
      expect(op.session.accepts, 0);
    }
  });
  test('unsupported preview is informational only and cannot call Native accept', () async {
    final x = f.Fixture();
    addTearDown(x.dispose);
    await x.select();
    final work = x.controller.prepare(SecretText('synthetic'));
    final op = x.gateway.calls.single;
    op.acknowledge();
    await f.flush();
    final json = f.readyJson(op.intent)
      ..['publicationSupported'] = false
      ..['publicationReason'] = 'publication_platform_unsupported';
    op.session.previewResult.complete(jsonEncode(json));
    await work;
    expect(x.controller.ready, isNotNull);
    expect(x.controller.canAccept, isFalse);
    await x.controller.accept();
    expect(op.session.accepts, 0);
    expect(x.controller.installed, isNull);
  });
  test('accept sends only captured review refs once and waits actual joined before installed', () async {
    final x = f.Fixture();
    addTearDown(x.dispose);
    final op = await x.prepareReady();
    final reviewed = x.controller.ready!;
    final accept = x.controller.accept();
    await x.controller.accept();
    expect(op.session.accepts, 1);
    expect(op.session.acceptedRefs.single, (id: reviewed.reviewId, digest: reviewed.reviewDigest));
    op.session.acceptResult.complete(jsonEncode(f.installedJson(reviewed)));
    await f.flush();
    expect(x.controller.installed, isNull);
    expect(x.controller.phase, PortableImportPhase.accepting);
    op.retire();
    await accept;
    expect(x.controller.phase, PortableImportPhase.installed);
    expect(x.controller.installed!.projectId, reviewed.projectId);
    expect(x.controller.canPrepare, isFalse);
    expect(x.controller.canAccept, isFalse);
  });
  test('real join before accept reply delivery preserves genuine same-session installed result', () async {
    final x = f.Fixture();
    addTearDown(x.dispose);
    final op = await x.prepareReady();
    final reviewed = x.controller.ready!;
    final work = x.controller.accept();
    op.retire();
    await f.flush();
    expect(x.controller.phase, PortableImportPhase.accepting);
    op.session.acceptResult.complete(jsonEncode(f.installedJson(reviewed)));
    await work;
    expect(x.controller.phase, PortableImportPhase.installed);
  });
  test(
    'uncertain accept remains sticky RecoveryRequired and never automatically retries even after input edit',
    () async {
      final x = f.Fixture();
      addTearDown(x.dispose);
      final op = await x.prepareReady();
      final work = x.controller.accept();
      op.session.acceptResult.completeError(StateError('SYNTHETIC_PRIVATE'));
      await f.flush();
      expect(op.cancels, greaterThanOrEqualTo(1));
      op.retire();
      await work;
      expect(x.controller.phase, PortableImportPhase.recoveryRequired);
      expect(x.controller.attemptedReview, isNotNull);
      x.controller.setRequestedName('Another');
      await x.controller.prepare(SecretText('synthetic'));
      await x.controller.chooseSource();
      await x.controller.accept();
      expect(x.gateway.calls.length, 1);
      expect(op.session.accepts, 1);
      expect(x.controller.installed, isNull);
    },
  );
  test('known pre-durable expiry denies old accept, closes session and never reports import success', () async {
    final x = f.Fixture();
    addTearDown(x.dispose);
    final op = await x.prepareReady();
    final work = x.controller.accept();
    op.session.acceptResult.completeError(const PortableImportFailure(PortableImportIssue.expired));
    op.retire();
    await work;
    expect(x.controller.phase, PortableImportPhase.expired);
    expect(x.controller.installed, isNull);
    expect(x.controller.canAccept, isFalse);
  });
  test('malformed or retargeted installed reply after accept requires recovery rather than false success', () async {
    for (final key in ['projectId', 'name', 'revisionDigest', 'projectRevision', 'encryptedSha256']) {
      final x = f.Fixture();
      addTearDown(x.dispose);
      final op = await x.prepareReady();
      final reviewed = x.controller.ready!;
      final work = x.controller.accept();
      final json = f.installedJson(reviewed);
      json[key] = key == 'projectRevision' ? 4 : 'WRONG';
      op.session.acceptResult.complete(jsonEncode(json));
      op.retire();
      await work;
      expect(x.controller.phase, PortableImportPhase.recoveryRequired);
      expect(x.controller.installed, isNull);
    }
  });
  test('cancel during accept cannot restore late success or promise rollback', () async {
    final x = f.Fixture();
    addTearDown(x.dispose);
    final op = await x.prepareReady();
    final reviewed = x.controller.ready!;
    final work = x.controller.accept();
    x.controller.cancel();
    op.session.acceptResult.complete(jsonEncode(f.installedJson(reviewed)));
    op.retire();
    await work;
    expect(x.controller.phase, PortableImportPhase.recoveryRequired);
    expect(x.controller.installed, isNull);
  });
  test('old preview failure cannot cancel or overwrite a newer Ready session', () async {
    final x = f.Fixture();
    addTearDown(x.dispose);
    await x.select();
    final oldWork = x.controller.prepare(SecretText('synthetic'));
    final old = x.gateway.calls.single;
    old.acknowledge();
    await f.flush();
    x.controller.setRequestedName('New');
    old.retire();
    await f.flush();
    final newWork = x.controller.prepare(SecretText('synthetic'));
    final current = x.gateway.calls.last;
    current.acknowledge();
    await f.flush();
    current.session.previewResult.complete(jsonEncode(f.readyJson(current.intent, name: 'New')));
    await newWork;
    old.session.previewResult.completeError(StateError('SYNTHETIC_PRIVATE'));
    await oldWork;
    expect(current.cancels, 0);
    expect(x.controller.phase, PortableImportPhase.ready);
    expect(x.controller.ready!.name, 'New');
  });
  test('failed real retirement is not fabricated as join or permission to start another session', () async {
    final x = f.Fixture();
    addTearDown(x.dispose);
    final op = await x.prepareReady();
    x.controller.setRequestedName('New');
    op.retired.completeError(StateError('SYNTHETIC_PRIVATE'));
    await f.flush();
    await x.controller.prepare(SecretText('synthetic'));
    expect(x.gateway.calls.length, 1);
    expect(x.controller.canPrepare, isFalse);
  });
  test(
    'generic typed unavailable/cancel/input error after Accept does not masquerade as known pre-durable refusal',
    () async {
      for (final issue in [
        PortableImportIssue.unavailable,
        PortableImportIssue.cancelled,
        PortableImportIssue.invalidInput,
      ]) {
        final x = f.Fixture();
        addTearDown(x.dispose);
        final op = await x.prepareReady();
        final work = x.controller.accept();
        op.session.acceptResult.completeError(PortableImportFailure(issue));
        op.retire();
        await work;
        expect(x.controller.phase, PortableImportPhase.recoveryRequired);
        expect(x.controller.installed, isNull);
      }
    },
  );
  test('input changes cannot start another registration while original Ready cleanup is still pending', () async {
    final x = f.Fixture();
    addTearDown(x.dispose);
    final old = await x.prepareReady();
    final picker = x.controller.chooseSource();
    x.controller.setRequestedName('New');
    await x.controller.prepare(SecretText('synthetic'));
    expect(x.gateway.calls.length, 1);
    old.retire();
    await picker;
    await f.flush();
    expect(x.gateway.calls.length, 1);
    expect(x.controller.phase, PortableImportPhase.idle);
  });

  test('actual expiry before late registration ACK prevents preview itself', () async {
    final x = f.Fixture();
    addTearDown(x.dispose);
    await x.select();
    final work = x.controller.prepare(SecretText('synthetic'));
    final op = x.gateway.calls.single;
    op.retire();
    await f.flush();
    op.acknowledge();
    await work;
    expect(op.session.previews, 0);
    expect(op.session.accepts, 0);
    expect(x.controller.phase, PortableImportPhase.expired);
  });
}
