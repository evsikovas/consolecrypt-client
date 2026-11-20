import 'dart:async';
import 'dart:convert';

import 'package:consolecrypt/automation/automation_dependency_controller.dart';
import 'package:consolecrypt/automation/automation_dependency_models.dart';
import 'package:consolecrypt/automation/automation_service.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:flutter_test/flutter_test.dart';

import '../helpers/automation_dependency_fixture.dart';

void main() {
  test('source review alone never downloads and start sends only captured review identity', () async {
    final s = Service();
    final model = controller(s);
    await model.reviewSource(proposal);
    expect(s.requests.map((x) => x['type']), ['review']);
    expect(model.review!.networkOrigin, 'https://fixtures.invalid');
    await model.download();
    expect(s.requests[1], {'type': 'start', 'reviewId': reviewId});
    expect(
      s.requests.any((x) => x['type'] == 'accept' || x['type'] == 'publish'),
      false,
    );
    model.dispose();
  });
  test(
    'unsupported Native profile and nonmanaged source never download',
    () async {
      final s = Service()..supported = false;
      final model = controller(s);
      await model.reviewSource(proposal);
      await model.download();
      expect(s.requests.length, 1);
      expect(model.review!.publicationSupported, false);
      model.dispose();
      final other = Service();
      final unmanaged = controller(other, managed: false);
      await unmanaged.reviewSource(proposal);
      expect(other.requests, isEmpty);
      unmanaged.dispose();
    },
  );
  test('explicit reviewed install accepts then publishes exact captured digest once', () async {
    final s = Service();
    final model = controller(s);
    await ready(model);
    await model.install();
    expect(s.requests.any((x) => x['type'] == 'accept'), false);
    model.markDiffViewed();
    await model.install();
    expect(
      s.requests
          .where((x) => x['type'] == 'accept' || x['type'] == 'publish')
          .toList(),
      [
        {'type': 'accept', 'operationId': operation, 'reviewDigest': digest},
        {'type': 'publish', 'operationId': operation, 'reviewDigest': digest},
      ],
    );
    expect(model.phase, DependencyInstallerPhase.published);
    expect(model.published!.revision, 2);
    await model.install();
    expect(s.requests.where((x) => x['type'] == 'publish').length, 1);
    model.dispose();
  });
  test(
    'cancel causally while accept blocked never sends publication',
    () async {
      final s = Service();
      final c = controller(s);
      await ready(c);
      c.markDiffViewed();
      final entered = Completer<void>(), release = Completer<String>();
      s.handler = (action) {
        if (action['type'] == 'accept') {
          entered.complete();
          return release.future;
        }
        return Future.value(s.defaultReply(action));
      };
      final installing = c.install();
      await entered.future;
      await c.cancel();
      release.complete(s.defaultReply({'type': 'accept'}));
      await installing;
      expect(s.requests.any((x) => x['type'] == 'publish'), false);
      expect(c.phase, DependencyInstallerPhase.cancelled);
      c.dispose();
    },
  );
  test(
    'original UI scope lost during acceptance cannot publish in new scope',
    () async {
      var current = true;
      final s = Service();
      final model = controller(s, current: () => current);
      await ready(model);
      model.markDiffViewed();
      final entered = Completer<void>(), release = Completer<String>();
      s.handler = (action) {
        if (action['type'] == 'accept') {
          entered.complete();
          return release.future;
        }
        return Future.value(s.defaultReply(action));
      };
      final installing = model.install();
      await entered.future;
      current = false;
      release.complete(s.defaultReply({'type': 'accept'}));
      await installing;
      expect(s.requests.any((x) => x['type'] == 'publish'), false);
      model.dispose();
    },
  );
  test(
    'late status cannot re-enable candidate after explicit publication',
    () async {
      final s = Service();
      final model = controller(s);
      await ready(model);
      model.markDiffViewed();
      final entered = Completer<void>(), release = Completer<String>();
      s.handler = (action) {
        if (action['type'] == 'status') {
          entered.complete();
          return release.future;
        }
        return Future.value(s.defaultReply(action));
      };
      final polling = model.poll();
      await entered.future;
      await model.install();
      expect(model.phase, DependencyInstallerPhase.published);
      release.complete(reply('operation', status()));
      await polling;
      expect(model.phase, DependencyInstallerPhase.published);
      model.dispose();
    },
  );
  test('unknown post-publication outcome remains uncertain and cannot rearm or retry', () async {
    final s = Service();
    final model = controller(s);
    await ready(model);
    model.markDiffViewed();
    s.handler = (action) {
      if (action['type'] == 'publish') {
        throw StateError('SYNTHETIC_BACKEND_MARKER');
      }
      return Future.value(s.defaultReply(action));
    };
    await model.install();
    expect(model.phase, DependencyInstallerPhase.recoveryRequired);
    expect(model.failure, DependencyFailureCode.recoveryRequired);
    final count = s.requests.length;
    model.changed();
    await model.reviewSource(proposal);
    await model.install();
    expect(s.requests.length, count);
    expect(model.phase, DependencyInstallerPhase.recoveryRequired);
    model.dispose();
  });
  test('diff adapter preserves native page limits and exact review plus candidate hashes', () async {
    final s = Service();
    final model = controller(s);
    await ready(model);
    await model.projectDiff(project, currentDigest: candidate, offset: 50);
    expect(s.requests.last, {
      'type': 'changes',
      'operationId': operation,
      'reviewDigest': digest,
      'offset': 50,
      'limit': 50,
    });
    await model.projectFileDiff(
      project,
      'roles/demo/tasks/main.yml',
      currentDigest: candidate,
    );
    final count = s.requests.length;
    await expectLater(
      model.projectDiff(project, currentDigest: base),
      throwsA(isA<DependencyInstallerFailure>()),
    );
    expect(s.requests.length, count);
    model.dispose();
  });
  test(
    'late source review is discarded under original operation identity',
    () async {
      var current = true;
      final s = Service(), model = controller(s, current: () => current);
      final release = Completer<String>(), entered = Completer<void>();
      s.handler = (action) {
        if (action['type'] == 'review') {
          entered.complete();
          return release.future;
        }
        return Future.value(s.defaultReply(action));
      };
      final pending = model.reviewSource(proposal);
      await entered.future;
      current = false;
      release.complete(reply('sourceReview', source()));
      await pending;
      expect(model.review, isNull);
      expect(
        s.requests.any(
          (x) => x['type'] == 'cancel' && x['operationId'] == operation,
        ),
        true,
      );
      model.dispose();
    },
  );
  test('Native fixed uncertain code preserved and backend details never become UI error', () async {
    final s = Service()
      ..handler = (_) => throw const AppException(
        AppErrorCode.validation,
        'SYNTHETIC_NATIVE_TEXT',
        args: {
          'field': 'automation_dependency',
          'rule': 'dependency_publication_recovery_required',
        },
      );
    await expectLater(
      DependencyActionClient(s)
          .call({'type': 'status', 'operationId': operation}, 'operation'),
      throwsA(
        isA<DependencyInstallerFailure>().having(
          (e) => e.code,
          'fixed',
          DependencyFailureCode.recoveryRequired,
        ),
      ),
    );
  });
  test('wire versions quotas and source echoes are fail closed', () {
    expect(
      () => DependencyReply.parse(
        jsonEncode({'version': 2, 'kind': 'operation', 'data': <String, dynamic>{}}),
        'operation',
      ),
      throwsA(isA<DependencyInstallerFailure>()),
    );
    expect(
      () => DependencyReply.parse('x' * (4 * 1024 * 1024 + 1), 'operation'),
      throwsA(isA<DependencyInstallerFailure>()),
    );
    final wrong = source();
    (wrong['source'] as AutomationJson)['name'] = 'another';
    expect(
      () => NativeDependencyReview.fromJson(wrong, proposal),
      throwsA(isA<DependencyInstallerFailure>()),
    );
  });
}
