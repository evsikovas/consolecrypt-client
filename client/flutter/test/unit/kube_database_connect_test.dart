import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/db/db_errors.dart';
import 'package:consolecrypt/db/db_models.dart';
import 'package:consolecrypt/kubernetes/kube_database_connect.dart';
import 'package:flutter/widgets.dart';
import 'package:flutter_test/flutter_test.dart';

Map<String, dynamic> _pod(List<Map<String, dynamic>> containers) => {
  'spec': {'containers': containers},
};

void main() {
  test('database pods are recognised by image or port, with plain env hints', () {
    final pg = kubeDatabaseTarget(
      _pod([
        {
          'name': 'postgres',
          'image': 'postgres:17.11-alpine@sha256:b0f9',
          'ports': [
            {'containerPort': 5432, 'protocol': 'TCP'},
          ],
          'env': [
            {'name': 'POSTGRES_DB', 'value': 'forever'},
            {'name': 'POSTGRES_USER', 'value': 'forever_app'},
            {
              'name': 'POSTGRES_PASSWORD',
              'valueFrom': {
                'secretKeyRef': {'name': 's', 'key': 'p'},
              },
            },
          ],
        },
      ]),
    )!;
    expect(pg.engine, DbEngine.postgresql);
    expect(pg.remotePort, 5432);
    expect(pg.database, 'forever');
    expect(pg.user, 'forever_app');

    final byPort = kubeDatabaseTarget(
      _pod([
        {'name': 'app', 'image': 'nginx'},
        {
          'name': 'cache',
          'image': 'example/cache:1',
          'ports': [
            {'containerPort': 6379},
          ],
        },
      ]),
    )!;
    expect(byPort.engine, DbEngine.redis);
    expect(byPort.remotePort, 6379);

    expect(
      kubeDatabaseTarget(
        _pod([
          {
            'name': 'web',
            'image': 'nginx:1.27',
            'ports': [
              {'containerPort': 80},
            ],
          },
        ]),
      ),
      isNull,
    );
    expect(
      kubeDatabaseTarget(
        _pod([
          {'name': 'ch', 'image': 'clickhouse/clickhouse-server:25.8'},
        ]),
      )!.remotePort,
      8123,
    );
  });

  group('saved route target', () {
    Map<String, dynamic> pod({String? owner, String? ownerName, String? hash}) => {
      'metadata': {
        'name': 'pg-0',
        if (hash != null) 'labels': {'pod-template-hash': hash},
        if (owner != null)
          'ownerReferences': [
            {'kind': owner, 'name': ownerName, 'controller': true},
          ],
      },
    };

    test('StatefulSet pods keep their stable name, even with a Service', () async {
      expect(
        await kubeDatabaseRouteTarget(
          pod(owner: 'StatefulSet', ownerName: 'pg'),
          'pg-0',
          services: () async => ['pg'],
        ),
        (DbKubeTargetKind.pod, 'pg-0'),
      );
    });

    test('the only selecting Service is preferred, then the Deployment', () async {
      final deployed = pod(owner: 'ReplicaSet', ownerName: 'orders-db-5d8f7c9b4', hash: '5d8f7c9b4');
      expect(await kubeDatabaseRouteTarget(deployed, 'orders-db-5d8f7c9b4-x2k', services: () async => ['orders-db']), (
        DbKubeTargetKind.service,
        'orders-db',
      ));
      // Ambiguous or failing lookups fall back to the owning Deployment.
      expect(await kubeDatabaseRouteTarget(deployed, 'orders-db-5d8f7c9b4-x2k', services: () async => ['rw', 'ro']), (
        DbKubeTargetKind.deployment,
        'orders-db',
      ));
      expect(
        await kubeDatabaseRouteTarget(
          deployed,
          'orders-db-5d8f7c9b4-x2k',
          services: () async => throw StateError('relations unavailable'),
        ),
        (DbKubeTargetKind.deployment, 'orders-db'),
      );
      expect(await kubeDatabaseRouteTarget(pod(), 'scratch'), (DbKubeTargetKind.pod, 'scratch'));
      expect(await kubeDatabaseRouteTarget(pod(owner: 'ReplicaSet', ownerName: 'odd'), 'odd-x'), (
        DbKubeTargetKind.pod,
        'odd-x',
      ));
    });
  });

  test('the Kubernetes route has the native wire shape and compares by workload', () {
    const route = DbRoute(
      kind: DbRouteKind.kubernetes,
      reference: 'ctx-1',
      namespace: 'data',
      targetKind: DbKubeTargetKind.statefulSet,
      targetName: 'pg',
    );
    expect(route.toJson(), {
      'kind': 'kubernetes',
      'context_id': 'ctx-1',
      'namespace': 'data',
      'target_kind': 'stateful_set',
      'target_name': 'pg',
    });
    final restored = DbRoute.fromJson(route.toJson());
    expect(restored.sameAs(route), isTrue);
    expect(restored.restorable, isTrue);
    expect(
      route.sameAs(
        const DbRoute(
          kind: DbRouteKind.kubernetes,
          reference: 'ctx-1',
          namespace: 'data',
          targetKind: DbKubeTargetKind.pod,
          targetName: 'pg',
        ),
      ),
      isFalse,
    );
    expect(const DbRoute(kind: DbRouteKind.tunnel, reference: 't1').toJson(), {'kind': 'tunnel', 'tunnel_id': 't1'});
    expect(const DbRoute().toJson(), {'kind': 'direct'});
  });

  test('route restore failures have their own messages', () {
    final l = lookupAppLocalizations(const Locale('en'));
    String text(String rule) =>
        dbFailureText(AppException(AppErrorCode.validation, rule, args: {'field': 'database', 'rule': rule}), l);
    expect(text('kube_unreachable'), l.dbErrorKubeUnreachable);
    expect(text('kube_target_missing'), l.dbErrorKubeTarget);
    expect(text('kube_access_denied'), l.dbErrorKubeDenied);
    expect(text('kube_forward_blocked'), l.kubeForwardProductionBlocked);
    expect(text('kube_forward_failed'), l.dbErrorKubeForward);
    expect(text('tunnel_failed'), l.dbErrorTunnel);
    expect(text('tunnel_unsupported'), l.dbErrorTunnelKind);
    expect(text('docker_unreachable'), l.dbErrorDockerUnreachable);
    expect(text('docker_container_missing'), l.dbErrorDockerContainer);
    expect(text('docker_forward_failed'), l.dbErrorDockerForward);
  });
}
