import 'dart:convert';

import 'package:consolecrypt/automation/automation_portable_package.dart';
import 'package:consolecrypt/core/security/secret_text.dart';

const portableImportMaxRemainingMillis = 300000;
const portableImportMaxRawNameBytes = 1024;
const portableImportMaxReplyBytes = 16384;

enum PortableImportIssue {
  unavailable,
  invalidInput,
  archiveFormat,
  archiveSize,
  archiveRead,
  changed,
  sourceChanged,
  unsupported,
  busy,
  expired,
  cancelled,
  response,
  failed,
  recoveryRequired,
}

final class PortableImportFailure implements Exception {
  const PortableImportFailure(this.issue);
  final PortableImportIssue issue;
  @override
  String toString() => 'PortableImportFailure(${issue.name})';
}

Never _bad() => throw const PortableImportFailure(PortableImportIssue.response);
Map<String, dynamic> _object(Object? value, Set<String> fields) {
  if (value is! Map<String, dynamic> || value.length != fields.length || !value.keys.every(fields.contains)) _bad();
  return value;
}

String _id(Object? value) {
  if (value is! String ||
      value.length != 36 ||
      value == '00000000-0000-0000-0000-000000000000' ||
      !RegExp(r'^[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}$').hasMatch(value)) {
    _bad();
  }
  return value;
}

String _hash(Object? value) {
  if (value is! String || value.length != 64 || !RegExp(r'^[a-f0-9]{64}$').hasMatch(value)) _bad();
  return value;
}

int _integer(Object? value, int max, {int min = 0}) {
  if (value is! int || value < min || value > max) _bad();
  return value;
}

String _name(Object? value) {
  if (value is! String ||
      value.isEmpty ||
      value.runes.length > 120 ||
      value.runes.any((r) => r < 32 || (r >= 127 && r <= 159))) {
    _bad();
  }
  return value; // Native normalization is displayed, never reimplemented here.
}

Map<String, dynamic> portableImportDecode(String raw) {
  if (utf8.encode(raw).length > portableImportMaxReplyBytes) _bad();
  try {
    final value = jsonDecode(raw);
    if (value is! Map<String, dynamic>) _bad();
    return value;
  } catch (_) {
    _bad();
  }
}

PortableScope _scope(Object? value) {
  try {
    return PortableScope.fromJson(value);
  } catch (_) {
    _bad();
  }
}

/// Captured input DATA, not a bearer approval or destination grant. No caller
/// project ID/target directory/transaction/review UUID is accepted.
final class PortableImportIntent {
  PortableImportIntent._(this.scope, this.source, this.requestedName);
  factory PortableImportIntent.capture({
    required PortableScope scope,
    required String source,
    required String requestedName,
  }) {
    if (!portablePackageFilename(source) ||
        requestedName.isEmpty ||
        requestedName.length > portableImportMaxRawNameBytes ||
        utf8.encode(requestedName).length > portableImportMaxRawNameBytes) {
      throw const PortableImportFailure(PortableImportIssue.invalidInput);
    }
    return PortableImportIntent._(scope, source, requestedName);
  }
  final PortableScope scope;
  final String source, requestedName;
  // Deliberately no toJson, Debug payload or local authority constructor.
}

final class PortableImportSummary {
  PortableImportSummary._(this.sourceDigest, this.portableDigest, this.files, this.bytes, this.exclusions);
  factory PortableImportSummary.fromJson(Object? value) {
    final v = _object(value, {'sourceDigest', 'portableDigest', 'files', 'bytes', 'exclusions'});
    return PortableImportSummary._(
      _hash(v['sourceDigest']),
      _hash(v['portableDigest']),
      _integer(v['files'], 1000),
      _integer(v['bytes'], 16 * 1024 * 1024),
      _integer(v['exclusions'], 2000),
    );
  }
  final String sourceDigest, portableDigest;
  final int files, bytes, exclusions;
}

final class PortableImportReady {
  PortableImportReady._(
    this.scope,
    this.reviewId,
    this.reviewDigest,
    this.projectId,
    this.name,
    this.requestedName,
    this.encryptedSha256,
    this.summary,
    this.publicationSupported,
    this.publicationReason,
    this.remainingMillis,
  );
  factory PortableImportReady.fromJson(Object? value, {required PortableImportIntent intent}) {
    final v = _object(value, {
      'version',
      'scope',
      'reviewId',
      'reviewDigest',
      'projectId',
      'name',
      'requestedName',
      'encryptedSha256',
      'summary',
      'publicationSupported',
      'publicationReason',
      'remainingMillis',
    });
    if (v['version'] is! int ||
        v['version'] != 1 ||
        v['publicationSupported'] is! bool ||
        v['requestedName'] is! String ||
        v['requestedName'] != intent.requestedName ||
        utf8.encode(v['requestedName'] as String).length > portableImportMaxRawNameBytes) {
      _bad();
    }
    final scope = _scope(v['scope']);
    if (!scope.same(intent.scope)) throw const PortableImportFailure(PortableImportIssue.changed);
    final supported = v['publicationSupported'] as bool, reason = v['publicationReason'];
    const reasons = {
      'publication_platform_unsupported',
      'publication_location_unavailable',
      'package_tree_unsupported',
      'import_capacity_reached',
    };
    if (supported ? reason != null : reason is! String || !reasons.contains(reason)) _bad();
    return PortableImportReady._(
      scope,
      _id(v['reviewId']),
      _hash(v['reviewDigest']),
      _id(v['projectId']),
      _name(v['name']),
      v['requestedName'] as String,
      _hash(v['encryptedSha256']),
      PortableImportSummary.fromJson(v['summary']),
      supported,
      reason as String?,
      _integer(v['remainingMillis'], portableImportMaxRemainingMillis, min: 1),
    );
  }
  final PortableScope scope;
  final String reviewId, reviewDigest, projectId, name, requestedName, encryptedSha256;
  final PortableImportSummary summary;
  final bool publicationSupported;
  final String? publicationReason;
  final int remainingMillis; // display hint only; no renewal/permission clock
}

final class PortableInstalledProject {
  PortableInstalledProject._(
    this.scope,
    this.reviewId,
    this.projectId,
    this.name,
    this.projectRevision,
    this.revisionDigest,
    this.encryptedSha256,
  );
  factory PortableInstalledProject.fromJson(Object? value, {required PortableImportReady reviewed}) {
    final v = _object(value, {
      'version',
      'state',
      'scope',
      'reviewId',
      'projectId',
      'name',
      'projectRevision',
      'revisionDigest',
      'encryptedSha256',
    });
    if (v['version'] is! int || v['version'] != 1 || v['state'] != 'installed') _bad();
    final scope = _scope(v['scope']);
    if (!scope.same(reviewed.scope) ||
        v['reviewId'] != reviewed.reviewId ||
        v['projectId'] != reviewed.projectId ||
        v['name'] != reviewed.name ||
        v['revisionDigest'] != reviewed.summary.portableDigest ||
        v['encryptedSha256'] != reviewed.encryptedSha256 ||
        v['projectRevision'] is! int ||
        v['projectRevision'] != 3) {
      _bad();
    }
    return PortableInstalledProject._(
      scope,
      _id(v['reviewId']),
      _id(v['projectId']),
      _name(v['name']),
      3,
      _hash(v['revisionDigest']),
      _hash(v['encryptedSha256']),
    );
  }
  final PortableScope scope;
  final String reviewId, projectId, name, revisionDigest, encryptedSha256;
  final int projectRevision;
}

/// ORIGINAL opaque Native owner. preview returns while its genuine witness is
/// still retained. accept uses ONLY refs captured from that same Ready value,
/// never JSON approval, replacement name/path/project ID or a latest context.
abstract interface class PortableImportReviewSession {
  Future<String> preview();
  Future<String> accept({required String reviewId, required String reviewDigest});
  void cancel();
}

/// Cancellation is bound BEFORE the first asynchronous suspension. The actual
/// gateway retains a sync password transfer during pending ACK; its cancel is
/// sticky across take/preACK and a late issued handle is retired before preview.
/// joined is REAL retirement, including Ready witness cleanup/expiry, not the
/// preview result and never a fabricated transport-error acknowledgement.
abstract interface class PortableImportRegistration {
  Future<PortableImportReviewSession> get registered;
  Future<void> get joined;
  void cancel();
}

abstract interface class PortableImportReviewGateway {
  /// Synchronously copy bounded SecretText to its own Native transfer before
  /// return. No plaintext/password JSON, global registry or generated guesses.
  PortableImportRegistration register({required PortableImportIntent intent, required SecretText password});
}

/// Optional until the real Native session/generated bridge has passed its gates.
/// This stage does not implement this capability on the active production service.
abstract interface class AutomationPortableImportReviewCapability {
  PortableImportReviewGateway createPortableImportReviewGateway();
}
