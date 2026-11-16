import 'package:consolecrypt/monitoring/job_context_navigation.dart';
import 'package:consolecrypt/monitoring/monitoring_models.dart';

enum MonitoringHostContextIssue { unsupported, invalidRequest, invalidResponse, changed }

final class MonitoringHostContextException implements Exception {
  const MonitoringHostContextException(this.issue);
  final MonitoringHostContextIssue issue;
  @override
  String toString() => 'MonitoringHostContextException(${issue.name})';
}

/// Optional metadata-only capability on the EXISTING Monitoring service.
/// Native supplies the first original nonce/host revision. No Host.updatedAt,
/// runtime notice scope, automation inventory or latest-profile read is used.
abstract interface class MonitoringHostContextReader {
  bool get hostContextSupported;
  Future<JobContextMetadata> captureHostContext({
    required String profileId,
    required MonitoringSource source,
    required MonitoringBinding binding,
  });
  Future<JobContextMetadata> recheckHostContext(JobContextMetadata original);
}

final _uuid = RegExp(r'^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$');
bool _id(String value) =>
    value.length == 36 && value != '00000000-0000-0000-0000-000000000000' && _uuid.hasMatch(value);
bool _revision(int value) => value > 0 && value <= 9007199254740991;

Json monitoringHostCaptureRequest(String profileId, MonitoringSource source, MonitoringBinding binding) {
  if (!_id(profileId) ||
      !_id(source.id) ||
      !_id(binding.id) ||
      !_id(binding.hostId) ||
      !_revision(source.revision) ||
      !_revision(binding.revision) ||
      source.readOnly ||
      binding.state != MonitoringBindingState.current ||
      binding.sourceId != source.id ||
      binding.sourceRevision != source.revision) {
    throw const MonitoringHostContextException(MonitoringHostContextIssue.invalidRequest);
  }
  return {
    'kind': 'capture',
    'version': 1,
    'profileId': profileId,
    'hostId': binding.hostId,
    'sourceId': source.id,
    'sourceRevision': source.revision,
    'bindingId': binding.id,
    'bindingRevision': binding.revision,
  };
}

Json monitoringHostRecheckRequest(JobContextMetadata original) => {
  'kind': 'recheck',
  'version': 1,
  'profileId': original.profileId,
  'notificationScopeId': original.notificationScopeId,
  'hostId': original.hostId,
  'hostRevision': original.hostRevision,
  'sourceId': original.sourceId,
  'sourceRevision': original.sourceRevision,
  'bindingId': original.bindingId,
  'bindingRevision': original.bindingRevision,
};

bool sameJobContextMetadata(JobContextMetadata a, JobContextMetadata b) =>
    a.profileId == b.profileId &&
    a.notificationScopeId == b.notificationScopeId &&
    a.hostId == b.hostId &&
    a.hostRevision == b.hostRevision &&
    a.protocol == b.protocol &&
    a.sourceId == b.sourceId &&
    a.sourceRevision == b.sourceRevision &&
    a.bindingId == b.bindingId &&
    a.bindingRevision == b.bindingRevision;

JobContextMetadata monitoringHostCaptureResponse(Json response, Json capturedRequest) {
  final metadata = JobContextMetadata.parse(response);
  if (metadata == null || response['protocol'] is! String) {
    throw const MonitoringHostContextException(MonitoringHostContextIssue.invalidResponse);
  }
  if (metadata.profileId != capturedRequest['profileId'] ||
      metadata.hostId != capturedRequest['hostId'] ||
      metadata.sourceId != capturedRequest['sourceId'] ||
      metadata.sourceRevision != capturedRequest['sourceRevision'] ||
      metadata.bindingId != capturedRequest['bindingId'] ||
      metadata.bindingRevision != capturedRequest['bindingRevision']) {
    throw const MonitoringHostContextException(MonitoringHostContextIssue.changed);
  }
  return metadata;
}
