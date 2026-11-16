import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/monitoring/monitoring_models.dart';

/// Local and Native diagnostics never become untranslated UI copy. Actual
/// Prometheus API errors retain their message as untrusted server-provided data,
/// alongside the localized explanation (the same treatment as API warnings).
String monitoringFailureMessage(AppLocalizations l, MonitoringFailure failure) {
  final label = switch (failure.kind) {
    MonitoringErrorCode.invalidBinding => l.monitoringBindingInvalid,
    MonitoringErrorCode.bindingChanged => l.monitoringBindingChanged,
    MonitoringErrorCode.sourceInUse => l.monitoringBindingSourceInUse,
    MonitoringErrorCode.authRequiresTls => l.monitoringHttpAuth,
    MonitoringErrorCode.invalidConfig => l.monitoringErrorConfig,
    MonitoringErrorCode.invalidQuery => l.monitoringErrorQuery,
    MonitoringErrorCode.authUnavailable => l.monitoringErrorAuth,
    MonitoringErrorCode.transport => l.monitoringErrorTransport,
    MonitoringErrorCode.timeout => l.monitoringErrorTimeout,
    MonitoringErrorCode.budgetExceeded ||
    MonitoringErrorCode.responseTooLarge ||
    MonitoringErrorCode.busy => l.monitoringErrorLimit,
    MonitoringErrorCode.sourceChanged => l.monitoringErrorSourceChanged,
    MonitoringErrorCode.unsupportedRoute => l.monitoringTunnelUnavailable,
    MonitoringErrorCode.cancelled => l.monitoringErrorCancelled,
    MonitoringErrorCode.invalidResponse || MonitoringErrorCode.unsupportedResult => l.monitoringErrorResponse,
    MonitoringErrorCode.readOnly => l.monitoringReadOnly,
    MonitoringErrorCode.unsupportedCatalogVersion => l.monitoringErrorCatalogVersion,
    MonitoringErrorCode.unknown => l.monitoringErrorServer,
  };
  if (failure.isServerMessage && failure.message.isNotEmpty) {
    return l.monitoringFailureDetails(label, failure.httpStatus?.toString() ?? '—', failure.message);
  }
  return failure.httpStatus == null ? label : l.monitoringResultStatus(label, failure.httpStatus.toString());
}
