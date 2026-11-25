import 'dart:async';
import 'dart:convert';
import 'dart:io';
import 'dart:typed_data';

import 'package:consolecrypt/src/rust/api/ssh.dart' as ssh;
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:html/parser.dart' as html;
import 'package:url_launcher/url_launcher.dart';

typedef WebOrigin = ({Uri uri, String? sourceId});

final portBrowserProvider = Provider<Future<bool> Function(Uri)>(
  (ref) =>
      (uri) => launchUrl(uri, mode: LaunchMode.externalApplication),
);

class PortWebPage {
  const PortWebPage({this.status, this.mime = '', this.title = '', this.description = '', this.error});

  final int? status;
  final String mime, title, description;
  final String? error;
}

const _limit = 32768;

PortWebPage describePortPage(int status, String mime, String charset, List<int> body) {
  String clean(String text, int limit) => String.fromCharCodes(
    text
        .replaceAll(RegExp(r'[\x00-\x1f\u007f\u202a-\u202e\u2066-\u2069]'), ' ')
        .replaceAll(RegExp(r'\s+'), ' ')
        .trim()
        .runes
        .take(limit),
  );
  if (status < 100 || status > 599) return const PortWebPage(error: 'no_response');
  final media = clean(mime, 100).toLowerCase();
  if (body.isEmpty || (media.isNotEmpty && media != 'text/html' && media != 'application/xhtml+xml')) {
    return PortWebPage(status: status, mime: media);
  }
  final bytes = body.length > _limit ? body.sublist(0, _limit) : body;
  final text = Encoding.getByName(charset) == latin1 ? latin1.decode(bytes) : utf8.decode(bytes, allowMalformed: true);
  final head = html.parse(text).head;
  final meta = head?.querySelectorAll('meta') ?? [];
  String metadata(String name) {
    for (final item in meta) {
      if ((item.attributes['name'] ?? item.attributes['property'])?.toLowerCase() == name) {
        final content = clean(item.attributes['content'] ?? '', 320);
        if (content.isNotEmpty) return content;
      }
    }
    return '';
  }

  final title = clean(head?.querySelector('title')?.text ?? '', 140);
  final description = metadata('description');
  return PortWebPage(
    status: status,
    mime: media,
    title: title.isNotEmpty ? title : clean(metadata('og:title'), 140),
    description: description.isNotEmpty ? description : metadata('og:description'),
  );
}

Future<PortWebPage> _localPage(HttpClient client, Uri uri) async {
  final request = await client.getUrl(uri);
  request.followRedirects = false;
  request.headers.set(HttpHeaders.acceptEncodingHeader, 'identity');
  final response = await request.close();
  final type = response.headers.contentType;
  final mime = type?.mimeType.toLowerCase() ?? '';
  final bytes = BytesBuilder(copy: false);
  if ((mime.isEmpty || mime == 'text/html' || mime == 'application/xhtml+xml') &&
      (response.headers.value(HttpHeaders.contentEncodingHeader) ?? 'identity') == 'identity') {
    var tail = '';
    await for (final chunk in response) {
      final n = (_limit - bytes.length).clamp(0, chunk.length);
      final part = n == chunk.length ? chunk : chunk.sublist(0, n);
      bytes.add(part);
      final slice = tail + latin1.decode(part).toLowerCase();
      if (bytes.length == _limit || slice.contains('</head>')) break;
      tail = slice.length > 6 ? slice.substring(slice.length - 6) : slice;
    }
  }
  return describePortPage(response.statusCode, mime, type?.charset ?? '', bytes.takeBytes());
}

final portWebPageProvider = FutureProvider.autoDispose.family<PortWebPage, WebOrigin>((ref, origin) async {
  final uri = origin.uri;
  if (!{'http', 'https'}.contains(uri.scheme) ||
      uri.userInfo.isNotEmpty ||
      uri.path != '/' ||
      uri.hasQuery ||
      uri.hasFragment ||
      uri.host.isEmpty ||
      uri.port < 1 ||
      uri.port > 65535) {
    return const PortWebPage(error: 'no_response');
  }
  try {
    if (origin.sourceId case final id?) {
      final raw = await ssh.portWebPreview(sourceId: id, host: uri.host, port: uri.port, https: uri.scheme == 'https');
      final data = jsonDecode(raw) as Map<String, dynamic>;
      if (data['error'] case final String error) return PortWebPage(error: error);
      return describePortPage(
        data['status'] as int,
        data['mime'] as String,
        data['charset'] as String,
        base64Decode(data['body'] as String),
      );
    }
    final client = HttpClient()..connectionTimeout = const Duration(seconds: 4);
    ref.onDispose(() => client.close(force: true));
    try {
      return await _localPage(client, uri).timeout(const Duration(seconds: 6));
    } finally {
      client.close(force: true);
    }
  } on HandshakeException {
    return const PortWebPage(error: 'tls');
  } on TimeoutException {
    return const PortWebPage(error: 'timeout');
  } catch (_) {
    return const PortWebPage(error: 'no_response');
  }
});
