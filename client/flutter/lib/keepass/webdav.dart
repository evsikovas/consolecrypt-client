import 'dart:async';
import 'dart:convert';
import 'dart:io';
import 'dart:isolate';
import 'dart:typed_data';

import 'package:consolecrypt/keepass/kdbx_client.dart';
import 'package:crypto/crypto.dart';
import 'package:xml/xml.dart';

typedef DavFile = ({Uint8List bytes, String? etag, String fingerprint});
typedef _Response = ({Uint8List bytes, String? etag, String? token, String? lockTimeout, int status});

/// One encrypted file. Conditional writes use a strong ETag, or an exclusive
/// DAV lock with a comparison against the downloaded ciphertext under that lock.
class KdbxWebDav {
  factory KdbxWebDav(
    Uri uri, {
    String username = '',
    String password = '',
    bool allowLoopbackHttp = false,
    Duration requestTimeout = const Duration(seconds: 20),
  }) {
    if (!isFileUri(uri, allowLoopbackHttp: allowLoopbackHttp)) throw const KdbxFailure('dav_url');
    return KdbxWebDav._(uri, username, password, requestTimeout);
  }
  KdbxWebDav._(this.uri, this.username, this._password, this.requestTimeout);

  static bool isFileUri(Uri uri, {bool allowLoopbackHttp = false}) =>
      (uri.scheme == 'https' ||
          allowLoopbackHttp && ['localhost', '127.0.0.1', '::1'].contains(uri.host) && uri.scheme == 'http') &&
      uri.host.isNotEmpty &&
      uri.userInfo.isEmpty &&
      !uri.hasFragment &&
      uri.path.toLowerCase().endsWith('.kdbx') &&
      uri.pathSegments.last.length > 5;

  static const maxBytes = 128 * 1024 * 1024;
  final Uri uri;
  final String username;
  final Duration requestTimeout;
  String _password;
  HttpClient? _active;
  bool _closed = false;
  void close() {
    _closed = true;
    _password = '';
    _active?.close(force: true);
    _active = null;
  }

  static String? _strongTag(String? value) {
    if (value == null) return null;
    final tag = value.trim();
    if (tag.toLowerCase().startsWith('w/')) return null;
    if (RegExp(r'^"[\x21\x23-\x7e\x80-\xff]*"$').hasMatch(tag)) return tag;
    // Some DAV servers expose a bare hash in GET/getetag. Retain its opaque
    // bytes and add the HTTP entity-tag delimiters for the conditional request.
    if (RegExp(r'^[a-zA-Z0-9._:-]+$').hasMatch(tag)) return '"$tag"';
    return null;
  }

  Future<String> _fingerprint(Uint8List bytes) => Isolate.run(() => sha256.convert(bytes).toString());

  Future<String?> _propertyTag() async {
    try {
      final response = await _request(
        'PROPFIND',
        body: utf8.encode(
          '<d:propfind xmlns:d="DAV:"><d:prop><d:getetag/></d:prop></d:propfind>', // l10n-ignore: WebDAV protocol XML.
        ),
        headers: {'Depth': '0'},
        allowed: const [207],
        metadata: true,
      );
      final document = XmlDocument.parse(utf8.decode(response.bytes));
      for (final item in document.findAllElements('response', namespaceUri: 'DAV:')) {
        final href = item.getElement('href', namespaceUri: 'DAV:')?.innerText.trim();
        final target = href == null ? null : uri.resolve(href);
        if (target == null ||
            target.scheme != uri.scheme ||
            target.host != uri.host ||
            target.port != uri.port ||
            target.path != uri.path) {
          continue;
        }
        for (final properties in item.findElements('propstat', namespaceUri: 'DAV:')) {
          if (!RegExp(r'^HTTP/\S+ 200(?:\s|$)')
              .hasMatch(properties.getElement('status', namespaceUri: 'DAV:')?.innerText.trim() ?? '')) {
            continue;
          }
          final tag = properties
              .getElement('prop', namespaceUri: 'DAV:')
              ?.getElement('getetag', namespaceUri: 'DAV:')
              ?.innerText;
          final strong = _strongTag(tag);
          if (strong != null) return strong;
        }
      }
    } on KdbxFailure catch (e) {
      if (!['dav_http', 'dav_metadata'].contains(e.code)) rethrow;
    } on FormatException {
      // A broken metadata endpoint must not block read-only access.
    } on XmlException {
      // Fall back to HEAD, then to lock-protected writes.
    }
    try {
      return _strongTag((await _request('HEAD', allowed: const [200], metadata: true)).etag);
    } on KdbxFailure catch (e) {
      if (!['dav_http', 'dav_metadata'].contains(e.code)) rethrow;
      return null;
    }
  }

  Future<DavFile> read() async {
    var response = await _request('GET');
    var tag = _strongTag(response.etag);
    if (tag == null) {
      tag = await _propertyTag();
      if (tag != null) {
        // Bind discovered metadata to the actual file bytes. A metadata lookup
        // after GET alone could otherwise attach a newer tag to older contents.
        response = await _request('GET', headers: {HttpHeaders.ifMatchHeader: tag});
        final returned = _strongTag(response.etag);
        if (returned != null && returned != tag) throw const KdbxFailure('changed');
      }
    }
    return (bytes: response.bytes, etag: tag, fingerprint: await _fingerprint(response.bytes));
  }

  Future<DavFile> write(Uint8List bytes, String? etag, {String? fingerprint}) async {
    if (bytes.length > maxBytes) throw const KdbxFailure('limits');
    final tag = _strongTag(etag);
    final digest = await _fingerprint(bytes);
    if (tag == null) {
      if (fingerprint == null) throw const KdbxFailure('dav_etag');
      return _writeLocked(bytes, fingerprint, digest);
    }
    final response = await _request(
      'PUT',
      body: bytes,
      headers: {HttpHeaders.ifMatchHeader: tag},
      allowed: const [200, 201, 204],
    );
    final updated = _strongTag(response.etag);
    if (updated != null) return (bytes: Uint8List(0), etag: updated, fingerprint: digest);
    // PUT often returns 204 without an ETag. Recover the new revision instead
    // of clearing it and making the next autosave fail.
    final refreshed = await read();
    if (refreshed.fingerprint != digest) throw const KdbxFailure('changed');
    return (bytes: Uint8List(0), etag: refreshed.etag, fingerprint: digest);
  }

  Future<DavFile> _writeLocked(Uint8List bytes, String expected, String digest) async {
    final lock = await _request(
      'LOCK',
      body: utf8.encode(
        '<d:lockinfo xmlns:d="DAV:"><d:lockscope><d:exclusive/></d:lockscope><d:locktype><d:write/></d:locktype></d:lockinfo>', // l10n-ignore: WebDAV protocol XML.
      ),
      headers: {'Depth': '0', 'Timeout': 'Second-120', HttpHeaders.ifMatchHeader: '*'},
      allowed: const [200],
      metadata: true,
    );
    var token = lock.token?.trim();
    String? timeout = lock.lockTimeout;
    try {
      final document = XmlDocument.parse(utf8.decode(lock.bytes));
      token ??= document
          .findAllElements('locktoken', namespaceUri: 'DAV:')
          .firstOrNull
          ?.getElement('href', namespaceUri: 'DAV:')
          ?.innerText;
      timeout ??= document.findAllElements('timeout', namespaceUri: 'DAV:').firstOrNull?.innerText;
    } on Object {
      // The HTTP headers may carry both values.
    }
    if (token == null) throw const KdbxFailure('dav_etag');
    token = token.trim();
    timeout = timeout?.trim();
    if (token.startsWith('<') && token.endsWith('>')) token = token.substring(1, token.length - 1);
    if (Uri.tryParse(token)?.hasScheme != true || RegExp(r'[\s<>]').hasMatch(token)) {
      throw const KdbxFailure('dav_etag');
    }
    final lockToken = '<$token>';
    try {
      final seconds = int.tryParse((timeout ?? '').replaceFirst(RegExp(r'^Second-', caseSensitive: false), ''));
      if (seconds == null || seconds < requestTimeout.inSeconds * 2 + 5 || seconds > 600) {
        throw const KdbxFailure('dav_etag');
      }
      final current = await _request('GET', headers: {'If': '($lockToken)'});
      if (await _fingerprint(current.bytes) != expected) throw const KdbxFailure('changed');
      final result = await _request(
        'PUT',
        body: bytes,
        headers: {'If': '($lockToken)'},
        allowed: const [200, 201, 204],
      );
      return (bytes: Uint8List(0), etag: _strongTag(result.etag), fingerprint: digest);
    } finally {
      if (!_closed) {
        try {
          await _request('UNLOCK', headers: {'Lock-Token': lockToken}, allowed: const [200, 204], metadata: true);
        } on Object {
          /* A bounded server lease releases an interrupted unlock. */
        }
      }
    }
  }

  Future<_Response> _request(
    String method, {
    List<int>? body,
    Map<String, String> headers = const {},
    List<int> allowed = const [200],
    bool metadata = false,
  }) async {
    if (_closed) throw const KdbxFailure('closed');
    final client = HttpClient()..connectionTimeout = const Duration(seconds: 10);
    _active = client;
    try {
      return await (() async {
        final request = await client.openUrl(method, uri);
        request.followRedirects = false;
        request.headers.set(
          HttpHeaders.acceptHeader,
          metadata ? 'application/xml, text/xml, */*' : 'application/octet-stream, */*', // l10n-ignore: MIME types.
        );
        request.headers.set(HttpHeaders.acceptEncodingHeader, 'identity');
        if (username.isNotEmpty || _password.isNotEmpty) {
          request.headers.set(
            HttpHeaders.authorizationHeader,
            'Basic ${base64.encode(utf8.encode('$username:$_password'))}',
          );
        }
        headers.forEach(request.headers.set);
        if (body != null) {
          request.headers.contentType = method == 'PUT'
              ? ContentType.binary
              : ContentType('application', 'xml', charset: 'utf-8');
          request.contentLength = body.length;
          request.add(body);
        }
        final response = await request.close();
        if (response.statusCode == 412) throw const KdbxFailure('changed');
        if (response.statusCode == 423) throw const KdbxFailure('dav_locked');
        if (response.statusCode == 401 || response.statusCode == 403) throw const KdbxFailure('dav_auth');
        if (response.statusCode >= 300 && response.statusCode < 400) throw const KdbxFailure('dav_redirect');
        if (!allowed.contains(response.statusCode)) throw KdbxFailure(method == 'LOCK' ? 'dav_etag' : 'dav_http');
        final max = metadata ? 256 * 1024 : maxBytes;
        if (method != 'HEAD' && response.contentLength > max) throw KdbxFailure(metadata ? 'dav_metadata' : 'limits');
        final output = BytesBuilder(copy: false);
        await for (final chunk in response) {
          if (output.length + chunk.length > max) throw KdbxFailure(metadata ? 'dav_metadata' : 'limits');
          output.add(chunk);
        }
        return (
          bytes: output.takeBytes(),
          etag: response.headers.value(HttpHeaders.etagHeader),
          token: response.headers.value('Lock-Token'),
          lockTimeout: response.headers.value('Timeout'),
          status: response.statusCode,
        );
      })().timeout(requestTimeout);
    } on TimeoutException {
      throw const KdbxFailure('dav_timeout');
    } on KdbxFailure {
      rethrow;
    } on Object {
      throw KdbxFailure(_closed ? 'cancelled' : 'dav_io');
    } finally {
      client.close(force: true);
      if (identical(_active, client)) _active = null;
    }
  }
}
