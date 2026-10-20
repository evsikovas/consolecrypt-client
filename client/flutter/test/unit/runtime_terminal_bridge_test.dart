import 'dart:async';
import 'dart:convert';
import 'dart:typed_data';

import 'package:consolecrypt/core/bridge/rust_backend.dart';
import 'package:consolecrypt/core/bridge/rust_sessions.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/src/rust/api/app.dart';
import 'package:consolecrypt/src/rust/api/ssh.dart';
import 'package:consolecrypt/src/rust/frb_generated.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  late _Native api;
  late RustBackend backend;
  late RustTerminalService service;
  setUp(() {
    api = _Native();
    RustLib.initMock(api: api);
    backend = RustBackend(
      const CoreInfo(dataDir: '/unused', platform: 'macos', coreVersion: 'test', alreadyInitialized: false),
    );
    service = RustTerminalService(backend);
  });
  tearDown(() async {
    await api.frames.close();
    await backend.dispose();
    RustLib.dispose();
  });

  test('approved kubectl installer adopts native output and closes without event listener', () async {
    final attached = await service.attachRuntime('approved-installer');
    expect((attached.target as LocalTerminalTarget).profile.id, 'kubectl-install');
    expect(api.resized, ['approved-installer']);
    final output = attached.handle.output.first;
    api.frames.add(
      TerminalFrame(kind: TerminalFrameKind.data, data: Uint8List.fromList(utf8.encode('Installing kubectl'))),
    );
    expect(utf8.decode(await output), 'Installing kubectl');
    await service.close(attached.handle.id).timeout(const Duration(seconds: 1));
    expect(api.closed, ['approved-installer']);
  });

  test('unapproved runtime profile is not resized or attached', () async {
    api.profile = 'arbitrary-command';
    await expectLater(service.attachRuntime('other'), throwsA(isA<AppException>()));
    expect(api.resized, isEmpty);
    expect(api.attached, isEmpty);
  });
}

class _Native implements RustLibApi {
  // Closed by test teardown after the service has revoked its attachment.
  // ignore: close_sinks
  final frames = StreamController<TerminalFrame>.broadcast();
  final resized = <String>[];
  final closed = <String>[];
  final attached = <String>[];
  String profile = 'kubectl-install';

  @override
  Future<String> crateApiSshTerminalInfo({required String terminalId}) async => jsonEncode({
    'title': 'kubectl · brew',
    'target': {'kind': 'local', 'profile_id': profile, 'shell': '/opt/homebrew/bin/brew'},
  });
  @override
  Future<void> crateApiSshTerminalResize({required String terminalId, required int cols, required int rows}) async {
    resized.add(terminalId);
  }

  @override
  Stream<TerminalFrame> crateApiSshTerminalAttach({required String terminalId}) {
    attached.add(terminalId);
    return frames.stream;
  }

  @override
  Future<void> crateApiSshTerminalClose({required String terminalId}) async {
    closed.add(terminalId);
  }

  @override
  dynamic noSuchMethod(Invocation invocation) =>
      throw UnsupportedError('Unexpected bridge call: ${invocation.memberName}');
}
