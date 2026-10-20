import 'package:consolecrypt/kubernetes/kube_navigation.dart';
import 'package:consolecrypt/kubernetes/kube_providers.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  test('late palette requests and snapshots cannot cross a lock/profile boundary', () {
    const first = KubeScope('first', true);
    const second = KubeScope('second', true);
    final container = ProviderContainer(overrides: [kubeScopeProvider.overrideWithValue(first)]);
    addTearDown(container.dispose);
    final requests = container.read(kubeNavigationProvider.notifier);
    const request = KubeNavigationRequest(scope: first, contextId: 'cluster', openAi: true);
    requests.open(request);
    expect(container.read(kubeNavigationProvider), same(request));
    final active = container.read(kubeActiveNavigationProvider.notifier);
    const snapshot = KubeNavigationSnapshot(scope: first, owner: 'session', contextId: 'cluster', namespace: 'default');
    active.publish(snapshot);
    container.updateOverrides([kubeScopeProvider.overrideWithValue(second)]);
    expect(container.read(kubeNavigationProvider), isNull);
    expect(container.read(kubeActiveNavigationProvider), isNull);
    requests.open(request);
    active.publish(snapshot);
    expect(container.read(kubeNavigationProvider), isNull);
    expect(container.read(kubeActiveNavigationProvider), isNull);
  });

  test('late consume and browser disposal cannot clear a newer navigation', () {
    const scope = KubeScope('first', true);
    final container = ProviderContainer(overrides: [kubeScopeProvider.overrideWithValue(scope)]);
    addTearDown(container.dispose);
    final requests = container.read(kubeNavigationProvider.notifier);
    const first = KubeNavigationRequest(scope: scope, contextId: 'one');
    const second = KubeNavigationRequest(scope: scope, contextId: 'two');
    requests.open(first);
    requests.open(second);
    requests.consume(first);
    expect(container.read(kubeNavigationProvider), same(second));
    final active = container.read(kubeActiveNavigationProvider.notifier);
    active.publish(const KubeNavigationSnapshot(scope: scope, owner: 'new', contextId: 'two', namespace: 'default'));
    active.clear('old');
    expect(container.read(kubeActiveNavigationProvider)?.owner, 'new');
    active.clear('new');
    expect(container.read(kubeActiveNavigationProvider), isNull);
  });
}
