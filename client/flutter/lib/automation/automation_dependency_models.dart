import 'dart:convert';

import 'package:consolecrypt/automation/automation_service.dart';
import 'package:consolecrypt/core/services/errors.dart';

enum DependencyFailureCode {
  invalidInput,
  invalidResponse,
  changed,
  unavailable,
  busy,
  unsupported,
  cancelled,
  recoveryRequired,
}

final class DependencyInstallerFailure implements Exception {
  const DependencyInstallerFailure(this.code);
  final DependencyFailureCode code;
}

Never _invalid() => throw const DependencyInstallerFailure(
  DependencyFailureCode.invalidResponse,
);
final _uuid = RegExp(
  r'^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$',
);
final _sha = RegExp(r'^[0-9a-f]{64}$');
String dependencyId(Object? value) =>
    value is String &&
        _uuid.hasMatch(value) &&
        value != '00000000-0000-0000-0000-000000000000'
    ? value
    : _invalid();
String dependencyDigest(Object? value) =>
    value is String && _sha.hasMatch(value) ? value : _invalid();
String _text(Object? value, int max) =>
    value is String && value.length <= max ? value : _invalid();
int _count(Object? value, int max, {int min = 0}) =>
    value is int && value >= min && value <= max ? value : _invalid();
bool _bool(Object? value) => value is bool ? value : _invalid();
AutomationJson _map(Object? value, Set<String> fields) {
  if (value is! Map<String, dynamic> ||
      value.length != fields.length ||
      !value.keys.every(fields.contains)) {
    _invalid();
  }
  return value;
}

String? _digestOrNull(Object? value) =>
    value == null ? null : dependencyDigest(value);

/// The transport is bound by the integration to the real generated Native API.
/// No callback here can mint Native acceptance or filesystem authority.
abstract interface class AutomationDependencyService {
  Future<String> dependencyAction(String requestJson);
}

final class DependencyReply {
  const DependencyReply(this.kind, this.data);
  factory DependencyReply.parse(String raw, String expectedKind) {
    if (raw.length > 4 * 1024 * 1024 ||
        utf8.encode(raw).length > 4 * 1024 * 1024) {
      _invalid();
    }
    final value = _map(jsonDecode(raw), {'version', 'kind', 'data'});
    if (value['version'] != 1 ||
        value['version'] is! int ||
        value['kind'] != expectedKind ||
        value['data'] is! Map<String, dynamic>) {
      _invalid();
    }
    return DependencyReply(
      expectedKind,
      Map<String, dynamic>.unmodifiable(value['data'] as AutomationJson),
    );
  }
  final String kind;
  final AutomationJson data;
}

final class DependencyActionClient {
  const DependencyActionClient(this.service);
  final AutomationDependencyService service;
  Future<DependencyReply> call(AutomationJson action, String kind) async {
    final raw = jsonEncode({'version': 1, 'action': action});
    if (utf8.encode(raw).length > 8192) {
      throw const DependencyInstallerFailure(
        DependencyFailureCode.invalidInput,
      );
    }
    try {
      return DependencyReply.parse(await service.dependencyAction(raw), kind);
    } on DependencyInstallerFailure {
      rethrow;
    } on AppException catch (error) {
      // Only existing structured Native fixed codes, never display backend text.
      final code = error.args['field'] == 'automation_dependency'
          ? error.args['rule']
          : null;
      throw DependencyInstallerFailure(switch (code) {
        'dependency_publication_recovery_required' =>
          DependencyFailureCode.recoveryRequired,
        'dependency_publication_unsupported' ||
        'dependency_existing_receipt_unsupported' =>
          DependencyFailureCode.unsupported,
        'dependency_busy' ||
        'dependency_publication_busy' => DependencyFailureCode.busy,
        'dependency_context_changed' ||
        'dependency_review_changed' ||
        'dependency_review_expired' => DependencyFailureCode.changed,
        'dependency_cancelled' => DependencyFailureCode.cancelled,
        'dependency_invalid_request' ||
        'dependency_invalid_source' => DependencyFailureCode.invalidInput,
        _ => DependencyFailureCode.unavailable,
      });
    } catch (_) {
      throw const DependencyInstallerFailure(DependencyFailureCode.unavailable);
    }
  }
}

final class DependencyProposal {
  const DependencyProposal({
    required this.kind,
    required this.name,
    required this.version,
    required this.url,
    required this.archiveSha256,
  });
  final String kind, name, version, url, archiveSha256;
  AutomationJson toJson() => {
    'kind': kind,
    'name': name,
    'version': version,
    'url': url,
    'archiveSha256': archiveSha256,
  };
  bool matchesNative(Object? raw) {
    final value = _map(raw, {
      'kind',
      'name',
      'version',
      'source',
      'strip_components',
    });
    final source = _map(value['source'], {'url', 'sha256'});
    return value['kind'] == kind &&
        value['name'] == name &&
        value['version'] == version &&
        value['strip_components'] == 0 &&
        source['url'] == url &&
        source['sha256'] == archiveSha256;
  }
}

final class NativeDependencyReview {
  NativeDependencyReview.fromJson(AutomationJson raw, this.proposal) {
    final value = _map(raw, {
      'version',
      'reviewId',
      'operationId',
      'projectId',
      'projectRevision',
      'runnerId',
      'runnerRevision',
      'source',
      'baseDigest',
      'oldLockSha256',
      'destination',
      'networkOrigin',
      'publicationSupported',
      'publicationReason',
      'expiresInSeconds',
    });
    if (value['version'] != 1 ||
        value['version'] is! int ||
        !proposal.matchesNative(value['source'])) {
      _invalid();
    }
    reviewId = dependencyId(value['reviewId']);
    operationId = dependencyId(value['operationId']);
    projectId = dependencyId(value['projectId']);
    runnerId = dependencyId(value['runnerId']);
    projectRevision = _count(
      value['projectRevision'],
      0x7fffffffffffffff,
      min: 1,
    );
    runnerRevision = _count(
      value['runnerRevision'],
      0x7fffffffffffffff,
      min: 1,
    );
    baseDigest = dependencyDigest(value['baseDigest']);
    oldLockSha256 = _digestOrNull(value['oldLockSha256']);
    destination = _text(value['destination'], 1024);
    networkOrigin = _text(value['networkOrigin'], 2048);
    if (!networkOrigin.startsWith('https://') ||
        networkOrigin.length <= 8 ||
        networkOrigin
            .substring(8)
            .codeUnits
            .any(
              (c) =>
                  c <= 32 ||
                  c >= 127 ||
                  c == 47 ||
                  c == 64 ||
                  c == 63 ||
                  c == 35 ||
                  c == 92,
            )) {
      _invalid();
    }
    publicationSupported = _bool(value['publicationSupported']);
    publicationReason = value['publicationReason'] as String?;
    if (publicationSupported
        ? publicationReason != null
        : !{
            'managed_project_required',
            'publication_location_unavailable',
            'excluded_project_entries',
            'publication_platform_unsupported',
          }.contains(publicationReason)) {
      _invalid();
    }
    expiresInSeconds = _count(value['expiresInSeconds'], 300, min: 1);
  }
  final DependencyProposal proposal;
  late final String reviewId,
      operationId,
      projectId,
      runnerId,
      baseDigest,
      destination,
      networkOrigin;
  late final String? oldLockSha256, publicationReason;
  late final int projectRevision, runnerRevision, expiresInSeconds;
  late final bool publicationSupported;
}

final class NativeDependencyOperation {
  NativeDependencyOperation.fromJson(AutomationJson raw) {
    final value = _map(raw, {
      'version',
      'operationId',
      'phase',
      'error',
      'cleanup',
      'archiveVerified',
      'baseDigest',
      'candidateDigest',
      'reviewDigest',
      'changeCount',
      'changes',
      'changesTruncated',
    });
    if (value['version'] != 1 || value['version'] is! int) _invalid();
    operationId = dependencyId(value['operationId']);
    phase = _text(value['phase'], 32);
    if (!{
      'opening',
      'preparing',
      'archiveVerified',
      'candidatePreview',
      'accepted',
      'publishing',
      'published',
      'recoveryRequired',
      'rejected',
      'cancelled',
      'contextChanged',
      'expired',
      'failed',
    }.contains(phase)) {
      _invalid();
    }
    archiveVerified = _bool(value['archiveVerified']);
    candidateDigest = _digestOrNull(value['candidateDigest']);
    reviewDigest = _digestOrNull(value['reviewDigest']);
    _digestOrNull(value['baseDigest']);
    _count(value['changeCount'], 20000);
    if (value['changes'] is! List ||
        (value['changes'] as List).length > 100 ||
        value['error'] != null && value['error'] is! String) {
      _invalid();
    }
    _bool(value['changesTruncated']);
    if (!{
      'notStarted',
      'closing',
      'localCloseCompleted',
      'remoteStopUnconfirmed',
    }.contains(value['cleanup'])) {
      _invalid();
    }
    error = value['error'] as String?;
  }
  late final String operationId, phase;
  late final String? candidateDigest, reviewDigest, error;
  late final bool archiveVerified;
}

final class DependencyCandidateEvidence {
  DependencyCandidateEvidence.fromJson(
    AutomationJson raw,
    NativeDependencyReview review,
    NativeDependencyOperation operation,
  ) {
    final value = _map(raw, {
      'reviewDigest',
      'baseDigest',
      'oldLockSha256',
      'candidateDigest',
      'candidateLockSha256',
      'candidateReceiptSha256',
      'archiveSha256',
      'archiveTreeDigest',
      'source',
      'destination',
      'archiveVerified',
      'receiptMergeSupported',
      'published',
      'executionEnabled',
    });
    reviewDigest = dependencyDigest(value['reviewDigest']);
    candidateDigest = dependencyDigest(value['candidateDigest']);
    lockSha256 = dependencyDigest(value['candidateLockSha256']);
    receiptSha256 = dependencyDigest(value['candidateReceiptSha256']);
    archiveTreeDigest = dependencyDigest(value['archiveTreeDigest']);
    if (reviewDigest != operation.reviewDigest ||
        candidateDigest != operation.candidateDigest ||
        value['baseDigest'] != review.baseDigest ||
        value['oldLockSha256'] != review.oldLockSha256 ||
        value['archiveSha256'] != review.proposal.archiveSha256 ||
        !review.proposal.matchesNative(value['source']) ||
        value['destination'] != review.destination ||
        value['archiveVerified'] != true ||
        value['receiptMergeSupported'] != false ||
        value['published'] != false ||
        value['executionEnabled'] != false) {
      _invalid();
    }
  }
  late final String reviewDigest,
      candidateDigest,
      lockSha256,
      receiptSha256,
      archiveTreeDigest;
}

final class DependencyPublished {
  DependencyPublished.fromJson(
    AutomationJson raw,
    NativeDependencyReview review,
    DependencyCandidateEvidence evidence,
  ) {
    final value = _map(raw, {
      'version',
      'operationId',
      'projectId',
      'revision',
      'transaction',
      'candidateDigest',
      'durability',
    });
    if (value['version'] != 1 ||
        value['version'] is! int ||
        value['operationId'] != review.operationId ||
        value['projectId'] != review.projectId ||
        value['revision'] != review.projectRevision + 1 ||
        value['candidateDigest'] != evidence.candidateDigest ||
        !{
          'WindowsFileFlushProcessRecovery',
          'LinuxDirectoryFsync',
          'MacosFileFullSyncDirectoryFsync',
        }.contains(value['durability'])) {
      _invalid();
    }
    transaction = dependencyId(value['transaction']);
    revision = _count(value['revision'], 0x7fffffffffffffff, min: 2);
  }
  late final String transaction;
  late final int revision;
}
