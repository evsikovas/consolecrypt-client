import 'dart:async';

import 'package:consolecrypt/automation/automation_portable_controller.dart';
import 'package:consolecrypt/automation/automation_portable_package.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/services/file_dialog_service.dart';
import 'package:flutter_test/flutter_test.dart';

const profile = '11111111-1111-4111-8111-111111111111',
    nonce = '22222222-2222-4222-8222-222222222222',
    project = '33333333-3333-4333-8333-333333333333';
PortableScope scope() => PortableScope.fromJson({'version': 1, 'profileId': profile, 'notificationScopeId': nonce});
Map<String, dynamic> contextJson() => {
  'scope': scope().toJson(),
  'projectId': project,
  'projectRevision': 1,
  'revisionDigest': 'a' * 64,
  'projectStamp': 'b' * 64,
  'choices': [
    {'path': 'play.yml', 'bytes': 24, 'directory': false, 'selectable': true},
    {'path': '.env', 'bytes': 44, 'directory': false, 'selectable': false},
  ],
};
PortableContext context() => PortableContext.fromJson(contextJson());
PortableExportOutcome outcome() =>
    PortableExportOutcome.fromJson({'version': 1, 'encryptedSha256': 'c' * 64, 'bytes': 1000});
PortableImportPreview preview() => PortableImportPreview.fromJson({
  'version': 1,
  'encryptedSha256': 'c' * 64,
  'summary': {'sourceDigest': 'a' * 64, 'portableDigest': 'd' * 64, 'files': 1, 'bytes': 24, 'exclusions': 1},
  'publicationSupported': false,
  'reason': 'managed_publication_required',
});

// UI service fixtures only: no cloned RustApi, crypto, filesystem or broker.
class Service implements AutomationPortablePackageService {
  int contexts = 0, exports = 0, inspects = 0;
  bool nativeCancel = false;
  Map<String, dynamic>? request;
  String? destination;
  Completer<PortableContext>? contextWait;
  Completer<PortableExportOutcome>? exportWait;
  Completer<PortableImportPreview>? inspectWait;
  Completer<void>? contextEntered, inspectEntered;
  @override
  Future<PortableContext> context(PortableScope scope, String id, int revision, PortableCancellation cancel) async {
    contexts++;
    cancel.bind(() => nativeCancel = true);
    contextEntered?.complete();
    return contextWait == null ? PortableContext.fromJson(contextJson()) : await contextWait!.future;
  }

  @override
  Future<PortableExportOutcome> export(
    Map<String, dynamic> value,
    SecretText password,
    String path,
    PortableCancellation cancel,
  ) async {
    exports++;
    expect(password.expose(), 'synthetic package password');
    request = value;
    destination = path;
    cancel.bind(() => nativeCancel = true);
    return exportWait == null ? outcome() : await exportWait!.future;
  }

  @override
  Future<PortableImportPreview> inspect(
    PortableScope scope,
    String path,
    SecretText password,
    PortableCancellation cancel,
  ) async {
    inspects++;
    expect(password.expose(), 'synthetic package password');
    cancel.bind(() => nativeCancel = true);
    inspectEntered?.complete();
    return inspectWait == null ? preview() : await inspectWait!.future;
  }
}

class Files implements FileDialogService {
  int saves = 0, opens = 0, finishes = 0;
  Completer<String?>? saveWait;
  Completer<bool>? finishWait;
  Completer<void>? finishEntered;
  String? savePath = '/owned/new.ccproject', openPath = '/owned/input.ccproject';
  @override
  Future<String?> chooseSaveFile({required String suggestedName, List<String> extensions = const []}) async {
    saves++;
    expect(extensions, ['ccproject']);
    return saveWait == null ? savePath : await saveWait!.future;
  }

  @override
  Future<String?> chooseOpenFile({List<String> extensions = const []}) async {
    opens++;
    return openPath;
  }

  @override
  Future<bool> finishSaveFile(String path) async {
    finishes++;
    finishEntered?.complete();
    return finishWait == null ? true : await finishWait!.future;
  }

  @override
  Future<String?> chooseDirectory() async => throw StateError('no arbitrary import target');
  @override
  Future<String?> chooseApplication({required String label, required String confirmButtonText}) async =>
      throw StateError('no executable');
}

AutomationPortableController controller(Service service, Files files, {bool Function()? current}) =>
    AutomationPortableController(
      scope: scope(),
      service: service,
      files: files,
      isOriginalCurrent: current ?? () => true,
      projectId: project,
      projectRevision: 1,
    );
void main() {
  test('strict typedNative metadata rejects authority fields and future schema', () {
    final value = contextJson();
    value['url'] = 'https://not-a-portable-authority.invalid';
    expect(() => PortableContext.fromJson(value), throwsA(isA<PortablePackageFailure>()));
    final raw = scope().toJson();
    raw['version'] = 2;
    expect(() => PortableScope.fromJson(raw), throwsA(isA<PortablePackageFailure>()));
    final c = context();
    expect(() => c.selection({'.env'}), throwsA(isA<PortablePackageFailure>()));
  });
  test('load never auto selects; explicit selection uses exactNative original metadata', () async {
    final service = Service(), files = Files(), c = controller(service, files);
    await c.load();
    expect(c.selected, isEmpty);
    expect(service.exports, 0);
    c.select('.env', true);
    expect(c.selected, isEmpty);
    c.select('play.yml', true);
    final password = SecretText('synthetic package password'), confirmation = SecretText('synthetic package password');
    await c.export(password, confirmation);
    expect(service.exports, 1);
    expect(service.request, context().selection({'play.yml'}));
    expect(service.destination, files.savePath);
    expect(c.phase, PortablePhase.exported);
    expect(password.isWiped && confirmation.isWiped, isTrue);
    expect(files.finishes, 1);
    c.dispose();
  });
  test('mismatch andUTF8 quota wipe buffers before anyfilepicker orNative call', () async {
    final service = Service(), files = Files(), c = controller(service, files);
    await c.load();
    c.select('play.yml', true);
    final password = SecretText('one'), confirmation = SecretText('two');
    await c.export(password, confirmation);
    expect(password.isWiped && confirmation.isWiped, isTrue);
    final long = SecretText('я' * 2049), same = SecretText('я' * 2049);
    await c.export(long, same);
    expect(long.isWiped && same.isWiped, isTrue);
    expect(files.saves, 0);
    expect(service.exports, 0);
    c.dispose();
  });
  test('cancelled ownfilepicker cannot issueNative export orcompletion', () async {
    final service = Service(), files = Files()..saveWait = Completer<String?>(), c = controller(service, files);
    await c.load();
    c.select('play.yml', true);
    final password = SecretText('synthetic package password'), confirmation = SecretText('synthetic package password');
    final work = c.export(password, confirmation);
    c.cancel();
    expect(password.isWiped && confirmation.isWiped, isTrue);
    files.saveWait!.complete('/owned/new.ccproject');
    await work;
    expect(service.exports, 0);
    expect(files.finishes, 0);
    expect(c.phase, PortablePhase.cancelled);
    c.dispose();
  });
  test('latecontext never adopts replacement original scope', () async {
    var current = true;
    final service = Service()..contextWait = Completer<PortableContext>(),
        files = Files(),
        c = controller(service, files, current: () => current);
    final load = c.load();
    current = false;
    service.contextWait!.complete(context());
    await load;
    expect(c.review, isNull);
    expect(service.exports, 0);
    c.dispose();
  });
  test('cancelafteractualservice entry forwardsown signal and ignoreslate completion', () async {
    final service = Service()..exportWait = Completer<PortableExportOutcome>(),
        files = Files(),
        c = controller(service, files);
    await c.load();
    c.select('play.yml', true);
    final password = SecretText('synthetic package password'), confirmation = SecretText('synthetic package password');
    final work = c.export(password, confirmation);
    await Future<void>.delayed(Duration.zero);
    expect(service.exports, 1);
    c.cancel();
    expect(service.nativeCancel, isTrue);
    expect(password.isWiped && confirmation.isWiped, isTrue);
    service.exportWait!.complete(outcome());
    await work;
    expect(files.finishes, 0);
    expect(c.exported, isNull);
    expect(c.phase, PortablePhase.cancelled);
    c.dispose();
  });
  test('inspect only authenticates/reviews; no destinationchooser orinstallmethod', () async {
    final service = Service(), files = Files(), c = controller(service, files);
    final password = SecretText('synthetic package password');
    await c.inspect(password);
    expect(c.phase, PortablePhase.inspected);
    expect(c.imported!.files, 1);
    expect(service.inspects, 1);
    expect(service.exports, 0);
    expect(files.saves, 0);
    expect(password.isWiped, isTrue);
    c.dispose();
  });
  test('closing whileNativepending wipespassword and neverfinishes fileSave', () async {
    final service = Service()..exportWait = Completer<PortableExportOutcome>(),
        files = Files(),
        c = controller(service, files);
    await c.load();
    c.select('play.yml', true);
    final password = SecretText('synthetic package password'), confirmation = SecretText('synthetic package password');
    final work = c.export(password, confirmation);
    await Future<void>.delayed(Duration.zero);
    c.dispose();
    expect(service.nativeCancel, isTrue);
    expect(password.isWiped && confirmation.isWiped, isTrue);
    service.exportWait!.complete(outcome());
    await work;
    expect(files.finishes, 0);
  });
  test('new inspection clears previous summary immediately and a rejected package cannot display it', () async {
    final service = Service(), files = Files(), c = controller(service, files);
    await c.inspect(SecretText('synthetic package password'));
    expect(c.imported, isNotNull);
    service.inspectWait = Completer<PortableImportPreview>();
    service.inspectEntered = Completer<void>();
    final password = SecretText('synthetic package password'), retry = c.inspect(password);
    expect(c.imported, isNull);
    await service.inspectEntered!.future;
    service.inspectWait!.completeError(const PortablePackageFailure('portable_context_or_package_unavailable'));
    await retry;
    expect(c.phase, PortablePhase.failed);
    expect(c.imported, isNull);
    expect(password.isWiped, isTrue);
    c.dispose();
  });
  test('invalid new password clears an earlier preview without another picker or service call', () async {
    final service = Service(), files = Files(), c = controller(service, files);
    await c.inspect(SecretText('synthetic package password'));
    final password = SecretText('');
    await c.inspect(password);
    expect(c.imported, isNull);
    expect(c.phase, PortablePhase.failed);
    expect(files.opens, 1);
    expect(service.inspects, 1);
    expect(password.isWiped, isTrue);
    c.dispose();
  });
  test('cancelling a pending new inspection forwards only its signal and cannot restore the old summary', () async {
    final service = Service(), files = Files(), c = controller(service, files);
    await c.inspect(SecretText('synthetic package password'));
    service.inspectWait = Completer<PortableImportPreview>();
    service.inspectEntered = Completer<void>();
    final password = SecretText('synthetic package password'), work = c.inspect(password);
    await service.inspectEntered!.future;
    c.cancel();
    expect(service.nativeCancel, isTrue);
    expect(c.imported, isNull);
    expect(password.isWiped, isTrue);
    service.inspectWait!.complete(preview());
    await work;
    expect(c.phase, PortablePhase.cancelled);
    expect(c.imported, isNull);
    c.dispose();
  });
  test(
    'picker filters are not validation: wrong extension reaches no Native operation and is never rewritten',
    () async {
      expect(portablePackageFilename(r'C:\owned\a.ccproject'), isTrue);
      expect(portablePackageFilename('/owned/a.ccproject'), isTrue);
      for (final path in ['/owned/a.CCPROJECT', '/owned/.ccproject', '/owned/a.txt', '/owned/a.ccproject/']) {
        final service = Service(),
            files = Files()
              ..savePath = path
              ..openPath = path,
            c = controller(service, files);
        await c.load();
        c.select('play.yml', true);
        await c.export(SecretText('synthetic package password'), SecretText('synthetic package password'));
        expect(service.exports, 0);
        expect(files.finishes, 0);
        expect(c.phase, PortablePhase.failed);
        await c.inspect(SecretText('synthetic package password'));
        expect(service.inspects, 0);
        expect(c.phase, PortablePhase.failed);
        c.dispose();
      }
    },
  );
  test(
    'package passwords are wiped before an awaited mobile export completion and cancelled completion is not success',
    () async {
      final service = Service(),
          files = Files()
            ..finishWait = Completer<bool>()
            ..finishEntered = Completer<void>(),
          c = controller(service, files);
      await c.load();
      c.select('play.yml', true);
      final password = SecretText('synthetic package password'),
          confirmation = SecretText('synthetic package password');
      final work = c.export(password, confirmation);
      await files.finishEntered!.future;
      expect(service.exports, 1);
      expect(password.isWiped && confirmation.isWiped, isTrue);
      expect(c.exported, isNull);
      files.finishWait!.complete(false);
      await work;
      expect(c.exported, isNull);
      expect(c.phase, PortablePhase.selecting);
      c.dispose();
    },
  );
  test(
    'failed context reload discards previous review and selection; pending context cancellation is forwarded',
    () async {
      final service = Service(), files = Files(), c = controller(service, files);
      await c.load();
      c.select('play.yml', true);
      service.contextWait = Completer<PortableContext>();
      service.contextEntered = Completer<void>();
      final work = c.load();
      expect(c.review, isNull);
      expect(c.selected, isEmpty);
      await service.contextEntered!.future;
      c.cancel();
      expect(service.nativeCancel, isTrue);
      service.contextWait!.complete(context());
      await work;
      expect(c.review, isNull);
      expect(c.selected, isEmpty);
      expect(c.phase, PortablePhase.cancelled);
      c.dispose();
    },
  );
  test('v1 is an integer and encrypted count cannot exceed exact Native envelope bound', () {
    final raw = scope().toJson()..['version'] = 1.0;
    expect(() => PortableScope.fromJson(raw), throwsA(isA<PortablePackageFailure>()));
    Map<String, dynamic> value(int bytes, Object version) => {
      'version': version,
      'encryptedSha256': 'c' * 64,
      'bytes': bytes,
    };
    expect(PortableExportOutcome.fromJson(value(portableMaxEncryptedBytes, 1)).bytes, portableMaxEncryptedBytes);
    for (final row in [value(portableMaxEncryptedBytes + 1, 1), value(0, 1), value(100, 1.0)]) {
      expect(() => PortableExportOutcome.fromJson(row), throwsA(isA<PortablePackageFailure>()));
    }
  });

  test('empty explicit selection returns a visible failure rather than an unhandled export Future', () async {
    final service = Service(), files = Files(), c = controller(service, files);
    await c.load();
    final password = SecretText('synthetic package password'), confirmation = SecretText('synthetic package password');
    await c.export(password, confirmation);
    expect(c.phase, PortablePhase.failed);
    expect(service.exports, 0);
    expect(files.saves, 0);
    expect(password.isWiped && confirmation.isWiped, isTrue);
    c.dispose();
  });
  test('disappearing cancel transport cannot unwind local retirement or pre-cancel binding', () {
    final operation = PortableCancellation();
    operation.bind(() => throw StateError('unrendered transport diagnostic'));
    expect(operation.cancel, returnsNormally);
    expect(operation.cancelled, isTrue);
    operation.unbind();
    expect(() => operation.bind(() => throw StateError('unrendered late cancel diagnostic')), returnsNormally);
    operation.unbind();
  });
}
