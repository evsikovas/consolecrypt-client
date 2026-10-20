import 'dart:async';

import 'package:consolecrypt/ai/device_local_ai_service.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/services/ai_service.dart' show deviceLocalAiProviderId;
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/kubernetes/kube_ai_history.dart';
import 'package:consolecrypt/kubernetes/kube_ai_proposals.dart';
import 'package:consolecrypt/kubernetes/kube_ai_service.dart';
import 'package:consolecrypt/kubernetes/kube_browser_service.dart';
import 'package:consolecrypt/kubernetes/kube_providers.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

class KubeAiPanel extends ConsumerStatefulWidget {
  const KubeAiPanel({
    required this.session,
    required this.contextName,
    required this.service,
    this.selected,
    this.initialQuestion,
    this.namespace = 'default',
    this.onReviewedCommand,
    super.key,
  });
  final KubeSession session;
  final String contextName;
  final KubeAiService service;
  final KubeResourceRef? selected;
  final String? initialQuestion;
  final String namespace;
  final Future<void> Function(String runtimeId, String command)? onReviewedCommand;
  @override
  ConsumerState<KubeAiPanel> createState() => _KubeAiPanelState();
}

class _KubeAiPanelState extends ConsumerState<KubeAiPanel> with WidgetsBindingObserver {
  late final KubeScope _scope = ref.read(kubeScopeProvider);
  final _question = TextEditingController();
  final _events = <KubeAiEvent>[];
  String? _providerId, _ticket, _conversationId;
  bool _configMapValues = false, _busy = false, _failed = false;
  int _generation = 0, _characters = 0;
  Future<void> _pendingStop = Future.value();
  // Cancelled on stop, lock, background and disposal.
  // ignore: cancel_subscriptions
  StreamSubscription<KubeAiEvent>? _subscription;
  Completer<void>? _answer;
  bool get _current => mounted && _scope.unlocked && identical(_scope, ref.read(kubeScopeProvider));
  @override
  void initState() {
    super.initState();
    _question.text = widget.initialQuestion ?? '';
    WidgetsBinding.instance.addObserver(this);
    // Manual listener stays active when the panel is covered by another route.
    ref.listenManual(kubeScopeProvider, (_, next) {
      if (identical(_scope, next)) return;
      unawaited(_stop());
      _question.clear();
      _events.clear();
      _conversationId = null;
    });
  }

  @override
  void didUpdateWidget(KubeAiPanel old) {
    super.didUpdateWidget(old);
    if (old.initialQuestion != widget.initialQuestion && widget.initialQuestion != null) {
      _question.text = widget.initialQuestion!;
    }
  }

  @override
  void dispose() {
    WidgetsBinding.instance.removeObserver(this);
    unawaited(_stop());
    _question.clear();
    _question.dispose();
    _events.clear();
    _conversationId = null;
    super.dispose();
  }

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    if (state == AppLifecycleState.paused || state == AppLifecycleState.hidden || state == AppLifecycleState.detached) {
      unawaited(_stop());
      if (mounted) setState(() => _busy = false);
    }
  }

  Future<void> _stop() {
    _generation++;
    final subscription = _subscription;
    _subscription = null;
    final answer = _answer;
    _answer = null;
    if (answer != null && !answer.isCompleted) answer.complete();
    final ticket = _ticket;
    _ticket = null;
    final service = widget.service, sessionId = widget.session.id;
    return _pendingStop = _pendingStop
        .then((_) async {
          // Revoke native work before awaiting stream teardown: a polling stream
          // may need that revocation to finish its pending read.
          await service.revokeKubeAi(sessionId).catchError((Object _) {});
          if (ticket != null) await service.discardKubeAi(sessionId, ticket).catchError((Object _) {});
          if (subscription != null) await subscription.cancel();
        })
        .catchError((Object _) {});
  }

  Future<void> _ask() async {
    final question = _question.text.trim();
    if (!_current || _busy || question.isEmpty) return;
    final stopping = _stop();
    final generation = _generation;
    setState(() {
      _busy = true;
      _failed = false;
      if (_conversationId == null) {
        _events.clear();
        _characters = 0;
      }
      _events.add(KubeAiEvent('question', {'content': question}));
      _characters += question.length;
    });
    try {
      await stopping;
      if (!_current || generation != _generation) return;
      // "Default provider" must mean the provider the rest of the app uses as
      // default, including the built-in model chosen on this device; the
      // native core alone only knows the vault's saved default.
      final providers = ref.read(aiProvidersProvider).value ?? const [];
      final providerId = _providerId ?? providers.where((p) => p.isDefault).firstOrNull?.id.value;
      final ai = providerId == deviceLocalAiProviderId.value ? ref.read(aiServiceProvider) : null;
      if (ai is DeviceLocalAiService) {
        await ai.withDeviceLocalProvider(() => _run(question, generation, providerId));
      } else {
        await _run(question, generation, providerId);
      }
    } catch (_) {
      if (_current && generation == _generation) {
        unawaited(_stop());
        setState(() {
          _failed = true;
          _busy = false;
        });
      }
    }
  }

  /// Prepares, reviews and streams one answer; completes when it ends.
  Future<void> _run(String question, int generation, String? providerId) async {
    {
      final service = widget.service;
      final review = _conversationId != null && service is KubeAiChatService
          ? await (service as KubeAiChatService).prepareKubeFollowUp(
              widget.session.id,
              _conversationId!,
              providerId: providerId,
              question: question,
              selected: widget.selected,
              configMapValues: _configMapValues,
            )
          : await service.prepareKubeAi(
              widget.session.id,
              providerId: providerId,
              question: question,
              selected: widget.selected,
              configMapValues: _configMapValues,
            );
      if (!mounted || !_current || generation != _generation) {
        await widget.service.discardKubeAi(widget.session.id, review.ticket);
        return;
      }
      _ticket = review.ticket;
      var approved = !review.external;
      if (review.external) {
        approved =
            await showAppDialog<bool>(
              context,
              secure: true,
              builder: (_) => _AiConsent(scope: _scope, review: review, configMapValues: _configMapValues),
            ) ==
            true;
      }
      if (!_current || generation != _generation || !approved) {
        unawaited(_stop());
        if (_current) setState(() => _busy = false);
        return;
      }
      final answer = _answer = Completer<void>();
      _subscription = widget.service
          .askKubeAi(widget.session.id, review, approveExternal: approved)
          .listen(
            (event) {
              if (!_current || generation != _generation) return;
              setState(() {
                if (event.type == 'conversation') {
                  _conversationId = event.data['conversation_id'] as String?;
                  return;
                }
                if (event.type == 'done') {
                  _busy = false;
                  _ticket = null;
                  _question.clear();
                  return;
                }
                if (event.type == 'error') _failed = true;
                // Bound rendered context independently from the native stream.
                while (_events.isNotEmpty &&
                    (_events.length >= 64 || _characters + event.content.length > 2 * 1024 * 1024)) {
                  _characters -= _events.removeAt(0).content.length;
                }
                if (event.content.length > 2 * 1024 * 1024) {
                  _failed = true;
                  return;
                }
                _characters += event.content.length;
                _events.add(event);
              });
            },
            onError: (Object _) {
              if (!answer.isCompleted) answer.complete();
              if (_current && generation == _generation) {
                setState(() {
                  _failed = true;
                  _busy = false;
                });
              }
            },
            onDone: () {
              if (!answer.isCompleted) answer.complete();
              if (_current && generation == _generation) setState(() => _busy = false);
            },
          );
      await answer.future;
    }
  }

  @override
  Widget build(BuildContext context) {
    ref.watch(kubeScopeProvider);
    if (!_current) return const SizedBox.shrink();
    final l = context.l10n;
    final providers = ref.watch(aiProvidersProvider).value ?? const [];
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        ConstrainedBox(
          constraints: const BoxConstraints(maxHeight: 170),
          child: SingleChildScrollView(
            child: ExpansionTile(
              key: const ValueKey('kube-ai-options'),
              tilePadding: EdgeInsets.zero,
              title: Text('${l.kubeAiAssistant} · ${widget.contextName}'),
              trailing: widget.service is KubeAiHistoryService
                  ? GlassIconButton(
                      key: const ValueKey('kube-ai-history'),
                      icon: Icons.history,
                      tooltip: l.kubeAiHistory,
                      onPressed: () => showAppDialog<void>(
                        context,
                        secure: true,
                        builder: (_) => KubeAiHistoryDialog(
                          contextId: widget.session.contextId,
                          service: widget.service as KubeAiHistoryService,
                          onResume: (id, events) {
                            if (!_current) return;
                            unawaited(_stop());
                            setState(() {
                              _conversationId = id;
                              _events
                                ..clear()
                                ..addAll(events.take(64));
                              _characters = _events.fold(0, (total, event) => total + event.content.length);
                              _question.clear();
                              _busy = false;
                              _failed = false;
                            });
                          },
                        ),
                      ),
                    )
                  : null,
              children: [
                Text(l.kubeAiHelp),
                if (_conversationId != null)
                  TextButton(
                    key: const ValueKey('kube-ai-new-conversation'),
                    onPressed: _busy
                        ? null
                        : () {
                            unawaited(_stop());
                            setState(() {
                              _conversationId = null;
                              _events.clear();
                              _characters = 0;
                              _question.clear();
                            });
                          },
                    child: Text(l.kubeAiNewConversation),
                  ),
                if (widget.selected != null)
                  Text(
                    /* l10n-ignore: Kubernetes kind, namespace and resource name. */ '${widget.selected!.api.kind} · ${widget.selected!.namespace ?? ''}/${widget.selected!.name}',
                  ), // l10n-ignore: Kubernetes kind, namespace and resource name.
                DropdownButtonFormField<String>(
                  key: const ValueKey('kube-ai-provider'),
                  initialValue: _providerId ?? '',
                  isExpanded: true,
                  decoration: InputDecoration(labelText: l.kubeAiProvider),
                  items: [
                    DropdownMenuItem(value: '', child: Text(l.kubeAiDefaultProvider)),
                    ...providers.map(
                      (provider) => DropdownMenuItem(
                        value: provider.id.value,
                        child: Text(provider.name, overflow: TextOverflow.ellipsis),
                      ),
                    ),
                  ],
                  onChanged: _busy ? null : (value) => setState(() => _providerId = value == '' ? null : value),
                ),
                CheckboxListTile(
                  key: const ValueKey('kube-ai-configmaps'),
                  contentPadding: EdgeInsets.zero,
                  title: Text(l.kubeAiConfigMaps),
                  value: _configMapValues,
                  onChanged: _busy ? null : (v) => setState(() => _configMapValues = v == true),
                ),
              ],
            ),
          ),
        ),
        Expanded(
          child: ListView.builder(
            key: const ValueKey('kube-ai-messages'),
            itemCount: _events.length,
            itemBuilder: (context, index) {
              final event = _events[index];
              if (event.type == 'answer') {
                return Padding(
                  padding: const EdgeInsets.all(8),
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.stretch,
                    children: [
                      KubeAiProposals(
                        answer: event.content,
                        session: widget.session,
                        contextName: widget.contextName,
                        namespace: widget.namespace,
                        service: widget.service,
                        selected: widget.selected,
                        unresolved: (event.data['unresolved'] as List? ?? const []).isNotEmpty,
                        onReviewedCommand: widget.onReviewedCommand,
                      ),
                      if ((event.data['unresolved'] as List? ?? const []).isNotEmpty) Text(l.kubeAiUnresolved),
                    ],
                  ),
                );
              }
              if (event.type == 'question') {
                return Padding(
                  padding: const EdgeInsets.all(8),
                  child: SelectableText(event.content, style: Theme.of(context).textTheme.titleSmall),
                );
              }
              if (event.type == 'context') {
                return ExpansionTile(
                  title: Text(event.tool ?? l.kubeAiReadTool),
                  subtitle: event.truncated ? Text(l.kubeAiTruncated) : null,
                  children: [SelectableText(event.content)],
                );
              }
              if (event.type == 'tool_error') {
                return ListTile(
                  leading: const Icon(Icons.warning_amber_outlined),
                  title: Text(event.tool ?? l.kubeAiReadTool),
                  subtitle: Text(l.kubeAiToolFailed),
                );
              }
              return const SizedBox.shrink();
            },
          ),
        ),
        if (_failed) Text(l.kubeAiFailed, style: TextStyle(color: Theme.of(context).colorScheme.error)),
        if (_busy) const LinearProgressIndicator(),
        Row(
          crossAxisAlignment: CrossAxisAlignment.end,
          children: [
            Expanded(
              child: TextField(
                key: const ValueKey('kube-ai-question'),
                controller: _question,
                enabled: !_busy,
                minLines: 1,
                maxLines: 3,
                maxLength: 16000,
                autocorrect: false,
                enableSuggestions: false,
                enableIMEPersonalizedLearning: false,
                decoration: InputDecoration(labelText: l.kubeAiQuestion, counterText: ''),
                onChanged: (_) => setState(() {}),
                onSubmitted: (_) => _ask(),
              ),
            ),
            GlassIconButton(
              key: const ValueKey('kube-ai-send'),
              icon: _busy ? Icons.stop_circle_outlined : Icons.send_outlined,
              tooltip: _busy ? l.commonCancel : l.kubeAiAsk,
              onPressed: _busy
                  ? () {
                      unawaited(_stop());
                      setState(() => _busy = false);
                    }
                  : _question.text.trim().isEmpty
                  ? null
                  : _ask,
            ),
          ],
        ),
      ],
    );
  }
}

class _AiConsent extends ConsumerWidget {
  const _AiConsent({required this.scope, required this.review, required this.configMapValues});
  final KubeScope scope;
  final KubeAiReview review;
  final bool configMapValues;
  @override
  Widget build(BuildContext context, WidgetRef ref) {
    ref.listen(kubeScopeProvider, (_, next) {
      if (identical(scope, next)) return;
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (!context.mounted) return;
        final route = ModalRoute.of(context);
        if (route?.isActive == true) Navigator.of(context).removeRoute(route!);
      });
    });
    final l = context.l10n;
    return GlassDialog(
      title: l.kubeAiExternalTitle,
      content: SingleChildScrollView(
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Text('${review.providerName} · ${review.contextName}'),
            Text(l.kubeAiExternalHelp),
            if (review.historyMessages > 0) Text('${l.kubeAiHistoryContext}: ${review.historyMessages}'),
            if (configMapValues) Text(l.kubeAiConfigMaps),
            if (!review.toolSupport) Text(l.kubeAiNoTools),
          ],
        ),
      ),
      primaryAction: GlassButton.prominent(
        key: const ValueKey('kube-ai-consent'),
        label: l.commonContinue,
        onPressed: () => closeDialog(context, true),
      ),
      secondaryActions: [GlassButton(label: l.commonCancel, onPressed: () => closeDialog(context, false))],
    );
  }
}
