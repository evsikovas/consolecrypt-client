import 'package:consolecrypt/db/db_models.dart';
import 'package:consolecrypt/docker/docker_database_connect.dart';
import 'package:consolecrypt/docker/docker_service.dart';
import 'package:flutter_test/flutter_test.dart';

DockerContainer _container(String image, List<DockerPort> ports, {Map<String, String> networks = const {}}) =>
    DockerContainer(
      id: 'c1',
      name: 'db',
      image: image,
      state: DockerContainerState.running,
      ports: ports,
      networks: networks,
    );

DockerPort _port(int port, {int? public, String? ip, String protocol = 'tcp'}) =>
    DockerPort(privatePort: port, publicPort: public, protocol: protocol, ip: ip);

const _local = DockerSessionInfo(id: 's', hostId: DockerHost.localId, hostName: '', engineVersion: '27');
const _ssh = DockerSessionInfo(id: 's', hostId: 'ssh-web', hostName: 'web-01', engineVersion: '27');

void main() {
  test('database containers are recognised by their standard ports, the image refines the engine', () {
    final pg = dockerDatabaseTarget(_container('example/app:1', [_port(5432)]))!;
    expect((pg.engine, pg.remotePort), (DbEngine.postgresql, 5432));
    final maria = dockerDatabaseTarget(_container('mariadb:11', [_port(3306, public: 3306, ip: '0.0.0.0')]))!;
    expect((maria.engine, maria.remotePort), (DbEngine.mariadb, 3306));
    final ch = dockerDatabaseTarget(_container('clickhouse/clickhouse-server', [_port(9000), _port(8123)]))!;
    expect((ch.engine, ch.remotePort), (DbEngine.clickhouse, 8123));
    expect(dockerDatabaseTarget(_container('redis:7', [_port(6379)]))!.engine, DbEngine.redis);
    expect(dockerDatabaseTarget(_container('mongo:8', [_port(27017)]))!.engine, DbEngine.mongodb);

    expect(dockerDatabaseTarget(_container('nginx', [_port(80)])), isNull);
    expect(dockerDatabaseTarget(_container('postgres:16', const [])), isNull);
    expect(dockerDatabaseTarget(_container('example/app', [_port(5432, protocol: 'udp')])), isNull);
    // EXPOSEd ports that only inspect knows about count too.
    final exposed = dockerDatabaseTarget(_container('postgres:16', const []), {
      'Config': {
        'ExposedPorts': {'5432/tcp': <String, Object>{}},
      },
    });
    expect(exposed?.remotePort, 5432);
  });

  test('endpoints keep the route: published host port, container IP over SSH, else a forward', () {
    final published = _container('postgres', [
      _port(5432, public: 15433, ip: '0.0.0.0'),
      _port(5432, public: 15433, ip: '::'),
    ]);
    final local = dockerDatabaseEndpoint(_local, published, 5432)!;
    expect((local.host, local.port, local.route.kind), ('127.0.0.1', 15433, DbRouteKind.direct));
    final ssh = dockerDatabaseEndpoint(_ssh, published, 5432)!;
    expect((ssh.host, ssh.port, ssh.route.kind, ssh.route.reference), ('127.0.0.1', 15433, DbRouteKind.ssh, 'ssh-web'));

    final bound = _container('postgres', [_port(5432, public: 5432, ip: '10.0.0.7')]);
    expect(dockerDatabaseEndpoint(_ssh, bound, 5432)!.host, '10.0.0.7');

    final hidden = _container('postgres', [_port(5432)], networks: {'shop_default': '172.18.0.2'});
    expect(dockerDatabaseEndpoint(_local, hidden, 5432), isNull);
    final viaIp = dockerDatabaseEndpoint(_ssh, hidden, 5432)!;
    expect((viaIp.host, viaIp.port, viaIp.route.reference), ('172.18.0.2', 5432, 'ssh-web'));

    final hostNet = _container('postgres', [_port(5432)], networks: {'host': ''});
    expect(dockerDatabaseEndpoint(_ssh, hostNet, 5432)!.host, '127.0.0.1');
    expect(dockerDatabaseEndpoint(_ssh, _container('postgres', [_port(5432)]), 5432), isNull);
  });

  test('without another endpoint the container is saved as a Docker route by name', () {
    final hidden = _container('postgres', [_port(5432)]);
    final local = dockerForwardEndpoint(_local, hidden, 5432);
    expect((local.host, local.port), ('127.0.0.1', 5432));
    expect((local.route.kind, local.route.reference, local.route.container), (DbRouteKind.docker, null, 'db'));
    final remote = dockerForwardEndpoint(_ssh, hidden, 5432);
    expect(remote.route.toJson(), {'kind': 'docker', 'host_id': 'ssh-web', 'container': 'db'});
    expect(DbRoute.fromJson(remote.route.toJson()).sameAs(remote.route), isTrue);
    expect(
      remote.route.sameAs(const DbRoute(kind: DbRouteKind.docker, reference: 'ssh-web', container: 'other')),
      isFalse,
    );
  });

  test('database and user come from plain environment values', () {
    final hints = dockerDatabaseEnv({
      'Config': {
        'Env': ['PATH=/bin', 'POSTGRES_DB=shop', 'POSTGRES_USER=app', 'POSTGRES_PASSWORD=••••••'],
      },
    });
    expect((hints.database, hints.user), ('shop', 'app'));
    expect(dockerDatabaseEnv(const {}), (database: '', user: ''));
  });
}
