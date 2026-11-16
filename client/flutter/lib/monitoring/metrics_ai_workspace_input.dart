import 'package:consolecrypt/monitoring/metrics_ai_context.dart';
import 'package:consolecrypt/monitoring/metrics_ai_review_dialog.dart';
import 'package:consolecrypt/monitoring/monitoring_alert_models.dart';

/// In-memory selection handoff to the existing explicit review dialog. Never a
/// router extra, durable record, provider request or replacement read authority.
final class MetricsAiWorkspaceInput {
  const MetricsAiWorkspaceInput({required this.metricsLease, required this.isCurrent, this.metric, this.alerts});
  final Object metricsLease;
  final bool Function() isCurrent;
  final MetricsAiMetricInput Function(MetricsAiFence fence)? metric;
  final MonitoringAlertSnapshot? alerts;
}
