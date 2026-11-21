import 'dart:async';
import 'dart:convert';
import 'dart:io';
import 'dart:typed_data';

import 'package:consolecrypt/core/security/secret_text.dart';

enum ExternalVaultAuth { userpass, token }

class ExternalVaultFailure implements Exception {
  const ExternalVaultFailure(this.code);
  final String code;
  @override
  String toString() => 'ExternalVaultFailure($code)';
}

List<String> vaultPath(String path, {bool optional = false}) {
  if (optional && path.isEmpty) return const [];
  if (path.length > 2048) throw const ExternalVaultFailure('configuration');
  final segments = path.split('/');
  if (segments.any((part) => part.isEmpty || part == '.' || part == '..' || _invalidSegment.hasMatch(part))) {
    throw const ExternalVaultFailure('configuration');
  }
  return segments;
}

final _invalidSegment = RegExp(r'[%\\\x00-\x1f\x7f]');

class ExternalVaultConnection {
  ExternalVaultConnection({
    required this.id,
    required this.profile,
    required this.vault,
    required this.name,
    required String endpoint,
    required this.auth,
    required this.username,
    required this.authMount,
    required this.mount,
    required this.namespace,
    required this.versioned,
    this.wrapped,
  }) : endpoint = _endpoint(endpoint) {
    if (name.trim().isEmpty || name.length > 120) throw const ExternalVaultFailure('configuration');
    vaultPath(mount);
    vaultPath(namespace, optional: true);
    if (namespace.runes.any((value) => value > 126)) throw const ExternalVaultFailure('configuration');
    if (auth == ExternalVaultAuth.userpass) {
      vaultPath(authMount);
      if (username.length > 256 || vaultPath(username).length != 1) {
        throw const ExternalVaultFailure('configuration');
      }
    }
  }

  final String id, profile, vault, name, username, authMount, mount, namespace;
  final Uri endpoint;
  final ExternalVaultAuth auth;
  final bool versioned;
  final String? wrapped;

  static Uri _endpoint(String raw) {
    final uri = Uri.tryParse(raw.trim());
    if (uri == null ||
        uri.host.isEmpty ||
        uri.userInfo.isNotEmpty ||
        uri.hasQuery ||
        uri.hasFragment ||
        (uri.scheme != 'https' && uri.scheme != 'http')) {
      throw const ExternalVaultFailure('configuration');
    }
    final prefix = uri.pathSegments.where((segment) => segment.isNotEmpty).toList();
    if (prefix.any(
      (segment) => segment.contains('/') || segment == '..' || segment == '.' || _invalidSegment.hasMatch(segment),
    )) {
      throw const ExternalVaultFailure('configuration');
    }
    return uri.replace(pathSegments: prefix);
  }

  // Context contains the exact origin, identity and API scope of the saved secret.
  String get binding => jsonEncode([
    profile,
    vault,
    id,
    endpoint.toString(),
    namespace,
    mount,
    versioned,
    auth.name,
    username,
    authMount,
  ]);

  Map<String, String> toStorage({String? cipherText}) => {
    'id': id,
    'profile': profile,
    'vault': vault,
    'name': name,
    'endpoint': endpoint.toString(),
    'auth': auth.name,
    'username': username,
    'authMount': authMount,
    'mount': mount,
    'namespace': namespace,
    'kv': versioned ? '2' : '1',
    'wrapped': ?cipherText,
  };

  factory ExternalVaultConnection.fromStorage(Map<String, String> row) => ExternalVaultConnection(
    id: row['id']!,
    profile: row['profile']!,
    vault: row['vault']!,
    name: row['name']!,
    endpoint: row['endpoint']!,
    auth: ExternalVaultAuth.values.byName(row['auth']!),
    username: row['username']!,
    authMount: row['authMount']!,
    mount: row['mount']!,
    namespace: row['namespace']!,
    versioned: row['kv'] == '2',
    wrapped: row['wrapped'],
  );
}

class ExternalVaultData {
  const ExternalVaultData(this.values, this.version);
  final Map<String, Object?> values;
  final int? version;
}

typedef ExternalVaultMount = ({String path, bool versioned});

/// Only an explicit user action reads or mutates a secret value.
class ExternalVaultSession {
  ExternalVaultSession(this.connection)
    : _http = HttpClient()..connectionTimeout = const Duration(seconds: 12),
      _mount = (path: connection.mount, versioned: connection.versioned);
  final ExternalVaultConnection connection;
  final HttpClient _http;
  ExternalVaultMount _mount;
  ExternalVaultMount get mount => _mount;
  SecretText? _token;
  bool _closed = false;

  bool get closed => _closed;

  void selectMount(ExternalVaultMount mount) {
    vaultPath(mount.path);
    _mount = mount;
  }

  /// This endpoint returns metadata about stores visible to the current token.
  Future<List<ExternalVaultMount>> discoverMounts() async {
    final result = await _request('GET', const ['sys', 'internal', 'ui', 'mounts']);
    final stores = _map(result['data'])['secret'];
    if (stores is! Map || stores.length > 10000) return const [];
    final mounts = <ExternalVaultMount>[];
    for (final entry in stores.entries) {
      final key = entry.key;
      final settings = entry.value;
      if (key is! String || settings is! Map || settings['type'] != 'kv') continue;
      final path = key.endsWith('/') ? key.substring(0, key.length - 1) : key;
      try {
        vaultPath(path);
        final options = settings['options'];
        final version = options is Map ? options['version'] : null;
        if (version != null && version != '1' && version != '2') continue;
        mounts.add((path: path, versioned: version == '2'));
      } on ExternalVaultFailure {
        continue;
      }
    }
    return mounts..sort((a, b) => a.path.compareTo(b.path));
  }

  Future<void> signIn(SecretText credential) async {
    if (credential.isEmpty || credential.byteLength > 16384) throw const ExternalVaultFailure('authentication');
    final value = credential.expose();
    if (connection.auth == ExternalVaultAuth.token) {
      _setToken(value);
      return;
    }
    final result = await _request(
      'POST',
      ['auth', ...vaultPath(connection.authMount), 'login', connection.username],
      payload: {'password': value},
      authenticate: false,
    );
    final auth = result['auth'];
    final token = auth is Map ? auth['client_token'] : null;
    if (token is! String) throw const ExternalVaultFailure('authentication');
    _setToken(token);
  }

  Future<void> verifyToken() async {
    await _request('GET', const ['auth', 'token', 'lookup-self']);
  }

  void _setToken(String value) {
    if (_closed) throw const ExternalVaultFailure('closed');
    if (value.isEmpty || value.length > 16384 || value.runes.any((c) => c <= 32 || c >= 127)) {
      throw const ExternalVaultFailure('authentication');
    }
    _token?.wipe();
    _token = SecretText(value);
  }

  List<String> _kv(String path, {String? section, bool optional = false, ExternalVaultMount? mount}) => [
    ...vaultPath((mount ?? _mount).path),
    if ((mount ?? _mount).versioned && section != null) section,
    ...vaultPath(path, optional: optional),
  ];

  Future<List<String>> list(String path, {ExternalVaultMount? mount}) async {
    final result = await _request(
      'LIST',
      _kv(path, section: 'metadata', optional: true, mount: mount),
      missingAsEmpty: true,
    );
    if (result.isEmpty) return <String>[];
    final keys = _map(result['data'])['keys'];
    if (keys is! List || keys.length > 10000 || keys.any((key) => key is! String)) {
      throw const ExternalVaultFailure('response');
    }
    final valid =
        keys
            .cast<String>()
            .where((key) {
              final label = key.endsWith('/') ? key.substring(0, key.length - 1) : key;
              try {
                return vaultPath(label).length == 1;
              } on ExternalVaultFailure {
                return false;
              }
            })
            .toSet()
            .toList()
          ..sort();
    return valid;
  }

  Future<ExternalVaultData> read(String path, {ExternalVaultMount? mount}) async {
    final versioned = (mount ?? _mount).versioned;
    final response = await _request('GET', _kv(path, section: 'data', mount: mount));
    final payload = _map(response['data']);
    if (!versioned) return ExternalVaultData(payload, null);
    final version = _map(payload['metadata'])['version'];
    if (version is! int || version <= 0) throw const ExternalVaultFailure('response');
    return ExternalVaultData(_map(payload['data']), version);
  }

  Future<void> write(String path, Map<String, Object?> values, {required int? version}) async {
    await _request(
      'POST',
      _kv(path, section: 'data'),
      payload: _mount.versioned
          ? {
              'data': values,
              'options': {'cas': version ?? 0},
            }
          : values,
      compareAndSet: _mount.versioned,
    );
  }

  Future<void> delete(String path, int? version) async {
    if (_mount.versioned) {
      if (version == null || version <= 0) throw const ExternalVaultFailure('response');
      await _request(
        'POST',
        _kv(path, section: 'delete'),
        payload: {
          'versions': [version],
        },
      );
    } else {
      await _request('DELETE', _kv(path));
    }
  }

  static Map<String, Object?> _map(Object? value) {
    if (value is Map<String, Object?>) return value;
    throw const ExternalVaultFailure('response');
  }

  Future<Map<String, Object?>> _request(
    String method,
    List<String> parts, {
    Map<String, Object?>? payload,
    bool authenticate = true,
    bool missingAsEmpty = false,
    bool compareAndSet = false,
  }) async {
    if (_closed || (authenticate && _token == null)) throw const ExternalVaultFailure('closed');
    final uri = connection.endpoint.replace(pathSegments: [...connection.endpoint.pathSegments, 'v1', ...parts]);
    const timeout = Duration(seconds: 20);
    Uint8List? outgoing;
    final incoming = BytesBuilder(copy: false);
    try {
      if (payload != null) {
        outgoing = Uint8List.fromList(utf8.encode(jsonEncode(payload)));
        if (outgoing.length > 4 * 1024 * 1024) throw const ExternalVaultFailure('response');
      }
      final request = await _http.openUrl(method, uri).timeout(timeout);
      if (_closed) throw const ExternalVaultFailure('closed');
      request.followRedirects = false;
      request.headers.set('Accept', 'application/json');
      request.headers.set('X-Vault-Request', 'true');
      if (authenticate) request.headers.set('X-Vault-Token', _token!.expose());
      if (connection.namespace.isNotEmpty) request.headers.set('X-Vault-Namespace', connection.namespace);
      if (outgoing != null) {
        request.headers.contentType = ContentType.json;
        request.add(outgoing);
      }
      final response = await request.close().timeout(timeout);
      await for (final chunk in response.timeout(timeout)) {
        if (incoming.length + chunk.length > 4 * 1024 * 1024) throw const ExternalVaultFailure('response');
        incoming.add(chunk);
      }
      if (_closed) throw const ExternalVaultFailure('closed');
      final status = response.statusCode;
      if (status >= 300 && status < 400) throw const ExternalVaultFailure('redirect');
      if (status == 401 || status == 403) throw ExternalVaultFailure(authenticate ? 'permission' : 'authentication');
      if (status == 404 && missingAsEmpty) return {};
      if (status == 404) throw const ExternalVaultFailure('missing');
      if (status >= 500) throw const ExternalVaultFailure('server');
      if (status < 200 || status >= 300) {
        if (compareAndSet && (status == 400 || status == 409)) throw const ExternalVaultFailure('conflict');
        throw const ExternalVaultFailure('request');
      }
      if (incoming.isEmpty) return {};
      final bytes = incoming.takeBytes();
      try {
        return _map(jsonDecode(utf8.decode(bytes)));
      } finally {
        bytes.fillRange(0, bytes.length, 0);
      }
    } on HandshakeException {
      throw const ExternalVaultFailure('tls');
    } on FormatException {
      throw const ExternalVaultFailure('response');
    } on IOException {
      throw ExternalVaultFailure(_closed ? 'closed' : 'network');
    } on TimeoutException {
      throw const ExternalVaultFailure('network');
    } finally {
      outgoing?.fillRange(0, outgoing.length, 0);
      final retained = incoming.takeBytes();
      retained.fillRange(0, retained.length, 0);
    }
  }

  void close() {
    if (_closed) return;
    _closed = true;
    _http.close(force: true);
    _token?.wipe();
    _token = null;
  }
}
