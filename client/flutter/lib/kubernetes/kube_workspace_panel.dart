import 'package:consolecrypt/app/platform.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/kubernetes/kube_ai_panel.dart';
import 'package:consolecrypt/kubernetes/kube_ai_service.dart';
import 'package:consolecrypt/kubernetes/kube_browser_service.dart';
import 'package:consolecrypt/kubernetes/kube_context_terminal_dialog.dart';
import 'package:consolecrypt/kubernetes/kube_context_terminal_service.dart';
import 'package:consolecrypt/kubernetes/kube_logs_panel.dart';
import 'package:consolecrypt/kubernetes/kube_providers.dart';
import 'package:consolecrypt/kubernetes/kube_terminal_panel.dart';
import 'package:consolecrypt/kubernetes/kube_workspace_content.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

class KubeWorkspacePanel extends ConsumerStatefulWidget {
  const KubeWorkspacePanel({
    required this.session,
    required this.contextName,
    required this.service,
    required this.onHide,
    this.ai = false,
    this.selected,
    this.question,
    this.closeRouteOnLock = false,
    this.extraSessions = const {},
    this.logPins = const [],
    this.selectedLog,
    this.onCloseLog,
    this.onRuntimeOpened,
    this.namespace = 'default',
    this.onReviewedCommand,
    this.onTabChanged,
    this.onToggleDock,
    this.dockedRight = false,
    this.outputs = const [],
    this.selectedOutput,
    super.key,
  });
  final KubeSession session;
  final Set<TerminalSessionId> extraSessions;
  final String contextName, namespace;
  final List<KubeLogPin> logPins;
  final List<KubePanelOutput> outputs;
  final String? selectedLog, selectedOutput;
  final ValueChanged<String>? onCloseLog, onRuntimeOpened, onTabChanged;
  final Future<void> Function(String runtimeId, String command)? onReviewedCommand;
  final KubeBrowserService service;
  final VoidCallback onHide;
  final VoidCallback? onToggleDock;
  final bool dockedRight;
  final bool ai, closeRouteOnLock;
  final KubeResourceRef? selected;
  final String? question;
  @override
  ConsumerState<KubeWorkspacePanel> createState() => _KubeWorkspacePanelState();
}

class _KubeWorkspacePanelState extends ConsumerState<KubeWorkspacePanel> {
  late final KubeScope _scope = ref.read(kubeScopeProvider);
  late bool _ai = widget.ai;
  late String? _log = widget.selectedLog, _output = widget.selectedOutput;
  @override
  void didUpdateWidget(KubeWorkspacePanel old) {
    super.didUpdateWidget(old);
    if (old.ai != widget.ai ||
        old.question != widget.question ||
        old.selectedLog != widget.selectedLog ||
        old.selectedOutput != widget.selectedOutput) {
      _ai = widget.ai;
      _log = widget.selectedLog;
      _output = widget.selectedOutput;
    }
    if (!widget.logPins.any((pin) => pin.id == _log)) _log = null;
    if (!widget.outputs.any((output) => output.id == _output)) _output = null;
  }

  @override
  Widget build(BuildContext context) {
    ref.listen(kubeScopeProvider, (_, next) {
      if (identical(_scope, next) || !widget.closeRouteOnLock) return;
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (!mounted) return;
        final route = ModalRoute.of(context);
        if (route?.isActive == true) Navigator.of(context).removeRoute(route!);
      });
    });
    if (!_scope.unlocked || !identical(_scope, ref.watch(kubeScopeProvider))) return const SizedBox.shrink();
    final l = context.l10n;
    return SecureSurface(
      opaque: true,
      padding: const EdgeInsets.all(8),
      shadow: false,
      child: Material(
        type: MaterialType.transparency,
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Row(
              children: [
                Expanded(
                  child: SingleChildScrollView(
                    scrollDirection: Axis.horizontal,
                    child: Row(
                      children: [
                        if (!AppPlatform.isMobile && widget.service is KubeContextTerminalService)
                          GlassIconButton(
                            icon: Icons.add,
                            tooltip: l.kubeContextTerminal,
                            onPressed: () async {
                              final id = await showAppDialog<String>(
                                context,
                                secure: true,
                                builder: (_) => KubeContextTerminalDialog(
                                  sessionId: widget.session.id,
                                  namespace: widget.namespace,
                                  service: widget.service as KubeContextTerminalService,
                                ),
                              );
                              if (mounted && id != null && identical(_scope, ref.read(kubeScopeProvider))) {
                                widget.onRuntimeOpened?.call(id);
                              }
                            },
                          ),
                        ChoiceChip(
                          label: Text(l.kubePodTerminal),
                          selected: !_ai && _log == null && _output == null,
                          onSelected: (_) => setState(() {
                            _ai = false;
                            _log = null;
                            _output = null;
                            widget.onTabChanged?.call('terminal');
                          }),
                        ),
                        if (widget.service is KubeAiService)
                          ChoiceChip(
                            key: const ValueKey('kube-panel-ai'),
                            label: Text(l.kubeAiAssistant),
                            selected: _ai,
                            onSelected: (_) => setState(() {
                              _ai = true;
                              _log = null;
                              _output = null;
                              widget.onTabChanged?.call('ai');
                            }),
                          ),
                        for (final pin in widget.logPins)
                          InputChip(
                            label: Text('${l.kubeLogs} · ${pin.resource.name}'),
                            selected: _log == pin.id,
                            onSelected: (_) => setState(() {
                              _log = pin.id;
                              widget.onTabChanged?.call('logs');
                              _ai = false;
                              _output = null;
                            }),
                            onDeleted: () {
                              widget.onCloseLog?.call(pin.id);
                              if (mounted) {
                                setState(() {
                                  if (_log == pin.id) _log = null;
                                });
                              }
                            },
                          ),
                        for (final output in widget.outputs)
                          ChoiceChip(
                            label: Text('${l.kubeOutput} · ${output.title}'),
                            selected: _output == output.id,
                            onSelected: (_) => setState(() {
                              _output = output.id;
                              widget.onTabChanged?.call('output');
                              _ai = false;
                              _log = null;
                            }),
                          ),
                      ],
                    ),
                  ),
                ),
                if (widget.onToggleDock != null)
                  GlassIconButton(
                    icon: widget.dockedRight ? Icons.vertical_align_bottom : Icons.view_sidebar_outlined,
                    tooltip: widget.dockedRight ? l.kubeDockBottom : l.kubeDockRight,
                    onPressed: widget.onToggleDock,
                  ),
                GlassIconButton(icon: Icons.keyboard_arrow_down, tooltip: l.kubeHidePanel, onPressed: widget.onHide),
              ],
            ),
            Expanded(
              child: _log != null && widget.logPins.any((pin) => pin.id == _log)
                  ? KubeLogsPanel(
                      key: ValueKey(_log),
                      session: widget.session,
                      resource: widget.logPins.firstWhere((pin) => pin.id == _log).resource,
                      containers: widget.logPins.firstWhere((pin) => pin.id == _log).containers,
                      service: widget.service,
                    )
                  : _output != null && widget.outputs.any((output) => output.id == _output)
                  ? SingleChildScrollView(
                      child: SelectableText(widget.outputs.firstWhere((output) => output.id == _output).text),
                    )
                  : _ai && widget.service is KubeAiService
                  ? KubeAiPanel(
                      session: widget.session,
                      contextName: widget.contextName,
                      service: widget.service as KubeAiService,
                      selected: widget.selected,
                      initialQuestion: widget.question,
                      namespace: widget.namespace,
                      onReviewedCommand: widget.onReviewedCommand,
                    )
                  : KubeTerminalPanel(
                      sessionId: widget.session.id,
                      onHide: widget.onHide,
                      extraSessions: widget.extraSessions,
                    ),
            ),
          ],
        ),
      ),
    );
  }
}
