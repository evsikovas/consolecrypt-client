import 'dart:async';
import 'dart:convert';

import 'package:consolecrypt/monitoring/job_context_focus.dart';
import 'package:consolecrypt/monitoring/job_context_navigation.dart';
import 'package:consolecrypt/monitoring/job_metric_correlation.dart';
import 'package:consolecrypt/monitoring/monitoring_host_context.dart';
import 'package:consolecrypt/monitoring/monitoring_models.dart';
import 'package:consolecrypt/monitoring/monitoring_service.dart';
import 'package:flutter_test/flutter_test.dart';

void check(bool value) {
  if (!value) throw StateError('Context focus expectation failed');
}

String id(int value) => '00000000-0000-4000-8000-${value.toRadixString(16).padLeft(12, '0')}';
final scope = JobCorrelationScope(id(1));
final source = MonitoringSource(
  id: id(2),
  revision: 4,
  name: 'Owned source',
  endpoint: 'https://PRIVATE_ENDPOINT.invalid',
);
final binding = MonitoringBinding(
  id: id(3),
  name: 'Owned binding',
  hostId: id(4),
  sourceId: source.id,
  revision: 7,
  sourceRevision: source.revision,
  state: MonitoringBindingState.current,
  selector: const {'instance': 'PRIVATE_SELECTOR'},
);
Map<String, dynamic> wire({String? nonce, int hostRevision = 9, String protocol = 'ssh'}) => {
  'version': 1,
  'profileId': scope.profileId,
  'notificationScopeId': nonce ?? id(900),
  'hostId': binding.hostId,
  'hostRevision': hostRevision,
  'protocol': protocol,
  'sourceId': source.id,
  'sourceRevision': source.revision,
  'bindingId': binding.id,
  'bindingRevision': binding.revision,
};
JobCorrelationContext context({String? hostId}) => JobCorrelationContext(
  scope: scope,
  hostId: hostId ?? binding.hostId,
  hostRevision: 9,
  sourceId: source.id,
  sourceRevision: source.revision,
  bindingId: binding.id,
  bindingRevision: binding.revision,
  bindingHostId: hostId ?? binding.hostId,
  bindingSourceId: source.id,
  bindingSourceRevision: source.revision,
  bindingState: JobCorrelationBindingState.current,
  startUtc: DateTime.utc(2026, 11, 12, 12),
  endUtc: DateTime.utc(2026, 11, 12, 13),
);
JobContextNavigationFrame frame() => JobContextNavigationFrame(
  context: context(),
  notificationScopeId: id(900),
  metadata: JobContextMetadata.parse(wire()),
);
JobContextFocus focus([JobContextDestination destination = JobContextDestination.sshHost]) {
  final original = frame(), owner = JobContextNavigation();
  owner.bind(original);
  final intent = owner.propose(original, destination).intent!;
  return JobContextFocus.fromTaken(owner.take(intent)!, original)!;
}

Future<void> denied(Future<void> Function() body, MonitoringHostContextIssue issue) async {
  try {
    await body();
  } on MonitoringHostContextException catch (error) {
    check(error.issue == issue);
    return;
  }
  throw StateError('Expected typed host-context denial');
}

Future<void> main() async {
  test('optional context capability is unavailable without native RPC and never falls back to a query', () async {
    var reads = 0;
    final service = AppCoreMonitoringService(
      read: (_) async {
        reads++;
        return {};
      },
      cancelOperation: (_) async {},
    );
    check(!service.hostContextSupported);
    await denied(() async {
      await service.captureHostContext(profileId: scope.profileId, source: source, binding: binding);
    }, MonitoringHostContextIssue.unsupported);
    check(reads == 0);
  });
  test('capture sends only exact eight metadata fields and accepts the actual native host revision', () async {
    final requests = <Json>[];
    final service = AppCoreMonitoringService(
      read: (_) async => throw StateError('No metric query permitted'),
      cancelOperation: (_) async {},
      hostContextRead: (q) async {
        requests.add(Map.of(q));
        return wire();
      },
    );
    final metadata = await service.captureHostContext(profileId: scope.profileId, source: source, binding: binding);
    final q = requests.single;
    check(
      q.length == 8 &&
          q['kind'] == 'capture' &&
          !q.containsKey('hostRevision') &&
          !q.containsKey('notificationScopeId'),
    );
    check(!jsonEncode(q).contains('PRIVATE_') && !q.containsKey('selector'));
    check(metadata.hostRevision == 9 && metadata.notificationScopeId == id(900));
  });
  test('recheck sends original nonce and host revision with no optional latest fallback', () async {
    final requests = <Json>[];
    final service = AppCoreMonitoringService(
      read: (_) async => {},
      cancelOperation: (_) async {},
      hostContextRead: (q) async {
        requests.add(Map.of(q));
        return wire();
      },
    );
    await service.recheckHostContext(JobContextMetadata.parse(wire())!);
    check(requests.single['kind'] == 'recheck' && requests.single.length == 10);
    check(requests.single['notificationScopeId'] == id(900) && requests.single['hostRevision'] == 9);
  });
  test('replacement nonce host/source revision and foreign profile responses are rejected', () async {
    for (final response in [
      wire(nonce: id(901)),
      wire(hostRevision: 10),
      wire()..['sourceRevision'] = 5,
      wire()..['profileId'] = id(99),
    ]) {
      var calls = 0;
      final service = AppCoreMonitoringService(
        read: (_) async => {},
        cancelOperation: (_) async {},
        hostContextRead: (_) async {
          calls++;
          return response;
        },
      );
      await denied(() async {
        await service.recheckHostContext(JobContextMetadata.parse(wire())!);
      }, MonitoringHostContextIssue.changed);
      check(calls == 1);
    }
  });
  test('malformed extra authority flags fail closed without raw metadata diagnostics', () async {
    final service = AppCoreMonitoringService(
      read: (_) async => {},
      cancelOperation: (_) async {},
      hostContextRead: (_) async => wire()..['approved'] = true,
    );
    await denied(() async {
      await service.captureHostContext(profileId: scope.profileId, source: source, binding: binding);
    }, MonitoringHostContextIssue.invalidResponse);
  });
  test('stale or foreign saved binding is denied before metadata RPC', () async {
    var calls = 0;
    final service = AppCoreMonitoringService(
      read: (_) async => {},
      cancelOperation: (_) async {},
      hostContextRead: (_) async {
        calls++;
        return wire();
      },
    );
    final stale = MonitoringBinding(
      id: binding.id,
      name: binding.name,
      hostId: binding.hostId,
      sourceId: source.id,
      revision: 8,
      sourceRevision: 3,
      state: MonitoringBindingState.staleSource,
      selector: const {},
    );
    await denied(() async {
      await service.captureHostContext(profileId: scope.profileId, source: source, binding: stale);
    }, MonitoringHostContextIssue.invalidRequest);
    check(calls == 0);
  });
  test('source-page hide after consumed navigation does not invalidate the original session focus', () async {
    final packet = focus(), pending = Completer<JobContextMetadata>();
    var sourcePageGeneration = 5, sourcePageActive = true, rechecks = 0;
    final ready = admitJobContextFocus(
      packet,
      destination: JobContextDestination.sshHost,
      originalSessionScope: scope,
      currentSessionScope: () => scope,
      targetCurrent: () => true,
      recheck: (original) {
        check(original.notificationScopeId == id(900));
        rechecks++;
        return pending.future;
      },
    );
    sourcePageGeneration++;
    sourcePageActive = false;
    pending.complete(JobContextMetadata.parse(wire())!);
    check(await ready != null && rechecks == 1 && sourcePageGeneration == 6 && !sourcePageActive);
    // No execution callback exists in this API. Ready means chooser focus only.
  });
  test('same-profile lock/reopen lifetime is rejected before any native recheck', () async {
    final packet = focus();
    var calls = 0;
    final reopened = JobCorrelationScope(scope.profileId);
    final ready = await admitJobContextFocus(
      packet,
      destination: JobContextDestination.sshHost,
      originalSessionScope: reopened,
      currentSessionScope: () => reopened,
      targetCurrent: () => true,
      recheck: (_) async {
        calls++;
        return JobContextMetadata.parse(wire())!;
      },
    );
    check(ready == null && calls == 0);
  });
  test('late recheck cannot show chooser after profile reopen or target route hide', () async {
    for (final switchProfile in [true, false]) {
      final packet = focus(), pending = Completer<JobContextMetadata>();
      JobCorrelationScope? current = scope;
      var visible = true;
      final ready = admitJobContextFocus(
        packet,
        destination: JobContextDestination.sshHost,
        originalSessionScope: scope,
        currentSessionScope: () => current,
        targetCurrent: () => visible,
        recheck: (_) => pending.future,
      );
      if (switchProfile) {
        current = JobCorrelationScope(scope.profileId);
      } else {
        visible = false;
      }
      pending.complete(JobContextMetadata.parse(wire())!);
      check(await ready == null);
    }
  });
  test('latest replacement nonce response cannot keep an original handoff alive', () async {
    final ready = await admitJobContextFocus(
      focus(),
      destination: JobContextDestination.sshHost,
      originalSessionScope: scope,
      currentSessionScope: () => scope,
      targetCurrent: () => true,
      recheck: (_) async => JobContextMetadata.parse(wire(nonce: id(901)))!,
    );
    check(ready == null);
  });
  test('handoff is one-use and cannot focus another destination or a cached page repeatedly', () async {
    final packet = focus();
    var calls = 0;
    Future<JobContextFocus?> receive(JobContextDestination destination) => admitJobContextFocus(
      packet,
      destination: destination,
      originalSessionScope: scope,
      currentSessionScope: () => scope,
      targetCurrent: () => true,
      recheck: (_) async {
        calls++;
        return JobContextMetadata.parse(wire())!;
      },
    );
    check(await receive(JobContextDestination.sshHost) != null);
    check(await receive(JobContextDestination.sshHost) == null && calls == 1);
    check(await receive(JobContextDestination.dockerHost) == null && calls == 1);
  });
  test('foreign frame metadata cannot be attached to a taken selection', () {
    final original = frame(), nav = JobContextNavigation();
    nav.bind(original);
    final selected = nav.take(nav.propose(original, JobContextDestination.sshHost).intent!)!;
    final foreign = JobContextNavigationFrame(
      context: context(hostId: id(55)),
      notificationScopeId: id(900),
      metadata: original.metadata,
    );
    check(JobContextFocus.fromTaken(selected, foreign) == null);
  });
}
