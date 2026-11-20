// Synthetic interface fixture only; not Native crypto/SQL/publication evidence.
import 'dart:async';
import 'dart:convert';

import 'package:consolecrypt/automation/automation_android_archive_picker.dart';
import 'package:consolecrypt/automation/automation_portable_import_controller.dart';
import 'package:consolecrypt/automation/automation_portable_import_session.dart';
import 'package:consolecrypt/automation/automation_portable_lifetime.dart';
import 'package:consolecrypt/automation/automation_portable_package.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/services/file_dialog_service.dart';
import 'package:consolecrypt/core/services/runtime_notice_adapter.dart';

const profile = '11111111-1111-4111-8111-111111111111', nonce = '22222222-2222-4222-8222-222222222222';
const reviewId = '33333333-3333-4333-8333-333333333333', projectId = '44444444-4444-4444-8444-444444444444';
PortableScope scope() => PortableScope.fromJson({'version': 1, 'profileId': profile, 'notificationScopeId': nonce});
Future<void> flush() => Future<void>.delayed(Duration.zero);
Map<String, dynamic> readyJson(PortableImportIntent intent, {String review = reviewId, String name = 'Fresh import'}) =>
    {
      'version': 1,
      'scope': intent.scope.toJson(),
      'reviewId': review,
      'reviewDigest': 'a' * 64,
      'projectId': projectId,
      'name': name,
      'requestedName': intent.requestedName,
      'encryptedSha256': 'b' * 64,
      'summary': {'sourceDigest': 'c' * 64, 'portableDigest': 'd' * 64, 'files': 2, 'bytes': 120, 'exclusions': 1},
      'publicationSupported': true,
      'publicationReason': null,
      'remainingMillis': 300000,
    };
Map<String, dynamic> installedJson(PortableImportReady ready) => {
  'version': 1,
  'state': 'installed',
  'scope': ready.scope.toJson(),
  'reviewId': ready.reviewId,
  'projectId': ready.projectId,
  'name': ready.name,
  'projectRevision': 3,
  'revisionDigest': ready.summary.portableDigest,
  'encryptedSha256': ready.encryptedSha256,
};

class Files implements FileDialogService {
  int opens = 0;
  Completer<String?>? pending;
  String? selection = '/owned/selected.ccproject';
  @override
  Future<String?> chooseOpenFile({List<String> extensions = const []}) async {
    opens++;
    return pending?.future ?? selection;
  }

  @override
  Future<String?> chooseSaveFile({required String suggestedName, List<String> extensions = const []}) async =>
      throw StateError('no export in import review');
  @override
  Future<bool> finishSaveFile(String path) async =>
      throw StateError('no imported publication acknowledgement from file picker');
  @override
  Future<String?> chooseDirectory() async => throw StateError('no caller target directory');
  @override
  Future<String?> chooseApplication({required String label, required String confirmButtonText}) async =>
      throw StateError('no execution');
}

class Session implements PortableImportReviewSession {
  int previews = 0, accepts = 0, cancels = 0;
  final previewResult = Completer<String>(), acceptResult = Completer<String>();
  final acceptedRefs = <({String id, String digest})>[];
  @override
  Future<String> preview() {
    previews++;
    return previewResult.future;
  }

  @override
  Future<String> accept({required String reviewId, required String reviewDigest}) {
    accepts++;
    acceptedRefs.add((id: reviewId, digest: reviewDigest));
    return acceptResult.future;
  }

  @override
  void cancel() {
    cancels++;
  }
}

class Registration implements PortableImportRegistration {
  Registration(this.intent, SecretText input) {
    final bytes = input.exposeBytes();
    try {
      password = SecretText.fromBytes(bytes);
    } finally {
      bytes.fillRange(0, bytes.length, 0);
    }
  }
  final PortableImportIntent intent;
  late final SecretText password;
  final session = Session(), ack = Completer<PortableImportReviewSession>(), retired = Completer<void>();
  int cancels = 0;
  @override
  Future<PortableImportReviewSession> get registered => ack.future;
  @override
  Future<void> get joined => retired.future;
  @override
  void cancel() {
    cancels++;
    password.wipe();
    session.cancel();
  }

  void acknowledge() {
    password.wipe();
    ack.complete(session);
  }

  void retire() {
    if (!retired.isCompleted) retired.complete();
  }

  void cleanup() {
    cancel();
    if (!ack.isCompleted) ack.complete(session);
    if (session.previews != 0 && !session.previewResult.isCompleted) {
      session.previewResult.completeError(const PortableImportFailure(PortableImportIssue.cancelled));
    }
    if (session.accepts != 0 && !session.acceptResult.isCompleted) {
      session.acceptResult.completeError(const PortableImportFailure(PortableImportIssue.cancelled));
    }
    retire();
  }
}

class Gateway implements PortableImportReviewGateway {
  final calls = <Registration>[];
  void Function()? beforeReturn;
  @override
  PortableImportRegistration register({required PortableImportIntent intent, required SecretText password}) {
    final call = Registration(intent, password);
    calls.add(call);
    beforeReturn?.call();
    return call;
  }
}

class Fixture {
  Fixture({bool available = true, Future<AutomationArchiveSelection?> Function()? pickArchive}) {
    lifetime = PortableUiLifetime(profileId: profile, events: events, isOriginalUiCurrent: () => original);
    controller = AutomationPortableImportController(
      scope: scope(),
      gateway: available ? gateway : null,
      files: files,
      pickArchive: pickArchive,
      lifetime: lifetime,
      isOriginalCurrent: () => original,
    );
  }
  final events = RuntimeNoticeEventAdapter(), gateway = Gateway(), files = Files();
  late final PortableUiLifetime lifetime;
  late final AutomationPortableImportController controller;
  bool original = true;
  Future<void> select({String name = 'Fresh import'}) async {
    controller.setRequestedName(name);
    await controller.chooseSource();
  }

  Future<Registration> prepareReady() async {
    await select();
    final work = controller.prepare(SecretText('synthetic package password'));
    final op = gateway.calls.last;
    op.acknowledge();
    await flush();
    op.session.previewResult.complete(jsonEncode(readyJson(op.intent)));
    await work;
    return op;
  }

  Future<void> dispose() async {
    controller.dispose();
    for (final op in gateway.calls) {
      op.cleanup();
    }
    await flush();
    lifetime.dispose();
    await events.dispose();
  }
}
