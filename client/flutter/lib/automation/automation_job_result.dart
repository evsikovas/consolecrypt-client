import 'dart:convert';

import 'package:consolecrypt/automation/automation_job_result_metadata.dart';
import 'package:consolecrypt/automation/automation_service.dart';
import 'package:consolecrypt/automation/automation_technical_block.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:material_ui/material_ui.dart';

/// UI state is keyed by the native job ID by the owning history/console.
/// Polling may replace immutable evidence without replacing this state.
class AutomationJobResultViewState {
  int page = 0, auditPage = 0;
  bool hostsOpen = false, environmentOpen = false, detailsOpen = false, auditOpen = false;
}

/// A bounded view of historical evidence. It never grants retry/target access.
class AutomationJobResult extends StatefulWidget {
  const AutomationJobResult({super.key, required this.job, this.viewState});
  final AutomationJson job;
  final AutomationJobResultViewState? viewState;
  @override
  State<AutomationJobResult> createState() => _AutomationJobResultState();
}

class _AutomationJobResultState extends State<AutomationJobResult> {
  final localState = AutomationJobResultViewState();
  AutomationJobResultViewState get view => widget.viewState ?? localState;
  static const pageSize = 25;

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    final manifest = _map(widget.job['manifest']);
    final evidence = automationVisibleResultMap(widget.job['runner_result']);
    final result = automationVisibleResultMap(evidence?['result']);
    final stats = automationVisibleResultMap(automationVisibleResultMap(result?['summary'])?['stats']);
    final aliases = <String>{
      if (manifest?['targets'] case final List<dynamic> targets)
        for (final target in targets.take(10000))
          if (_map(target)?['alias'] case final String alias) alias,
    }.toList()..sort();
    final lastPage = aliases.isEmpty ? 0 : (aliases.length - 1) ~/ pageSize;
    final selectedPage = view.page.clamp(0, lastPage).toInt();
    final shown = aliases.skip(selectedPage * pageSize).take(pageSize);
    final receipt = automationVisibleResultMap(evidence?['environment']);
    final identity =
        receipt?['version'] is int &&
            receipt?['version'] == 1 &&
            manifest?['environment_digest'] is String &&
            receipt?['environment_digest'] == manifest?['environment_digest']
        ? automationClosedEnvironmentIdentity(receipt?['identity'])
        : null;
    final packages = automationVisibleResultMap(identity?['packages']);
    final metadata = view.detailsOpen ? automationResultMetadata(widget.job) : null;
    final details = metadata == null ? '' : const JsonEncoder.withIndent('  ').convert(metadata.facts);
    const detailsLimit = 16384;
    final truncated = details.length > detailsLimit;
    final metrics = <String, String>{
      'ok': l.automationResultOk,
      'changed': l.automationResultChanged,
      'failures': l.automationResultFailed,
      'dark': l.automationResultUnreachable,
      'skipped': l.automationResultSkipped,
    };
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        if (result?['exit_code'] case final int code)
          AutomationTechnicalBlock(
            padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 8),
            child: Text('${l.automationResultExitCode}: $code'),
          ),
        if (evidence?['remote_changes_may_be_partial'] == true || result?['remote_changes_may_be_partial'] == true)
          InfoBanner(
            key: ValueKey('automation-result-partial-${widget.job['job_id']}'),
            tone: BannerTone.warning,
            message: l.automationResultPartial,
          ),
        AutomationTechnicalBlock(
          child: ExpansionTile(
            key: ValueKey('automation-result-hosts-${widget.job['job_id']}'),
            initiallyExpanded: view.hostsOpen,
            onExpansionChanged: (open) => setState(() => view.hostsOpen = open),
            title: Text(l.automationResultHosts),
            children: [
              Text(l.automationResultMissingHint),
              if (aliases.isEmpty) Text(l.automationResultNoHosts),
              for (final alias in shown)
                Padding(
                  key: ValueKey('automation-result-host-$alias'),
                  padding: const EdgeInsets.symmetric(vertical: 8, horizontal: 12),
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      SelectableText(alias),
                      Wrap(
                        spacing: 14,
                        runSpacing: 4,
                        children: [
                          for (final metric in metrics.entries)
                            Text('${metric.value}: ${_count(stats, metric.key, alias) ?? l.automationResultMissing}'),
                        ],
                      ),
                    ],
                  ),
                ),
              if (aliases.length > pageSize)
                Wrap(
                  crossAxisAlignment: WrapCrossAlignment.center,
                  children: [
                    TextButton(
                      key: const ValueKey('automation-result-previous'),
                      onPressed: selectedPage > 0 ? () => setState(() => view.page = selectedPage - 1) : null,
                      child: Text(l.automationInventoryPrevious),
                    ),
                    Text('${selectedPage + 1} / ${lastPage + 1}'),
                    TextButton(
                      key: const ValueKey('automation-result-next'),
                      onPressed: selectedPage < lastPage ? () => setState(() => view.page = selectedPage + 1) : null,
                      child: Text(l.automationInventoryNext),
                    ),
                  ],
                ),
            ],
          ),
        ),
        AutomationTechnicalBlock(
          child: ExpansionTile(
            key: ValueKey('automation-result-environment-${widget.job['job_id']}'),
            initiallyExpanded: view.environmentOpen,
            onExpansionChanged: (open) => setState(() => view.environmentOpen = open),
            title: Text(l.automationResultEnvironment),
            children: [
              if (identity == null)
                Text(l.automationResultEnvironmentMissing)
              else ...[
                Text /* l10n-ignore: localized receipt and platform data */ (
                  '${l.automationResultEnvironmentHint} ${_text(identity['system'])} · ${_text(identity['machine'])}',
                ), // l10n-ignore: localized receipt label plus platform identity
                Text('Python: ${_text(identity['python'])}'), // l10n-ignore: Python product name and recorded version
                Text /* l10n-ignore: product name and version */ (
                  /* l10n-ignore: product version */ 'ConsoleCrypt Runner: ${_text(identity['helper'])}',
                ), // l10n-ignore: ConsoleCrypt Runner product name and recorded version
                for (final name in ['ansible-core', 'ansible-runner', 'ansible-lint'])
                  Text /* l10n-ignore: package name and version */ (
                    '$name: ${_text(_map(packages?[name])?['actual'])}',
                  ), // l10n-ignore: package name and recorded version
              ],
            ],
          ),
        ),
        AutomationTechnicalBlock(
          child: ExpansionTile(
            key: ValueKey('automation-result-details-${widget.job['job_id']}'),
            initiallyExpanded: view.detailsOpen,
            title: Text(l.automationResultDetails),
            onExpansionChanged: (open) => setState(() => view.detailsOpen = open),
            children: [
              Text(l.automationResultDetailsFiltered),
              if (metadata?.recognized == false) Text(l.automationResultDetailsUnknown),
              if (truncated) Text(l.automationOutputLimited),
              SelectableText(truncated ? '${details.substring(0, detailsLimit)}…' : details),
            ],
          ),
        ),
      ],
    );
  }
}

AutomationJson? _map(Object? value) => value is AutomationJson ? value : null;
String _text(Object? value) => value is String && value.length <= 256 ? value : '—';
int? _count(AutomationJson? stats, String metric, String alias) {
  final value = automationVisibleResultMap(stats?[metric])?[alias];
  return value is int && value >= 0 && value <= 0x7fffffff ? value : null;
}
