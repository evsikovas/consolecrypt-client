import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:consolecrypt/ai/local/local_ai_catalog.dart';
import 'package:crypto/crypto.dart';

class LocalAiException implements Exception {
  const LocalAiException(this.code);
  final String code;
  @override
  String toString() => 'LocalAiException($code)';
}

class LocalAiDownloadCancelled extends LocalAiException {
  const LocalAiDownloadCancelled() : super('download_cancelled');
}

class LocalAiCancellation {
  bool _cancelled = false;
  final Set<void Function()> _listeners = {};
  bool get isCancelled => _cancelled;
  void cancel() {
    if (_cancelled) return;
    _cancelled = true;
    for (final listener in _listeners.toList()) {
      listener();
    }
  }

  void check() {
    if (_cancelled) throw const LocalAiDownloadCancelled();
  }
}

/// Downloads to a resumable partial file and publishes only verified content.
/// No shell, external download program, bearer credentials, or cookies are used.
class LocalAiDownloader {
  const LocalAiDownloader({this.allowHttpLoopbackForTests = false});
  final bool allowHttpLoopbackForTests;

  Future<File> download(
    LocalAiArtifact artifact,
    Directory cache, {
    required LocalAiCancellation cancellation,
    void Function(int received, int total)? onProgress,
  }) async {
    final progressClock = Stopwatch()..start();
    var lastProgress = -100;
    void progress(int received, {bool force = false}) {
      if (force || progressClock.elapsedMilliseconds - lastProgress >= 100) {
        lastProgress = progressClock.elapsedMilliseconds;
        onProgress?.call(received, artifact.bytes);
      }
    }

    cancellation.check();
    await cache.create(recursive: true);
    final complete = File('${cache.path}/${artifact.sha256}');
    final partial = File('${complete.path}.part');
    final metadata = File('${partial.path}.json');
    final identity = jsonEncode({'url': artifact.url, 'sha256': artifact.sha256, 'bytes': artifact.bytes});
    if (complete.existsSync()) {
      if (await verify(complete, artifact, cancellation: cancellation)) {
        progress(artifact.bytes, force: true);
        return complete;
      }
      await complete.delete();
    }
    if (!metadata.existsSync() || await metadata.readAsString() != identity) {
      if (partial.existsSync()) await partial.delete();
      await metadata.writeAsString(identity, flush: true);
    }
    var offset = partial.existsSync() ? await partial.length() : 0;
    if (offset > artifact.bytes) {
      await partial.delete();
      offset = 0;
    }
    if (offset < artifact.bytes) {
      final client = HttpClient()
        ..connectionTimeout = const Duration(seconds: 30)
        ..autoUncompress = false;
      void abort() => client.close(force: true);
      cancellation._listeners.add(abort);
      RandomAccessFile? output;
      try {
        var uri = Uri.parse(artifact.url);
        HttpClientResponse? response;
        for (var redirects = 0; redirects <= 8; redirects++) {
          cancellation.check();
          _checkUrl(uri);
          final request = await client.getUrl(uri).timeout(const Duration(seconds: 40));
          request.followRedirects = false;
          request.headers.set(HttpHeaders.acceptEncodingHeader, 'identity');
          if (offset > 0) request.headers.set(HttpHeaders.rangeHeader, 'bytes=$offset-');
          response = await request.close().timeout(const Duration(seconds: 60));
          if (![301, 302, 303, 307, 308].contains(response.statusCode)) break;
          final location = response.headers.value(HttpHeaders.locationHeader);
          if (location == null || redirects == 8) throw const LocalAiException('download_redirect');
          await response.drain<void>();
          uri = uri.resolve(location);
        }
        cancellation.check();
        if (response == null || (response.statusCode != 200 && response.statusCode != 206)) {
          throw const LocalAiException('download_http_error');
        }
        if (response.statusCode == 200) {
          offset = 0; // Server ignored Range: replace, never append a second copy.
        } else {
          final range = response.headers.value(HttpHeaders.contentRangeHeader);
          final match = RegExp(r'^bytes (\d+)-(\d+)/(\d+)$').firstMatch(range ?? '');
          if (match == null ||
              int.parse(match[1]!) != offset ||
              int.parse(match[3]!) != artifact.bytes ||
              int.parse(match[2]!) != artifact.bytes - 1) {
            throw const LocalAiException('download_invalid_range');
          }
        }
        if (response.contentLength >= 0 && response.contentLength != artifact.bytes - offset) {
          throw const LocalAiException('download_size_mismatch');
        }
        output = await partial.open(mode: offset == 0 ? FileMode.write : FileMode.append);
        progress(offset, force: true);
        await for (final bytes in response.timeout(const Duration(seconds: 60))) {
          cancellation.check();
          if (offset + bytes.length > artifact.bytes) throw const LocalAiException('download_size_mismatch');
          await output.writeFrom(bytes);
          offset += bytes.length;
          progress(offset);
        }
        await output.flush();
      } catch (_) {
        cancellation.check();
        rethrow;
      } finally {
        await output?.close();
        cancellation._listeners.remove(abort);
        client.close(force: true);
      }
    }
    cancellation.check();
    if (partial.existsSync() && await partial.length() < artifact.bytes) {
      // A clean but prematurely terminated chunked response is resumable too.
      throw const LocalAiException('download_incomplete');
    }
    if (!await verify(partial, artifact, cancellation: cancellation)) {
      if (partial.existsSync()) await partial.delete();
      if (metadata.existsSync()) await metadata.delete();
      throw const LocalAiException('download_checksum_mismatch');
    }
    cancellation.check();
    await partial.rename(complete.path);
    await metadata.delete();
    progress(artifact.bytes, force: true);
    return complete;
  }

  static Future<bool> verify(File file, LocalAiArtifact artifact, {LocalAiCancellation? cancellation}) async {
    if (!file.existsSync() || await file.length() != artifact.bytes) return false;
    cancellation?.check();
    final digest = await sha256
        .bind(
          file.openRead().map((chunk) {
            cancellation?.check();
            return chunk;
          }),
        )
        .first;
    return digest.toString() == artifact.sha256;
  }

  void _checkUrl(Uri uri) {
    final local = allowHttpLoopbackForTests && uri.scheme == 'http' && uri.host == '127.0.0.1';
    final officialHost =
        uri.host == 'github.com' ||
        uri.host == 'release-assets.githubusercontent.com' ||
        uri.host == 'objects.githubusercontent.com' ||
        uri.host == 'huggingface.co' ||
        uri.host.endsWith('.huggingface.co') ||
        uri.host.endsWith('.hf.co');
    if (uri.userInfo.isNotEmpty || (!local && (uri.scheme != 'https' || !officialHost))) {
      throw const LocalAiException('download_untrusted_url');
    }
  }
}
