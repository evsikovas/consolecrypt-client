import 'dart:typed_data';

import 'package:consolecrypt/rdp/rdp_service.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  test('saved host ticket rejects missing native snapshot or mismatched endpoint', () {
    final options = RdpConnectionOptions(address: 'example.test', username: 'demo');
    expect(
      () => RdpSavedHostTicket(
        hostId: 'demo-id',
        name: 'Demo',
        options: options,
        certificate: RdpCertificate(address: 'example.test', port: 3389, sha256: List.filled(64, 'a').join()),
        hasSavedPassword: true,
      ),
      throwsFormatException,
    );
    expect(
      () => RdpSavedHostTicket(
        hostId: 'demo-id',
        name: 'Demo',
        options: options,
        certificate: RdpCertificate(
          address: 'different.example.test',
          port: 3389,
          sha256: List.filled(64, 'a').join(),
          snapshotStamp: 'opaque-native-snapshot',
        ),
        hasSavedPassword: true,
      ),
      throwsFormatException,
    );
  });

  test('frame validates allocation and owns immutable exact RGBA buffer', () {
    final bytes = Uint8List.fromList([1, 2, 3, 255]);
    final frame = RdpFrame(sequence: 0, width: 1, height: 1, rgba: bytes);
    bytes[0] = 99;
    expect(frame.rgba.first, 1);
    expect(() => frame.rgba[0] = 99, throwsUnsupportedError);
    expect(() => RdpFrame(sequence: 0, width: 4097, height: 1, rgba: bytes), throwsFormatException);
    expect(() => RdpFrame(sequence: -1, width: 1, height: 1, rgba: bytes), throwsFormatException);
    expect(() => RdpFrame(sequence: 0, width: 2, height: 1, rgba: bytes), throwsFormatException);
    expect(() => RdpFrame(sequence: 0, width: 4096, height: 2161, rgba: bytes), throwsFormatException);
  });

  test('options reject invalid endpoints and certificate shows all 32 bytes', () {
    expect(() => RdpConnectionOptions(address: 'host/name', username: 'demo'), throwsFormatException);
    expect(() => RdpConnectionOptions(address: 'example.test', username: 'demo', port: 0), throwsFormatException);
    expect(() => RdpConnectionOptions(address: 'example.test', username: 'demo', height: 2161), throwsFormatException);
    expect(() => RdpCertificate(address: 'example.test', port: 3389, sha256: 'ab'), throwsFormatException);
    final certificate = RdpCertificate(address: 'example.test', port: 3389, sha256: List.filled(32, 'AB').join());
    expect(certificate.displaySha256.split(':'), hasLength(32));
    expect(certificate.sha256.length, 64);
    expect(RdpConnectionOptions(address: '::1', username: 'demo').endpoint, '[::1]:3389');
  });
}
