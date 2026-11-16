import 'dart:convert';

/// For separately entered browser addresses only. Imported JSON links and
/// source endpoints must never be fed to this as an automatic destination.
/// This performs no network lookup, launch, persistence or authentication.
Uri? reviewGrafanaBrowserUri(String input) {
  bool control(int r) => r < 32 || r >= 127 && r <= 159 || r >= 0x202a && r <= 0x202e || r >= 0x2066 && r <= 0x2069;
  if (input.length > 4096 || utf8.encode(input).length > 4096 || input.runes.any(control)) return null;
  final value = input.trim();
  if (value.isEmpty || value.runes.any((r) => r <= 32 || r == 92)) return null;
  final authority = RegExp(r'^[A-Za-z][A-Za-z0-9+.-]*://([^/?#]*)').firstMatch(value)?.group(1);
  // Uri canonicalization removes an empty userinfo marker and can normalize
  // encoded host characters. Reject ambiguous raw authority before parsing.
  if (authority == null ||
      authority.isEmpty ||
      authority.contains('@') ||
      authority.contains('%') ||
      authority.endsWith(':')) {
    return null;
  }
  try {
    final decoded = Uri.decodeFull(value);
    if (decoded.runes.any((r) => control(r) || r == 92)) return null;
    final uri = Uri.parse(value);
    if (!const {'http', 'https'}.contains(uri.scheme) ||
        !uri.hasAuthority ||
        uri.host.isEmpty ||
        uri.userInfo.isNotEmpty ||
        uri.authority.contains('@') ||
        uri.authority.contains('%') ||
        uri.authority.endsWith(':') ||
        uri.hasPort && (uri.port < 1 || uri.port > 65535)) {
      return null;
    }
    return uri;
  } on FormatException {
    return null;
    // Uri.decodeFull throws ArgumentError for malformed percent escapes from
    // user input. This narrow expected parse failure must remain a safe rejection.
    // ignore: avoid_catching_errors
  } on ArgumentError {
    return null;
  }
}
