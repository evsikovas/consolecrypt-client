import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/kubernetes/kube_connection_status.dart';
import 'package:consolecrypt/kubernetes/kube_providers.dart';
import 'package:consolecrypt/kubernetes/kube_service.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

Future<void> flush() => Future<void>.delayed(Duration.zero);

void main() {
  const scope = KubeScope('first', true);
  late ProviderContainer container;
  setUp(() {
    container = ProviderContainer(
      overrides: [
        kubeScopeProvider.overrideWithValue(scope),
        kubeServiceProvider.overrideWithValue(const UnavailableKubeService()),
        kubeConnectionsProvider.overrideWith((ref, arg) async => <KubeConnection>[]),
      ],
    );
  });
  tearDown(() => container.dispose());

  test('records explicit success, distinguishes forbidden and never runs a probe', () async {
    final statuses = container.read(kubeConnectionStatusProvider.notifier);
    await container.read(kubeConnectionsProvider(scope).future);
    expect(container.read(kubeConnectionStatusProvider), isEmpty);
    final run = statuses.begin(scope, 'cluster');
    await flush();
    expect(container.read(kubeConnectionStatusProvider)['cluster']?.state, KubeConnectionState.checking);
    statuses.available(run, 'v1.32.4');
    await flush();
    statuses.cancel(run);
    await flush();
    expect(container.read(kubeConnectionStatusProvider)['cluster']?.serverVersion, 'v1.32.4');
    final retry = statuses.begin(scope, 'cluster');
    statuses.failed(retry, const AppException(AppErrorCode.forbidden, 'denied'));
    await flush();
    expect(container.read(kubeConnectionStatusProvider)['cluster']?.state, KubeConnectionState.forbidden);
    expect(container.read(kubeConnectionStatusProvider)['cluster']?.serverVersion, isNull);
  });

  test('late attempts cannot repopulate after inventory change, cancellation or lock', () async {
    final statuses = container.read(kubeConnectionStatusProvider.notifier);
    await container.read(kubeConnectionsProvider(scope).future);
    final first = statuses.begin(scope, 'cluster');
    await flush();
    container.invalidate(kubeConnectionsProvider(scope));
    await container.read(kubeConnectionsProvider(scope).future);
    statuses.available(first, 'stale');
    await flush();
    expect(container.read(kubeConnectionStatusProvider), isEmpty);
    final cancelled = statuses.begin(scope, 'cluster');
    statuses.cancel(cancelled);
    statuses.available(cancelled, 'stale');
    await flush();
    expect(container.read(kubeConnectionStatusProvider), isEmpty);
    final pending = statuses.begin(scope, 'cluster');
    container.updateOverrides([
      kubeScopeProvider.overrideWithValue(const KubeScope('first', false)),
      kubeServiceProvider.overrideWithValue(const UnavailableKubeService()),
      kubeConnectionsProvider.overrideWith((ref, arg) async => <KubeConnection>[]),
    ]);
    expect(container.read(kubeConnectionStatusProvider), isEmpty);
    statuses.available(pending, 'stale');
    await flush();
    expect(container.read(kubeConnectionStatusProvider), isEmpty);
  });
}
