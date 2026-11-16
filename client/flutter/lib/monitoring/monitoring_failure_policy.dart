import 'package:consolecrypt/monitoring/monitoring_models.dart';

/// Native codes and untrusted Prometheus API error types are separate domains.
/// An API type cannot borrow a Native enum's retry classification.
bool monitoringTransientFailure(Object? error) {
  if (error is! MonitoringFailure) return false;
  final status = error.httpStatus;
  if (status != null && status >= 400 && status < 500 && status != 408 && status != 429) return false;
  if (error.isServerMessage) {
    return const {'timeout', 'canceled', 'internal', 'unavailable', 'execution'}.contains(error.code) &&
        (status == 408 || status == 429 || status != null && status >= 500 && status <= 599);
  }
  switch (error.kind) {
    case MonitoringErrorCode.transport:
    case MonitoringErrorCode.timeout:
    case MonitoringErrorCode.busy:
      return true;
    default:
      return false;
  }
}
