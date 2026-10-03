import 'package:consolecrypt/core/bridge/mapping.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  test('RDP type and Windows fields survive mapping and group movement', () {
    final now = DateTime.now().toUtc();
    final h = Host(
      id: ObjectId.generate(),
      name: 'Windows',
      address: 'windows.example.test',
      createdAt: now,
      updatedAt: now,
      protocol: HostProtocol.rdp,
      rdpDomain: 'LAB',
      rdpWidth: 1920,
      rdpHeight: 1080,
    );
    final mapped = hostFromJson(hostToJson(h.withGroup(ObjectId.generate())));
    expect(mapped.isRdp, isTrue);
    expect(mapped.rdpDomain, 'LAB');
    expect(mapped.rdpWidth, 1920);
    expect(mapped.rdpHeight, 1080);
    final legacy = hostToJson(h)
      ..remove('protocol')
      ..remove('rdp_width')
      ..remove('rdp_height');
    expect(hostFromJson(legacy).isRdp, isFalse);
  });
}
