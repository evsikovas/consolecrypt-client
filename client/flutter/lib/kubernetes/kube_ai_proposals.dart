import 'package:consolecrypt/ai/code_blocks.dart';
import 'package:consolecrypt/app/platform.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/kubernetes/kube_ai_service.dart';
import 'package:consolecrypt/kubernetes/kube_browser_service.dart';
import 'package:consolecrypt/kubernetes/kube_context_terminal_dialog.dart';
import 'package:consolecrypt/kubernetes/kube_context_terminal_service.dart';
import 'package:consolecrypt/kubernetes/kube_create_dialog.dart';
import 'package:consolecrypt/kubernetes/kube_manifest_service.dart';
import 'package:consolecrypt/kubernetes/kube_mutation_service.dart';
import 'package:consolecrypt/kubernetes/kube_proposal_identity.dart';
import 'package:consolecrypt/kubernetes/kube_providers.dart';
import 'package:consolecrypt/kubernetes/kube_yaml_editor.dart';
import 'package:consolecrypt/snippets/run_flow.dart';
import 'package:consolecrypt/snippets/snippet_editor.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

/// Model output has no execution authority. Fences offer only explicit review
/// through the same risk gate or dry-run/apply dialogs used elsewhere.
class KubeAiProposals extends StatelessWidget {
  const KubeAiProposals({
    required this.answer,
    required this.session,
    required this.contextName,
    required this.namespace,
    required this.service,
    this.selected,
    this.unresolved = false,
    this.onReviewedCommand,
    super.key,
  });
  final String answer, contextName, namespace;
  final KubeSession session;
  final KubeAiService service;
  final KubeResourceRef? selected;
  final bool unresolved;
  final Future<void> Function(String runtimeId, String command)? onReviewedCommand;
  @override
  Widget build(BuildContext context) => Column(
    crossAxisAlignment: CrossAxisAlignment.stretch,
    children: [
      for (final segment in parseAnswer(answer, maxSegments: 32))
        if (segment is TextSegment)
          SelectableText(segment.text.trim())
        else if (segment is CodeSegment)
          _ProposalCard(
            code: segment.code,
            language: segment.language,
            session: session,
            contextName: contextName,
            namespace: namespace,
            service: service,
            selected: selected,
            unresolved: unresolved,
            onReviewedCommand: onReviewedCommand,
          ),
    ],
  );
}

class _ProposalCard extends ConsumerStatefulWidget {
  const _ProposalCard({
    required this.code,
    required this.language,
    required this.session,
    required this.contextName,
    required this.namespace,
    required this.service,
    required this.selected,
    required this.unresolved,
    required this.onReviewedCommand,
  });
  final String code, contextName, namespace;
  final String? language;
  final KubeSession session;
  final KubeAiService service;
  final KubeResourceRef? selected;
  final bool unresolved;
  final Future<void> Function(String, String)? onReviewedCommand;
  @override
  ConsumerState<_ProposalCard> createState() => _ProposalCardState();
}

class _ProposalCardState extends ConsumerState<_ProposalCard> {
  late final KubeScope _scope = ref.read(kubeScopeProvider);
  final _parser = KubeProposalParser();
  bool _busy = false, _failed = false;
  bool get _current => mounted && _scope.unlocked && identical(_scope, ref.read(kubeScopeProvider));
  @override
  void dispose() {
    _parser.cancel();
    super.dispose();
  }

  bool get _yaml => const ['yaml', 'yml'].contains(widget.language?.toLowerCase());
  bool get _command => const [
    null,
    '',
    'sh',
    'shell',
    'bash',
    'zsh',
    'powershell',
    'ps1',
    'cmd',
    'console',
  ].contains(widget.language?.toLowerCase());
  Future<void> _reviewCommand() async {
    final service = widget.service;
    if (!_current || _busy || service is! KubeContextTerminalService || widget.onReviewedCommand == null) return;
    setState(() {
      _busy = true;
      _failed = false;
    });
    try {
      final risk = await ref
          .read(snippetServiceProvider)
          .assessRisk(widget.code, declared: RiskLevel.unknown, source: SnippetSource.ai);
      if (!mounted || !_current) return;
      final approved =
          await showAppDialog<bool>(
            context,
            secure: true,
            builder: (_) => _ScopedCommandReview(
              scope: _scope,
              command: widget.code,
              hostLabel: '${widget.session.production ? 'PROD · ' : ''}${widget.contextName} · ${widget.namespace}',
              risk: risk,
            ),
          ) ==
          true;
      if (!mounted || !_current || !approved) return;
      final terminal = await showAppDialog<String>(
        context,
        secure: true,
        builder: (_) => KubeContextTerminalDialog(
          sessionId: widget.session.id,
          namespace: widget.namespace,
          service: service as KubeContextTerminalService,
        ),
      );
      if (!_current || terminal == null) return;
      await widget.onReviewedCommand!(terminal, widget.code);
    } catch (_) {
      if (_current) setState(() => _failed = true);
    } finally {
      if (_current) setState(() => _busy = false);
    }
  }

  Future<void> _reviewYaml() async {
    final service = widget.service;
    if (!_current || _busy || service is! KubeBrowserService || widget.code.length > 256 * 1024) return;
    setState(() {
      _busy = true;
      _failed = false;
    });
    try {
      final identity = await _parser.parse(widget.code);
      if (!mounted || !_current) return;
      if (identity == null) throw const FormatException();
      final apiVersion = identity.apiVersion, kind = identity.kind, name = identity.name;
      final namespace = identity.namespace ?? widget.namespace;
      final kinds = await (service as KubeBrowserService).discover(widget.session.id);
      if (!mounted || !_current) return;
      final type = kinds.where((type) => type.apiVersion == apiVersion && type.kind == kind).firstOrNull;
      if (type == null || type.isSecret) throw const FormatException();
      final reference = KubeResourceRef(api: type, namespace: type.namespaced ? namespace : null, name: name);
      final selected = widget.selected;
      if (selected != null &&
          selected.api.id == type.id &&
          selected.name == name &&
          selected.namespace == reference.namespace &&
          service is KubeMutationService) {
        final original = await (service as KubeBrowserService).getResource(widget.session.id, reference);
        if (!mounted || !_current) return;
        await showAppDialog<void>(
          context,
          secure: true,
          barrierDismissible: false,
          builder: (_) => KubeYamlEditor(
            session: widget.session,
            contextName: widget.contextName,
            resource: reference,
            original: original,
            service: service as KubeMutationService,
            initialYaml: widget.code,
          ),
        );
      } else if (service is KubeManifestService) {
        await showAppDialog<void>(
          context,
          secure: true,
          barrierDismissible: false,
          builder: (_) => KubeCreateDialog(
            session: widget.session,
            contextName: widget.contextName,
            kind: type,
            service: service as KubeManifestService,
            namespace: reference.namespace,
            initialName: name,
            initialYaml: widget.code,
          ),
        );
      }
    } catch (_) {
      if (_current) setState(() => _failed = true);
    } finally {
      if (_current) setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final scope = ref.watch(kubeScopeProvider);
    if (!identical(scope, _scope) || !_current) return const SizedBox.shrink();
    final l = context.l10n;
    final insertable =
        !AppPlatform.isMobile &&
        _command &&
        widget.code.length <= 16000 &&
        !RegExp(r'[\x00-\x1f\x7f]').hasMatch(widget.code) &&
        widget.onReviewedCommand != null &&
        widget.service is KubeContextTerminalService;
    return Card(
      child: Padding(
        padding: const EdgeInsets.all(12),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            SelectableText(widget.code),
            if (insertable) Text(l.kubeAiProposalContext, style: Theme.of(context).textTheme.bodySmall),
            Wrap(
              spacing: 8,
              children: [
                TextButton(
                  onPressed: () => copyPlainWithNotice(context, ref, widget.code, what: l.copyWhatCommand),
                  child: Text(l.commonCopy),
                ),
                if (_command)
                  TextButton(
                    onPressed: _busy
                        ? null
                        : () => showSnippetEditor(
                            context,
                            draft: SnippetDraft(
                              name: 'Kubernetes',
                              template: widget.code,
                              snippetType: SnippetType.kubectl,
                            ),
                          ),
                    child: Text(l.codeBlocksSaveAsSnippet),
                  ),
                if ((_yaml && widget.service is KubeBrowserService) || insertable)
                  TextButton(
                    key: const ValueKey('kube-ai-proposal-review'),
                    onPressed: _busy || widget.unresolved
                        ? null
                        : _yaml
                        ? _reviewYaml
                        : _reviewCommand,
                    child: Text(l.kubeAiProposalReview),
                  ),
              ],
            ),
            if (_busy) const LinearProgressIndicator(),
            if (_failed) Text(l.kubeRequestFailed),
          ],
        ),
      ),
    );
  }
}

class _ScopedCommandReview extends ConsumerWidget {
  const _ScopedCommandReview({required this.scope, required this.command, required this.hostLabel, required this.risk});
  final KubeScope scope;
  final String command, hostLabel;
  final RiskAssessment risk;
  @override
  Widget build(BuildContext context, WidgetRef ref) {
    ref.listen(kubeScopeProvider, (_, next) {
      if (identical(next, scope)) return;
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (!context.mounted) return;
        final route = ModalRoute.of(context);
        if (route?.isActive == true) Navigator.of(context).removeRoute(route!);
      });
    });
    if (!identical(ref.watch(kubeScopeProvider), scope)) return const SizedBox.shrink();
    return RunConfirmationDialog(
      command: command,
      hostLabel: hostLabel,
      risk: risk,
      confirmLabel: context.l10n.codeBlocksInsert,
    );
  }
}
