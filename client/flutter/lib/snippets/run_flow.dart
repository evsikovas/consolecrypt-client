import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/error_messages.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/l10n/labels.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/core/widgets/risk_badge.dart';
import 'package:consolecrypt/hosts/host_picker.dart';
import 'package:consolecrypt/terminal/terminal_tabs_controller.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import 'package:material_ui/material_ui.dart';

/// Whether the Run button may be pressed (CLIENT_SPEC §16): read-only
/// commands run after review; everything else needs explicit acknowledgment.
bool canRunCommand(RiskLevel risk, {required bool acknowledged}) => !risk.requiresConfirmation || acknowledged;

/// Asks for template variable values. Returns `null` on cancel.
Future<Map<String, String>?> showVariablesDialog(BuildContext context, Snippet snippet) =>
    showAppDialog<Map<String, String>>(context, builder: (_) => _VariablesDialog(snippet: snippet));

class _VariablesDialog extends StatefulWidget {
  const _VariablesDialog({required this.snippet});

  final Snippet snippet;

  @override
  State<_VariablesDialog> createState() => _VariablesDialogState();
}

class _VariablesDialogState extends State<_VariablesDialog> {
  late final Map<String, TextEditingController> _controllers = {
    for (final v in widget.snippet.effectiveVariables) v.name: TextEditingController(text: v.defaultValue ?? ''),
  };

  @override
  void dispose() {
    for (final c in _controllers.values) {
      c.dispose();
    }
    super.dispose();
  }

  Map<String, String> get _values => {for (final e in _controllers.entries) e.key: e.value.text};

  bool get _complete => widget.snippet.effectiveVariables
      .where((v) => v.required)
      .every((v) => _controllers[v.name]!.text.trim().isNotEmpty);

  @override
  Widget build(BuildContext context) {
    final l10n = context.l10n;
    final tokens = GlassTokens.of(context);
    final preview = renderTemplate(widget.snippet.template, {
      for (final e in _values.entries)
        if (e.value.isNotEmpty) e.key: e.value,
    });
    return GlassDialog(
      key: const ValueKey('variables-dialog'),
      title: widget.snippet.name,
      width: 560,
      content: SingleChildScrollView(
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            for (final (i, v) in widget.snippet.effectiveVariables.indexed) ...[
              TextField(
                key: ValueKey('var-${v.name}'),
                controller: _controllers[v.name],
                autofocus: i == 0,
                decoration: InputDecoration(
                  labelText: v.name,
                  helperText: v.description.isEmpty ? null : v.description,
                ),
                onChanged: (_) => setState(() {}),
              ),
              const SizedBox(height: GlassSpacing.s12),
            ],
            Text(l10n.runFlowVariablesPreview, style: tokens.typography.bodyEmph.copyWith(color: tokens.palette.label)),
            const SizedBox(height: GlassSpacing.s4),
            ContentSurface(
              kind: ContentSurfaceKind.inset,
              padding: const EdgeInsets.all(GlassSpacing.s8),
              child: SelectableText(
                preview,
                style: tokens.typography.mono.copyWith(fontSize: 12, color: tokens.palette.label),
              ),
            ),
          ],
        ),
      ),
      secondaryActions: [
        GlassButton(label: l10n.commonCancel, onPressed: () => closeDialog<Map<String, String>>(context)),
      ],
      primaryAction: GlassButton.prominent(
        key: const ValueKey('variables-continue'),
        onPressed: _complete ? () => closeDialog(context, _values) : null,
        label: l10n.commonContinue,
      ),
      onSubmit: _complete ? () => closeDialog(context, _values) : null,
    );
  }
}

/// Shows command, host and risk; returns true only after an explicit Run.
/// A risk decision: the secure material — opaque, nothing animated (§4.7).
Future<bool> showRunConfirmation(
  BuildContext context, {
  required String command,
  required String hostLabel,
  required RiskAssessment risk,
}) async =>
    await showAppDialog<bool>(
      context,
      secure: true,
      builder: (_) => RunConfirmationDialog(command: command, hostLabel: hostLabel, risk: risk),
    ) ??
    false;

/// Risky-command confirmation (§4.12): host and command always visible,
/// the effective risk as icon + label + colour; modifying / unknown need an
/// acknowledgement, destructive also a danger button and the host in the
/// title.
class RunConfirmationDialog extends StatefulWidget {
  const RunConfirmationDialog({required this.command, required this.hostLabel, required this.risk, super.key});

  final String command;
  final String hostLabel;
  final RiskAssessment risk;

  @override
  State<RunConfirmationDialog> createState() => _RunConfirmationDialogState();
}

class _RunConfirmationDialogState extends State<RunConfirmationDialog> {
  bool _acknowledged = false;

  @override
  Widget build(BuildContext context) {
    final l10n = context.l10n;
    final risk = widget.risk.effective;
    final tokens = GlassTokens.of(context);
    final t = tokens.typography;
    final destructive = risk == RiskLevel.destructive;
    final canRun = canRunCommand(risk, acknowledged: _acknowledged);
    void run() => closeDialog(context, true);
    return GlassDialog(
      key: const ValueKey('run-confirmation'),
      icon: destructive ? Icons.dangerous_rounded : Icons.play_circle_rounded,
      iconTone: destructive ? GlassTone.danger : GlassTone.accent,
      title: destructive ? l10n.runFlowConfirmTitleDestructive(widget.hostLabel) : l10n.runFlowConfirmTitle,
      width: 600,
      content: SingleChildScrollView(
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Row(
              children: [
                Icon(Icons.dns_rounded, size: 18, color: tokens.secondaryLabel),
                const SizedBox(width: GlassSpacing.s6),
                Expanded(
                  child: Text(widget.hostLabel, key: const ValueKey('run-host'), style: t.bodyEmph),
                ),
                const SizedBox(width: GlassSpacing.s8),
                RiskBadge(risk: risk),
              ],
            ),
            const SizedBox(height: GlassSpacing.s12),
            ContentSurface(
              kind: ContentSurfaceKind.inset,
              padding: const EdgeInsets.all(GlassSpacing.s12),
              child: SelectableText(
                widget.command,
                key: const ValueKey('run-command'),
                style: t.mono.copyWith(color: tokens.palette.label),
              ),
            ),
            const SizedBox(height: GlassSpacing.s8),
            if (widget.risk.reasons.isNotEmpty)
              Text(
                l10n.runFlowLocalRules(widget.risk.reasons.join('; ')),
                style: t.callout.copyWith(color: tokens.secondaryLabel),
              ),
            if (widget.risk.declared != widget.risk.effective && widget.risk.declared != RiskLevel.unknown)
              Text(
                l10n.runFlowDeclaredVsLocal(
                  widget.risk.declared.localized(l10n),
                  widget.risk.effective.localized(l10n),
                ),
                style: t.callout.copyWith(color: tokens.secondaryLabel),
              ),
            if (risk.requiresConfirmation) ...[
              const SizedBox(height: GlassSpacing.s8),
              CheckboxListTile(
                key: const ValueKey('run-acknowledge'),
                contentPadding: EdgeInsets.zero,
                controlAffinity: ListTileControlAffinity.leading,
                value: _acknowledged,
                onChanged: (v) => setState(() => _acknowledged = v ?? false),
                title: Text(switch (risk) {
                  RiskLevel.destructive => l10n.runFlowAckDestructive(widget.hostLabel),
                  RiskLevel.modifying => l10n.runFlowAckModifying(widget.hostLabel),
                  _ => l10n.runFlowAckUnknown,
                }),
              ),
            ],
          ],
        ),
      ),
      secondaryActions: [
        GlassButton(label: l10n.commonCancel, autofocus: destructive, onPressed: () => closeDialog(context, false)),
      ],
      primaryAction: destructive
          ? GlassButton.destructive(
              key: const ValueKey('run-confirm'),
              icon: Icons.play_arrow_rounded,
              onPressed: canRun ? run : null,
              label: l10n.runFlowRun,
            )
          : GlassButton.prominent(
              key: const ValueKey('run-confirm'),
              icon: Icons.play_arrow_rounded,
              onPressed: canRun ? run : null,
              label: l10n.runFlowRun,
            ),
    );
  }
}

String _hostLabel(Host host) => '${host.name} (${host.address})';

/// Picks the target (active terminal tab or a host), asks for confirmation
/// and runs. Nothing executes without the user pressing Run (§16).
Future<bool> runCommandFlow(
  BuildContext context,
  WidgetRef ref, {
  required String command,
  RiskLevel declared = RiskLevel.unknown,
  SnippetSource source = SnippetSource.user,
}) async {
  final tabs = ref.read(terminalTabsProvider.notifier);
  var target = ref.read(terminalTabsProvider).active;
  Host? host = target?.host;
  if (host == null) {
    host = await showHostPicker(context, title: context.l10n.runFlowPickHostTitle);
    if (host == null || !context.mounted) return false;
  }
  final RiskAssessment risk;
  try {
    risk = await ref.read(snippetServiceProvider).assessRisk(command, declared: declared, source: source);
  } on AppException catch (e) {
    if (context.mounted) showSnack(context, errorMessage(context.l10n, e), error: true);
    return false;
  }
  if (!context.mounted) return false;
  final ok = await showRunConfirmation(context, command: command, hostLabel: _hostLabel(host), risk: risk);
  if (!ok || !context.mounted) return false;
  if (target == null) {
    target = await runWithFeedback(context, () => tabs.open(host!));
    if (target == null) return false;
  }
  tabs.runInTab(target, command);
  if (context.mounted) context.go(AppRoutes.terminal);
  return true;
}

/// Snippet: variables form → render → confirmation → run.
Future<bool> runSnippetFlow(BuildContext context, WidgetRef ref, Snippet snippet) async {
  var values = const <String, String>{};
  if (snippet.effectiveVariables.isNotEmpty) {
    final result = await showVariablesDialog(context, snippet);
    if (result == null || !context.mounted) return false;
    values = result;
  }
  final String command;
  try {
    command = await ref.read(snippetServiceProvider).render(snippet, values);
  } on AppException catch (e) {
    if (context.mounted) showSnack(context, errorMessage(context.l10n, e), error: true);
    return false;
  }
  if (!context.mounted) return false;
  final ran = await runCommandFlow(context, ref, command: command, declared: snippet.riskLevel, source: snippet.source);
  if (ran) await ref.read(snippetServiceProvider).recordUsage(snippet.id);
  return ran;
}

/// "Insert": types a snippet (after variables) into the active terminal
/// without pressing Enter.
Future<void> insertSnippetFlow(BuildContext context, WidgetRef ref, Snippet snippet) async {
  final profile = ref.read(activeProfileProvider)?.id;
  final target = ref.read(terminalTabsProvider).active;
  var values = const <String, String>{};
  if (snippet.effectiveVariables.isNotEmpty) {
    final result = await showVariablesDialog(context, snippet);
    if (result == null || !context.mounted) return;
    values = result;
  }
  final command = await runWithFeedback(context, () => ref.read(snippetServiceProvider).render(snippet, values));
  if (command == null || !context.mounted) return;
  if (profile != ref.read(activeProfileProvider)?.id || target != ref.read(terminalTabsProvider).active) {
    showSnack(context, context.l10n.snippetTargetsChanged);
    return;
  }
  if (target != null &&
      RegExp(r'[\r\n\x1b]').hasMatch(command) &&
      (!target.isConnected || !target.terminal.bracketedPasteMode || command.contains('\x1b'))) {
    showSnack(context, context.l10n.snippetUnsafePaste);
    return;
  }
  if (ref.read(terminalTabsProvider.notifier).insertIntoActive(command)) {
    context.go(AppRoutes.terminal);
  } else {
    showSnack(context, context.l10n.runFlowOpenTerminalFirst);
  }
}

/// Review a fixed set of connected sessions; never retarget after a dialog,
/// queue onto a disconnected session, or execute on a newly opened tab.
Future<void> runSnippetInManyFlow(BuildContext context, WidgetRef ref, Snippet snippet) async {
  final profile = ref.read(activeProfileProvider)?.id;
  final candidates = ref.read(terminalTabsProvider).tabs.where((t) => t.isConnected).toList();
  if (candidates.isEmpty) {
    showSnack(context, context.l10n.snippetNoConnected);
    return;
  }
  final targets = await showAppDialog<List<TerminalTab>>(context, builder: (_) => _SnippetTargets(tabs: candidates));
  if (targets == null || targets.isEmpty || !context.mounted) return;
  var values = const <String, String>{};
  if (snippet.effectiveVariables.isNotEmpty) {
    final chosen = await showVariablesDialog(context, snippet);
    if (chosen == null || !context.mounted) return;
    values = chosen;
  }
  final service = ref.read(snippetServiceProvider);
  final command = await runWithFeedback(context, () => service.render(snippet, values));
  if (command == null || !context.mounted) return;
  final risk = await runWithFeedback(
    context,
    () => service.assessRisk(command, declared: snippet.riskLevel, source: snippet.source),
  );
  if (risk == null || !context.mounted) return;
  final confirmed = await showRunConfirmation(
    context,
    command: command,
    hostLabel: targets.map((t) => _hostLabel(t.host)).join('\n'),
    risk: risk,
  );
  if (!confirmed || !context.mounted) return;
  final current = ref.read(terminalTabsProvider).tabs;
  if (profile != ref.read(activeProfileProvider)?.id || targets.any((t) => !current.contains(t) || !t.isConnected)) {
    showSnack(context, context.l10n.snippetTargetsChanged);
    return;
  }
  final controller = ref.read(terminalTabsProvider.notifier);
  for (final target in targets) {
    controller.runInTab(target, command);
  }
  await service.recordUsage(snippet.id);
  if (context.mounted) context.go(AppRoutes.terminal);
}

class _SnippetTargets extends StatefulWidget {
  const _SnippetTargets({required this.tabs});
  final List<TerminalTab> tabs;
  @override
  State<_SnippetTargets> createState() => _SnippetTargetsState();
}

class _SnippetTargetsState extends State<_SnippetTargets> {
  late final _selected = widget.tabs.toSet();
  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    return GlassDialog(
      key: const ValueKey('snippet-targets'),
      title: l.snippetChooseTerminals,
      width: 560,
      content: ConstrainedBox(
        constraints: const BoxConstraints(maxHeight: 400),
        child: SingleChildScrollView(
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Text(l.snippetConnectedOnly),
              const SizedBox(height: 12),
              for (final tab in widget.tabs)
                CheckboxListTile(
                  key: ValueKey('snippet-target-${tab.sessionId}'),
                  value: _selected.contains(tab),
                  title: Text(tab.host.name),
                  subtitle: Text(tab.host.address),
                  onChanged: (selected) =>
                      setState(() => selected == true ? _selected.add(tab) : _selected.remove(tab)),
                ),
            ],
          ),
        ),
      ),
      secondaryActions: [GlassButton(label: l.commonCancel, onPressed: () => closeDialog<List<TerminalTab>>(context))],
      primaryAction: GlassButton.prominent(
        key: const ValueKey('snippet-targets-continue'),
        label: l.commonContinue,
        onPressed: _selected.isEmpty
            ? null
            : () => closeDialog(context, widget.tabs.where(_selected.contains).toList()),
      ),
    );
  }
}
