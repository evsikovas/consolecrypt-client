import 'package:consolecrypt/wslc/features/containers/files_panel.dart';
import 'package:consolecrypt/wslc/shared/ports.dart';
import 'package:consolecrypt/wslc/src/rust/models.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  test('list port strings become published ports', () {
    final ports = parsePorts(
      '127.0.0.1:18081->80/tcp, 0.0.0.0:53->53/udp, [::]:8443->443/tcp, '
      ':::9000->9000/tcp, 6379/tcp, 8000-8001->80-81/tcp',
    );
    expect(ports, hasLength(5));
    expect(ports[0].hostIp, '127.0.0.1');
    expect(ports[0].hostPort, 18081);
    expect(ports[0].loopbackOnly, isTrue);
    expect(ports[0].url('localhost'), 'http://localhost:18081');
    expect(ports[1].tcp, isFalse);
    expect(ports[1].loopbackOnly, isFalse);
    expect(ports[2].hostIp, '::');
    expect(ports[2].https, isTrue);
    expect(ports[2].url('192.168.1.5'), 'https://192.168.1.5:8443');
    expect(ports[3].hostIp, '::');
    expect(ports[4].published, isFalse);
    expect(parsePorts(null), isEmpty);
    expect(parsePorts(''), isEmpty);
  });

  test('inspect bindings and container paths', () {
    final port = PublishedPort.fromBinding(const PortBinding(containerPort: '80/tcp', hostIp: '::1', hostPort: '8080'));
    expect(port.loopbackOnly, isTrue);
    expect(port.url('::1'), 'http://[::1]:8080');
    expect(joinPath('/', 'etc'), '/etc');
    expect(joinPath('/usr/share', 'nginx'), '/usr/share/nginx');
    expect(parentPath('/usr/share/nginx'), '/usr/share');
    expect(parentPath('/usr'), '/');
    expect(parentPath('/'), '/');
  });

  test('real adapters are offered before virtual ones and VPNs', () {
    const found = [
      HostAddress('AmneziaVPN', '10.8.1.1'),
      HostAddress('Ethernet', '192.168.56.1'),
      HostAddress('Беспроводная сеть', '10.10.10.41'),
      HostAddress('vEthernet (Default Switch)', '172.31.192.1'),
      HostAddress('Ethernet 2', '192.168.1.20'),
    ];
    final sorted = [...found]..sort((a, b) => addressRank(a) - addressRank(b));
    expect(sorted.map((a) => a.address).take(2), ['10.10.10.41', '192.168.1.20']);
    expect(sorted.skip(2).every((a) => addressRank(a) == 3), isTrue);
  });
}
