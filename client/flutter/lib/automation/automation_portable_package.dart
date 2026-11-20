import 'dart:convert';

import 'package:consolecrypt/core/security/secret_text.dart';

const portablePackageExtension = 'ccproject';
// Native v1 source+manifest+fixed envelope limit; contract test checks the actual Native serializer witness.
const portableMaxEncryptedBytes = 17301600;
const portableMaxExportRequestBytes = 262144;
// Mirrors the v1 Native package tree limits for feedback before the save picker.
const _maxPackageFiles = 1000;
const _maxPackageBytes = 16 * 1024 * 1024;
const _maxFileBytes = 1024 * 1024;
// A picker filter is not validation. Native still validates the entire local
// path, parent, ASCII leaf, no-overwrite and original source/scope independently.
bool portablePackageFilename(String path) {
  final leaf = path.replaceAll('\\', '/').split('/').last;
  return leaf.endsWith('.$portablePackageExtension') && leaf.length > portablePackageExtension.length + 1;
}

/// UI data only. The Native side independently authorizes original profile,
/// scope, exact catalog row + source digest; none of these IDs is permission.
final class PortablePackageFailure implements Exception {
  const PortablePackageFailure(this.code);
  final String code;
  @override
  String toString() => 'PortablePackageFailure($code)';
}

Never _invalid() => throw const PortablePackageFailure('portable_response_invalid');
Map<String, dynamic> _object(Object? value, Set<String> keys) {
  if (value is! Map<String, dynamic> || value.length != keys.length || !value.keys.toSet().containsAll(keys)) {
    _invalid();
  }
  return value;
}

String _text(Object? value, int max) {
  if (value is! String || value.isEmpty || value.length > max) _invalid();
  return value;
}

int _integer(Object? value, int max) {
  if (value is! int || value < 0 || value > max) _invalid();
  return value;
}

String _id(Object? value) {
  final s = _text(value, 36);
  if (!RegExp(r'^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$').hasMatch(s) ||
      s == '00000000-0000-0000-0000-000000000000') {
    _invalid();
  }
  return s;
}

String _hash(Object? value) {
  final s = _text(value, 64);
  if (!RegExp(r'^[0-9a-f]{64}$').hasMatch(s)) _invalid();
  return s;
}

final class PortableScope {
  PortableScope._(this.profileId, this.notificationScopeId);
  factory PortableScope.fromJson(Object? raw) {
    final v = _object(raw, {'version', 'profileId', 'notificationScopeId'});
    if (v['version'] is! int || v['version'] != 1) _invalid();
    return PortableScope._(_id(v['profileId']), _id(v['notificationScopeId']));
  }
  final String profileId, notificationScopeId;
  Map<String, dynamic> toJson() => {'version': 1, 'profileId': profileId, 'notificationScopeId': notificationScopeId};
  bool same(PortableScope other) => profileId == other.profileId && notificationScopeId == other.notificationScopeId;
}

final class PortableChoice {
  PortableChoice._(this.path, this.bytes, this.directory, this.selectable);
  factory PortableChoice.fromJson(Object? raw) {
    final v = _object(raw, {'path', 'bytes', 'directory', 'selectable'});
    final p = _text(v['path'], 1024);
    if (p.contains('\\') ||
        p.startsWith('/') ||
        p.split('/').any((x) => x.isEmpty || x == '.' || x == '..') ||
        v['directory'] is! bool ||
        v['selectable'] is! bool) {
      _invalid();
    }
    return PortableChoice._(
      p,
      _integer(v['bytes'], 100 * 1024 * 1024),
      v['directory'] as bool,
      v['selectable'] as bool,
    );
  }
  final String path;
  final int bytes;
  final bool directory, selectable;
}

final class PortableContext {
  PortableContext._(this.scope, this.projectId, this.projectRevision, this.digest, this.projectStamp, this.choices);
  factory PortableContext.fromJson(Object? raw) {
    final v = _object(raw, {'scope', 'projectId', 'projectRevision', 'revisionDigest', 'projectStamp', 'choices'});
    final rows = v['choices'];
    if (rows is! List || rows.length > 2000) _invalid();
    final choices = [for (final row in rows) PortableChoice.fromJson(row)];
    if (choices.map((e) => e.path).toSet().length != choices.length) _invalid();
    final revision = _integer(v['projectRevision'], 9007199254740991);
    if (revision == 0) _invalid();
    return PortableContext._(
      PortableScope.fromJson(v['scope']),
      _id(v['projectId']),
      revision,
      _hash(v['revisionDigest']),
      _hash(v['projectStamp']),
      List.unmodifiable(choices),
    );
  }
  final PortableScope scope;
  final String projectId, digest, projectStamp;
  final int projectRevision;
  final List<PortableChoice> choices;
  ({int files, int bytes, bool tooLarge}) selectionSize(Set<String> paths) {
    var files = 0, bytes = 0;
    var oversizedFile = false;
    for (final choice in choices) {
      if (choice.directory || !paths.contains(choice.path)) continue;
      files++;
      bytes += choice.bytes;
      oversizedFile |= choice.bytes > _maxFileBytes;
    }
    return (
      files: files,
      bytes: bytes,
      tooLarge: oversizedFile || files > _maxPackageFiles || bytes > _maxPackageBytes,
    );
  }

  Map<String, dynamic> selection(Set<String> paths) {
    if (paths.isEmpty ||
        paths.length > 2000 ||
        !choices.where((e) => e.selectable).map((e) => e.path).toSet().containsAll(paths) ||
        selectionSize(paths).tooLarge) {
      throw const PortablePackageFailure('portable_selection_invalid');
    }
    final request = {
      'scope': scope.toJson(),
      'projectId': projectId,
      'projectRevision': projectRevision,
      'revisionDigest': digest,
      'projectStamp': projectStamp,
      'selectedPaths': paths.toList()..sort(),
      'playbook': null,
    };
    if (utf8.encode(jsonEncode(request)).length > portableMaxExportRequestBytes) {
      throw const PortablePackageFailure('portable_selection_invalid');
    }
    return request;
  }
}

final class PortableExportOutcome {
  PortableExportOutcome._(this.hash, this.bytes);
  factory PortableExportOutcome.fromJson(Object? raw) {
    final v = _object(raw, {'version', 'encryptedSha256', 'bytes'});
    if (v['version'] is! int || v['version'] != 1) _invalid();
    final bytes = _integer(v['bytes'], portableMaxEncryptedBytes);
    if (bytes == 0) _invalid();
    return PortableExportOutcome._(_hash(v['encryptedSha256']), bytes);
  }
  final String hash;
  final int bytes;
}

final class PortableImportPreview {
  PortableImportPreview._(this.hash, this.sourceDigest, this.portableDigest, this.files, this.bytes, this.exclusions);
  factory PortableImportPreview.fromJson(Object? raw) {
    final v = _object(raw, {'version', 'encryptedSha256', 'summary', 'publicationSupported', 'reason'});
    if (v['version'] is! int ||
        v['version'] != 1 ||
        v['publicationSupported'] != false ||
        v['reason'] != 'managed_publication_required') {
      _invalid();
    }
    final s = _object(v['summary'], {'sourceDigest', 'portableDigest', 'files', 'bytes', 'exclusions'});
    return PortableImportPreview._(
      _hash(v['encryptedSha256']),
      _hash(s['sourceDigest']),
      _hash(s['portableDigest']),
      _integer(s['files'], 1000),
      _integer(s['bytes'], 16 * 1024 * 1024),
      _integer(s['exclusions'], 2000),
    );
  }
  final String hash, sourceDigest, portableDigest;
  final int files, bytes, exclusions;
  // Deliberately no install/commit method or authority flag.
}

/// Local cancellation forwarding only, not a global registry/serialized token.
/// The future Native service binds the ACTUAL original operation cancel before
/// its first await and unregisters after joined completion. No fake Bridge yet.
final class PortableCancellation {
  bool _cancelled = false;
  void Function()? _signal;
  bool get cancelled => _cancelled;
  void bind(void Function() signal) {
    if (_signal != null) throw StateError('already bound');
    _signal = signal;
    if (_cancelled) _signalCancellation();
  }

  void unbind() {
    _signal = null;
  }

  // Local retirement must not unwind password wiping/route disposal if the
  // transport disappeared. Native acknowledgement and join remain service-owned.
  void _signalCancellation() {
    try {
      _signal?.call();
    } catch (_) {
      // No raw transport diagnostic or new cancellation authority is retained.
    }
  }

  void cancel() {
    if (_cancelled) return;
    _cancelled = true;
    _signalCancellation();
  }
}

abstract interface class AutomationPortablePackageService {
  Future<PortableContext> context(PortableScope scope, String projectId, int revision, PortableCancellation cancel);

  /// Must copy SecretText into Native SecretString before first suspension. The
  /// supplied destination is an explicit FileDialog selection; Native creates a
  /// new file without overwrite and repeats the original source/scope fences.
  Future<PortableExportOutcome> export(
    Map<String, dynamic> request,
    SecretText password,
    String destination,
    PortableCancellation cancel,
  );
  Future<PortableImportPreview> inspect(
    PortableScope scope,
    String source,
    SecretText password,
    PortableCancellation cancel,
  );
}
