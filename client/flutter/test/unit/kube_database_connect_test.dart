import 'package:consolecrypt/db/db_models.dart';
import 'package:consolecrypt/kubernetes/kube_database_connect.dart';
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
}
