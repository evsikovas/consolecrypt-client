import 'dart:convert';

import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/monitoring/monitoring_alert_models.dart';
import 'package:consolecrypt/monitoring/monitoring_result_view.dart';
import 'package:material_ui/material_ui.dart';

String _stateLabel(AppLocalizations l, MonitoringAlertStatus state) => switch (state.state) {
  MonitoringAlertState.inactive => l.monitoringAlertInactive,
  MonitoringAlertState.pending => l.monitoringAlertPending,
  MonitoringAlertState.firing => l.monitoringAlertFiring,
  MonitoringAlertState.unknown => l.monitoringAlertUnknown,
};
String _healthLabel(AppLocalizations l, MonitoringHealthStatus health) => switch (health.health) {
  MonitoringRuleHealth.ok => l.monitoringRuleHealthOk,
  MonitoringRuleHealth.error => l.monitoringRuleHealthError,
  MonitoringRuleHealth.unknown => l.monitoringAlertUnknown,
};

/// Display-only surface. The existing workspace owns source/profile generation,
/// request admission, explicit reads and cancellation; this owns no service.
class MonitoringAlertsView extends StatelessWidget {
  const MonitoringAlertsView({
    super.key,
    required this.result,
    required this.enabled,
    required this.busy,
    required this.reading,
    required this.onRead,
    required this.onCancel,
  });
  final MonitoringAlertSnapshot? result;
  final bool enabled, busy, reading;
  final ValueChanged<MonitoringAlertKind> onRead;
  final VoidCallback onCancel;

  Widget _unknown(AppLocalizations l, List<String> fields) => fields.isEmpty
      ? const SizedBox.shrink()
      : Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [Text(l.monitoringAlertUnsupportedFields), SelectableText(fields.join(', '))],
        );
  Widget _metadata(AppLocalizations l, Map<String, String> labels, Map<String, String> annotations) => Column(
    crossAxisAlignment: CrossAxisAlignment.stretch,
    children: [
      ExpansionTile(title: Text(l.monitoringAlertLabels), children: [SelectableText(jsonEncode(labels))]),
      ExpansionTile(title: Text(l.monitoringAlertAnnotations), children: [SelectableText(jsonEncode(annotations))]),
    ],
  );
  Widget _alert(BuildContext context, MonitoringActiveAlert alert) {
    final l = context.l10n;
    final name = alert.labels['alertname'];
    return ExpansionTile(
      title: Text(name == null || name.isEmpty ? l.monitoringAlertRecord : name),
      subtitle: Text(_stateLabel(l, alert.state)),
      children: [
        Padding(
          padding: const EdgeInsets.all(8),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              if (alert.state.raw != null) SelectableText(alert.state.raw!),
              Text(l.monitoringAlertValue),
              SelectableText(alert.value.display),
              if (alert.activeAt != null) ...[Text(l.monitoringAlertActiveAt), SelectableText(alert.activeAt!)],
              if (alert.keepFiringSince != null) ...[
                Text(l.monitoringAlertKeepFiringSince),
                SelectableText(alert.keepFiringSince!),
              ],
              _metadata(l, alert.labels, alert.annotations),
              _unknown(l, alert.unsupportedFields),
            ],
          ),
        ),
      ],
    );
  }

  Widget _rule(BuildContext context, MonitoringAlertRule rule) {
    final l = context.l10n;
    return ExpansionTile(
      title: Text(rule.name.isEmpty ? l.monitoringAlertRule : rule.name),
      subtitle: Wrap(
        spacing: 8,
        runSpacing: 4,
        children: [Text(_stateLabel(l, rule.state)), Text(_healthLabel(l, rule.health))],
      ),
      children: [
        Padding(
          padding: const EdgeInsets.all(8),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              if (rule.state.raw != null) SelectableText(rule.state.raw!),
              if (rule.health.raw != null) SelectableText(rule.health.raw!),
              if (rule.lastError != null)
                SelectableText(rule.lastError!, style: TextStyle(color: Theme.of(context).colorScheme.error)),
              ExpansionTile(title: Text(l.monitoringRuleExpression), children: [SelectableText(rule.query)]),
              ExpansionTile(
                title: Text(l.monitoringRuleTiming),
                children: [
                  SelectableText(
                    jsonEncode({
                      'durationSeconds': rule.durationSeconds,
                      'keepFiringForSeconds': rule.keepFiringForSeconds,
                      'evaluationTimeSeconds': rule.evaluationTimeSeconds,
                      'lastEvaluation': rule.lastEvaluation,
                    }),
                  ),
                ],
              ),
              _metadata(l, rule.labels, rule.annotations),
              if (rule.alerts.isNotEmpty)
                ConstrainedBox(
                  constraints: const BoxConstraints(maxHeight: 220),
                  child: ListView.builder(
                    shrinkWrap: true,
                    primary: false,
                    itemCount: rule.alerts.length,
                    itemBuilder: (_, i) => _alert(context, rule.alerts[i]),
                  ),
                ),
              _unknown(l, rule.unsupportedFields),
            ],
          ),
        ),
      ],
    );
  }

  Widget _group(BuildContext context, MonitoringAlertGroup group) {
    final l = context.l10n;
    return ExpansionTile(
      title: Text(group.name.isEmpty ? l.monitoringAlertGroup : group.name),
      children: [
        Padding(
          padding: const EdgeInsets.all(8),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Text(l.monitoringAlertBackendFile),
              SelectableText(group.file),
              ExpansionTile(
                title: Text(l.monitoringRuleTiming),
                children: [
                  SelectableText(
                    jsonEncode({
                      'intervalSeconds': group.intervalSeconds,
                      'limit': group.limit,
                      'evaluationTimeSeconds': group.evaluationTimeSeconds,
                      'lastEvaluation': group.lastEvaluation,
                    }),
                  ),
                ],
              ),
              if (group.rules.isEmpty) Text(l.monitoringAlertEmpty),
              if (group.rules.isNotEmpty)
                ConstrainedBox(
                  constraints: const BoxConstraints(maxHeight: 300),
                  child: ListView.builder(
                    shrinkWrap: true,
                    primary: false,
                    itemCount: group.rules.length,
                    itemBuilder: (_, i) => _rule(context, group.rules[i]),
                  ),
                ),
              _unknown(l, group.unsupportedFields),
            ],
          ),
        ),
      ],
    );
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n, result = this.result;
    return ContentSurface(
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Text(l.monitoringAlertsTitle, style: Theme.of(context).textTheme.titleMedium),
          const SizedBox(height: 8),
          Text(l.monitoringAlertsHint),
          const SizedBox(height: 8),
          Wrap(
            spacing: 8,
            runSpacing: 8,
            children: [
              GlassButton(
                key: const ValueKey('monitoring-read-alerts'),
                label: l.monitoringAlertsRead,
                onPressed: !enabled || busy ? null : () => onRead(MonitoringAlertKind.alerts),
              ),
              GlassButton(
                key: const ValueKey('monitoring-read-alert-rules'),
                label: l.monitoringRulesRead,
                onPressed: !enabled || busy ? null : () => onRead(MonitoringAlertKind.alertRules),
              ),
              if (reading)
                TextButton(
                  key: const ValueKey('monitoring-alerts-cancel'),
                  onPressed: onCancel,
                  child: Text(l.commonCancel),
                ),
            ],
          ),
          if (reading) const LinearProgressIndicator(),
          if (result == null && !reading) Text(l.monitoringAlertsNotRead),
          if (result != null) ...[
            const SizedBox(height: 12),
            Text(
              l.monitoringResultStatus(
                result.status == 'success' ? l.monitoringSuccess : l.monitoringError,
                result.httpStatus.toString(),
              ),
              key: const ValueKey('monitoring-alerts-status'),
            ),
            if (result.partial) Text(l.monitoringPartialResult, key: const ValueKey('monitoring-alerts-partial')),
            if (result.error != null) SelectableText(monitoringFailureText(context, result.error!)),
            for (final warning in result.warnings) Text(l.monitoringWarning(warning)),
            for (final info in result.infos) Text(l.monitoringInfo(info)),
            if (result.empty) Text(l.monitoringAlertEmpty, key: const ValueKey('monitoring-alerts-empty')),
            if (result.groupNextToken != null) Text(l.monitoringAlertContinuation),
            _unknown(l, result.unsupportedFields),
            if (result.alerts.isNotEmpty)
              ConstrainedBox(
                constraints: const BoxConstraints(maxHeight: 340),
                child: ListView.builder(
                  key: const ValueKey('monitoring-active-alerts'),
                  shrinkWrap: true,
                  primary: false,
                  itemCount: result.alerts.length,
                  itemBuilder: (_, i) => _alert(context, result.alerts[i]),
                ),
              ),
            if (result.groups.isNotEmpty)
              ConstrainedBox(
                constraints: const BoxConstraints(maxHeight: 400),
                child: ListView.builder(
                  key: const ValueKey('monitoring-alert-rule-groups'),
                  shrinkWrap: true,
                  primary: false,
                  itemCount: result.groups.length,
                  itemBuilder: (_, i) => _group(context, result.groups[i]),
                ),
              ),
          ],
        ],
      ),
    );
  }
}
