import 'dart:convert';

const grafanaInputByteLimit = 2 * 1024 * 1024;

enum GrafanaInputFailure { tooLarge, invalidUtf8, readFailed }

class GrafanaInputException implements Exception {
  const GrafanaInputException(this.reason);
  final GrafanaInputFailure reason;
}

/// Reads only a user-picked local stream supplied by the host. No path, URL,
/// Native service, logging, persistence or sidecar file is available here.
Future<String?> readGrafanaInput(Stream<List<int>> input, {required bool Function() isCurrent}) async {
  const limit = grafanaInputByteLimit;
  if (!isCurrent()) return null;
  final bytes = <int>[];
  try {
    await for (final chunk in input) {
      if (!isCurrent()) return null;
      if (chunk.length > limit - bytes.length) {
        throw const GrafanaInputException(GrafanaInputFailure.tooLarge);
      }
      bytes.addAll(chunk);
    }
    if (!isCurrent()) return null;
    try {
      return utf8.decode(bytes);
    } on FormatException {
      throw const GrafanaInputException(GrafanaInputFailure.invalidUtf8);
    }
  } on GrafanaInputException {
    rethrow;
  } catch (_) {
    if (!isCurrent()) return null;
    throw const GrafanaInputException(GrafanaInputFailure.readFailed);
  } finally {
    bytes.clear();
  }
}
