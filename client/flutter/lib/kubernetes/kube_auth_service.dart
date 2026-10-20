import 'dart:convert';
import 'dart:typed_data';

import 'package:consolecrypt/kubernetes/kube_providers.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

/// A transient protected-dialog value. Never put this into providers that
/// outlive the dialog, application logs, diagnostics, history or AI prompts.
final class KubeExecReview {
  KubeExecReview({
    required this.ticket,
    required this.contextName,
    required this.binaryPath,
    required this.binarySha256,
    required this.invocationSha256,
    required this.expiresAt,
    required List<String> arguments,
    required Map<String, String> environment,
  }) : arguments = List.of(arguments),
       environment = Map.of(environment);
  final String ticket, contextName, binaryPath, binarySha256, invocationSha256;
  final DateTime expiresAt;
  final List<String> arguments;
  final Map<String, String> environment;

  factory KubeExecReview.fromBytes(Uint8List bytes) {
    try {
      final data = jsonDecode(utf8.decode(bytes)) as Map<String, dynamic>;
      return KubeExecReview(
        ticket: data['ticket'] as String,
        contextName: data['context_name'] as String,
        binaryPath: data['binary_path'] as String,
        binarySha256: data['binary_sha256'] as String,
        invocationSha256: data['invocation_sha256'] as String,
        expiresAt: DateTime.fromMillisecondsSinceEpoch(data['expires_at_ms'] as int, isUtc: true),
        arguments: (data['arguments'] as List).cast<String>(),
        environment: (data['environment'] as Map).cast<String, String>(),
      );
    } finally {
      bytes.fillRange(0, bytes.length, 0);
    }
  }

  // Dart strings cannot be overwritten. Remove references as soon as the
  // protected dialog closes; the bridge's original byte buffer is wiped above.
  void dispose() {
    arguments.clear();
    environment.clear();
  }
}

final class KubeOidcChallenge {
  const KubeOidcChallenge({
    required this.ticket,
    required this.authorizationUrl,
    required this.redirectUri,
    required this.expiresAt,
    required this.loopback,
  });
  final String ticket;
  final Uri authorizationUrl, redirectUri;
  final DateTime expiresAt;
  final bool loopback;

  factory KubeOidcChallenge.fromBytes(Uint8List bytes) {
    try {
      final data = jsonDecode(utf8.decode(bytes)) as Map<String, dynamic>;
      return KubeOidcChallenge(
        ticket: data['ticket'] as String,
        authorizationUrl: Uri.parse(data['authorization_url'] as String),
        redirectUri: Uri.parse(data['redirect_uri'] as String),
        expiresAt: DateTime.fromMillisecondsSinceEpoch(data['expires_at_ms'] as int, isUtc: true),
        loopback: data['loopback'] as bool,
      );
    } finally {
      bytes.fillRange(0, bytes.length, 0);
    }
  }
}

abstract interface class KubeAuthService {
  Future<KubeExecReview> reviewExec(String contextId);
  Future<void> approveExec(String ticket);
  Future<KubeOidcChallenge> beginOidc(String contextId);
  Future<void> completeOidc(String ticket, {String? callbackUrl});
  Future<void> cancelAuth(String ticket);
}

abstract interface class KubeImportExecService {
  Future<KubeImportExecInspection> inspectImportExec(String ticket, int index);
}

final class KubeImportExecInspection {
  KubeImportExecInspection({
    required this.command,
    required this.arguments,
    required this.environment,
    required this.supported,
    this.binaryPath,
    this.binarySha256,
  });
  final String command;
  final List<String> arguments;
  final Map<String, String> environment;
  final bool supported;
  final String? binaryPath, binarySha256;
  factory KubeImportExecInspection.fromBytes(Uint8List bytes) {
    try {
      final data = jsonDecode(utf8.decode(bytes)) as Map<String, dynamic>;
      return KubeImportExecInspection(
        command: data['command'] as String,
        arguments: (data['arguments'] as List).cast<String>(),
        environment: (data['environment'] as Map).cast<String, String>(),
        supported: data['supported'] as bool,
        binaryPath: data['binary_path'] as String?,
        binarySha256: data['binary_sha256'] as String?,
      );
    } finally {
      bytes.fillRange(0, bytes.length, 0);
    }
  }
  void dispose() {
    arguments.clear();
    environment.clear();
  }
}

final kubeAuthServiceProvider = Provider<KubeAuthService?>((ref) {
  final service = ref.watch(kubeServiceProvider);
  return service is KubeAuthService ? service as KubeAuthService : null;
});
