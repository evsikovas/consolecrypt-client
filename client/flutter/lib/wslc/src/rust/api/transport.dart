import 'dart:convert';
import 'dart:typed_data';

import 'package:consolecrypt/core/bridge/mapping.dart';
import 'package:consolecrypt/core/bridge/rust_account.dart';
import 'package:consolecrypt/core/models/step_up.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/docker/docker_service.dart' show DockerWrongPassphrase;
import 'package:consolecrypt/src/rust/api/wslc.dart' as native;

import 'package:consolecrypt/wslc/src/rust/models.dart';

typedef WslcReveal = Future<dynamic> Function(String object, Future<dynamic> Function(StepUpProof proof) read);

/// One instance per workspace/profile, retained by every async callback. A
/// disposed controller cannot silently borrow a newly opened profile's lease.
class WslcTransport {
  WslcTransport({required this.profileId, required this.reveal});
  final String profileId;
  final WslcReveal reveal;
  Future<String>? _opening;
  bool _closed = false;

  AppError get expired =>
      const AppError(code: 'session_expired', operation: 'workspace', message: 'Сессия WSLc закрыта');
  Future<String> _session() async {
    if (_closed) throw expired;
    final id = await (_opening ??= () async {
      final response = jsonDecode(await guard(() => native.wslcOpen(profileId: profileId))) as Map;
      return response['session_id'] as String;
    }());
    if (_closed) throw expired;
    return id;
  }

  dynamic _unwrap(String response) {
    final envelope = jsonDecode(response) as Map;
    if (envelope.containsKey('error')) throw AppError.fromJson((envelope['error'] as Map).cast<String, dynamic>());
    return envelope['ok'];
  }

  Future<dynamic> request(String operation, Map<String, dynamic> args) async {
    final id = await _session();
    Future<dynamic> call([StepUpProof? proof]) async {
      if (_closed) throw expired;
      Future<String> read(Uint8List? bytes, String? token) => guard(
        () => native.wslcRequest(
          sessionId: id,
          operation: operation,
          argsJson: jsonEncode(wslcEncode(args)),
          passphrase: bytes,
          presenceToken: token,
        ),
      );
      final String response;
      try {
        response = proof == null
            ? await read(null, null)
            : await withStepUp(proof, (bytes, token) => read(bytes, token));
      } on AppException catch (error) {
        if (error.code == AppErrorCode.wrongPassphrase) throw const DockerWrongPassphrase();
        rethrow;
      }
      if (_closed) throw expired;
      return _unwrap(response);
    }

    final sensitive =
        operation == 'container_env' ||
        (operation == 'inspect_container' || operation == 'inspect_image') && args['reveal_environment'] == true ||
        operation == 'container_run_spec' && args['include_env'] == true;
    if (!sensitive) return call();
    final object = 'wslc:$id/${args['container_id'] ?? args['image_id'] ?? 'environment'}';
    final value = await reveal(object, (proof) => call(proof));
    if (value == null) throw const AppError(code: 'cancelled', operation: 'reveal', message: 'Отменено');
    return value;
  }

  Future<void> login({String? server, required String username, required String password}) async {
    final id = await _session();
    final response = await withSecret(
      SecretText(password),
      (bytes) =>
          guard(() => native.wslcRegistryLogin(sessionId: id, server: server, username: username, password: bytes)),
    );
    if (_closed) throw expired;
    _unwrap(response);
  }

  Future<void> close() async {
    if (_closed) return;
    _closed = true;
    final opening = _opening;
    if (opening != null) {
      try {
        await native.wslcClose(sessionId: await opening);
      } catch (_) {
        /* native lock also closes the lease */
      }
    }
  }
}
