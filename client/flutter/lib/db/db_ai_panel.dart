import 'dart:async';

import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/db/db_ai_service.dart';
import 'package:consolecrypt/db/db_models.dart';
import 'package:consolecrypt/db/db_providers.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

class DbAiPanel extends ConsumerStatefulWidget {
  const DbAiPanel({
    required this.service,
    required this.session,
    required this.query,
    required this.onDraft,
    this.object,
    this.errorCode,
    this.queryId,
    this.resultIndex,
    this.selectedRows = const [],
    this.plan,
    super.key,
  });
  final DbAiService service;
  final DbSessionInfo session;
  final String Function() query;
  final ValueChanged<String> onDraft;
  final DbObjectRef? object;
  final String? errorCode, queryId;
  final int? resultIndex;
  final List<int> selectedRows;
  final DbJson? plan;
  @override
  ConsumerState<DbAiPanel> createState() => _DbAiPanelState();
}

class _DbAiPanelState extends ConsumerState<DbAiPanel> {
  late final _scope = ref.read(dbScopeProvider);
  final _question = TextEditingController(), _contextLimit = TextEditingController(text: '12');
  final _focus = FocusNode();
  final _events = <DbAiEvent>[];
  List<DbAiProvider> _providers = [];
  String? _provider, _ticket, _conversation;
  DbAiAction _action = DbAiAction.compose;
  DbEngine _target = DbEngine.postgresql;
  bool _busy = false, _failed = false;
  int _generation = 0, _bytes = 0;
  StreamSubscription<DbAiEvent>? _subscription;
  bool get _current => mounted && _scope.unlocked && identical(_scope, ref.read(dbScopeProvider));
  @override
  void initState() {
    super.initState();
    unawaited(_load());
  }

  Future<void> _load() async {
    try {
      final providers = await widget.service.aiProviders();
      if (_current) {
        setState(() {
          _providers = providers;
          _provider = providers.firstOrNull?.id;
        });
      }
    } catch (_) {
      if (_current) setState(() => _failed = true);
    }
  }

  void _wipe() {
    for (final event in _events) {
      event.wipe();
    }
    _events.clear();
    _bytes = 0;
  }

  Future<void> _cancel({bool revoke = false}) async {
    _generation++;
    final ticket = _ticket;
    _ticket = null;
    unawaited(_subscription?.cancel() ?? Future<void>.value());
    _subscription = null;
    if (ticket != null) {
      try {
        await widget.service.cancelAi(widget.session.id, ticket);
      } catch (_) {}
    }
    if (revoke) {
      try {
        await widget.service.revokeAiConsent(widget.session.id);
      } catch (_) {}
    }
    if (_current) setState(() => _busy = false);
  }

  @override
  void dispose() {
    unawaited(_cancel(revoke: true));
    _wipe();
    _question.clear();
    _question.dispose();
    _contextLimit.dispose();
    _focus.dispose();
    super.dispose();
  }

  Future<void> _ask() async {
    if (!_current || _busy || _provider == null || _question.text.trim().isEmpty) return;
    final limit = int.tryParse(_contextLimit.text);
    if (limit == null || limit < 1 || limit > 100) return;
    final generation = ++_generation;
    setState(() {
      _busy = true;
      _failed = false;
    });
    try {
      final review = await widget.service.prepareAi(
        widget.session.id,
        _provider!,
        DbAiRequest(
          action: _action,
          question: _question.text,
          query: widget.query(),
          errorCode: widget.errorCode,
          plan: widget.plan,
          object: widget.object,
          targetEngine: _action == DbAiAction.translate ? _target : null,
          conversationId: _conversation,
          queryId: widget.queryId,
          resultIndex: widget.resultIndex,
          selectedRows: List.of(widget.selectedRows),
          contextLimit: limit,
        ),
      );
      if (!_current || generation != _generation) {
        await widget.service.cancelAi(widget.session.id, review.ticket);
        return;
      }
      _ticket = review.ticket;
      if (!mounted) return;
      final consent = await showAppDialog<DbAiConsent>(
        context,
        secure: true,
        builder: (_) => _ConsentDialog(review: review),
      );
      if (!_current || generation != _generation) {
        await widget.service.cancelAi(widget.session.id, review.ticket);
        return;
      }
      if (consent == null || !DateTime.now().isBefore(review.expiresAt)) {
        await _cancel();
        return;
      }
      _wipe();
      _subscription = widget.service
          .askAi(widget.session.id, review.ticket, consent)
          .listen(
            (event) {
              if (!_current || generation != _generation) {
                event.wipe();
                return;
              }
              _bytes += event.content?.byteLength ?? 0;
              if (_bytes > 2 * 1024 * 1024 || _events.length >= 2000) {
                event.wipe();
                unawaited(_cancel());
                setState(() => _failed = true);
                return;
              }
              setState(() {
                if (event.kind == DbAiEventKind.conversation) _conversation = event.conversationId;
                if (event.kind == DbAiEventKind.error) {
                  _failed = true;
                  _busy = false;
                }
                if (event.kind == DbAiEventKind.done) _busy = false;
                _events.add(event);
              });
            },
            onError: (Object _) {
              if (_current && generation == _generation) {
                setState(() {
                  _busy = false;
                  _failed = true;
                });
              }
            },
            onDone: () {
              if (_current && generation == _generation) setState(() => _busy = false);
            },
          );
    } catch (_) {
      if (_current && generation == _generation) {
        setState(() {
          _busy = false;
          _failed = true;
        });
      }
    }
  }

  String _label(DbAiAction action, AppLocalizations l) => switch (action) {
    DbAiAction.compose => l.dbAiCompose,
    DbAiAction.explain => l.dbAiExplain,
    DbAiAction.fix => l.dbAiFix,
    DbAiAction.optimize => l.dbAiOptimize,
    DbAiAction.translate => l.dbAiTranslate,
    DbAiAction.snippet => l.dbAiSnippet,
    DbAiAction.examples => l.dbAiExamples,
    DbAiAction.result => l.dbAiResult,
  };
  @override
  Widget build(BuildContext context) {
    ref.listen(dbScopeProvider, (_, next) {
      if (!identical(next, _scope)) {
        _wipe();
        _question.clear();
        unawaited(_cancel(revoke: true));
      }
    });
    ref.watch(dbScopeProvider);
    if (!_current) return const SizedBox.shrink();
    final l = context.l10n;
    final drafts = _events.where((e) => e.kind == DbAiEventKind.draft && e.content?.isWiped == false).toList();
    final ddl = drafts.any((e) => e.isDdl), draft = drafts.map((e) => e.content!.expose()).join();
    return LayoutBuilder(
      builder: (context, bounds) => Column(
        children: [
          ConstrainedBox(
            constraints: BoxConstraints(maxHeight: bounds.maxHeight * .6),
            child: SingleChildScrollView(
              child: Padding(
                padding: const EdgeInsets.all(8),
                child: Column(
                  children: [
                    GlassDropdownField<String>(
                      value: _provider,
                      label: l.dbAiProvider,
                      items: [for (final p in _providers) GlassSelectItem(value: p.id, label: p.name)],
                      onChanged: _busy ? null : (v) => setState(() => _provider = v),
                    ),
                    GlassDropdownField<DbAiAction>(
                      value: _action,
                      label: l.dbAi,
                      items: [for (final a in DbAiAction.values) GlassSelectItem(value: a, label: _label(a, l))],
                      onChanged: _busy ? null : (v) => setState(() => _action = v),
                    ),
                    if (_action == DbAiAction.translate)
                      GlassDropdownField<DbEngine>(
                        value: _target,
                        items: [for (final e in DbEngine.values) GlassSelectItem(value: e, label: e.label)],
                        onChanged: _busy ? null : (v) => setState(() => _target = v),
                      ),
                    TextField(
                      controller: _contextLimit,
                      keyboardType: TextInputType.number,
                      decoration: InputDecoration(labelText: l.dbAiContextLimit),
                    ),
                    TextField(
                      key: const ValueKey('db-ai-question'),
                      controller: _question,
                      focusNode: _focus,
                      minLines: 2,
                      maxLines: 4,
                      autocorrect: false,
                      enableSuggestions: false,
                      decoration: InputDecoration(labelText: l.dbAiQuestion),
                    ),
                    Wrap(
                      spacing: 8,
                      children: [
                        GlassButton(label: l.dbAi, onPressed: _busy ? null : _ask),
                        if (_busy) GlassButton(label: l.commonCancel, onPressed: _cancel),
                        IconButton(
                          tooltip: l.dbAiRevoke,
                          icon: const Icon(Icons.privacy_tip_outlined),
                          onPressed: () => _cancel(revoke: true),
                        ),
                        IconButton(
                          tooltip: l.dbAiNewConversation,
                          icon: const Icon(Icons.add_comment_outlined),
                          onPressed: _busy
                              ? null
                              : () => setState(() {
                                  _wipe();
                                  _conversation = null;
                                }),
                        ),
                      ],
                    ),
                  ],
                ),
              ),
            ),
          ),
          if (_busy) const LinearProgressIndicator(),
          if (_failed) Text(l.dbOperationFailed),
          Expanded(
            child: ListView(
              children: [
                for (final event in _events.where(
                  (e) =>
                      e.kind != DbAiEventKind.draft &&
                      e.kind != DbAiEventKind.done &&
                      e.kind != DbAiEventKind.conversation,
                ))
                  ListTile(
                    title: Text(
                      event.kind == DbAiEventKind.tool || event.kind == DbAiEventKind.context ? l.dbAiTools : l.dbAi,
                    ),
                    subtitle: SelectableText(
                      event.content?.isWiped == false ? event.content!.expose() : (event.tool ?? ''),
                    ),
                  ),
                if (draft.isNotEmpty)
                  Card(
                    child: Padding(
                      padding: const EdgeInsets.all(12),
                      child: Column(
                        crossAxisAlignment: CrossAxisAlignment.stretch,
                        children: [
                          Text(l.dbAiDraft),
                          SelectableText(draft, style: const TextStyle(fontFamily: 'monospace')),
                          Text(l.dbAiNoExecute),
                          if (widget.session.production && ddl) Text(l.dbAiProdDdl),
                          Wrap(
                            spacing: 8,
                            children: [
                              GlassButton(
                                key: const ValueKey('db-ai-accept'),
                                label: l.dbAiAccept,
                                onPressed: _busy || (widget.session.production && ddl)
                                    ? null
                                    : () {
                                        if (_current) widget.onDraft(draft);
                                      },
                              ),
                              GlassButton(label: l.dbAiReject, onPressed: () => setState(_wipe)),
                              GlassButton(label: l.dbAiRefine, onPressed: _focus.requestFocus),
                            ],
                          ),
                        ],
                      ),
                    ),
                  ),
              ],
            ),
          ),
        ],
      ),
    );
  }
}

class _ConsentDialog extends ConsumerStatefulWidget {
  const _ConsentDialog({required this.review});
  final DbAiReview review;
  @override
  ConsumerState<_ConsentDialog> createState() => _ConsentDialogState();
}

class _ConsentDialogState extends ConsumerState<_ConsentDialog> {
  late final _scope = ref.read(dbScopeProvider);
  bool _schema = false, _rows = false, _sample = false, _readonly = false;
  Timer? _expiry;
  @override
  void initState() {
    super.initState();
    _expiry = Timer(widget.review.expiresAt.difference(DateTime.now()), () {
      if (mounted) setState(() {});
    });
  }

  @override
  void dispose() {
    _expiry?.cancel();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n, review = widget.review;
    ref.listen(dbScopeProvider, (_, next) {
      if (!identical(next, _scope)) {
        WidgetsBinding.instance.addPostFrameCallback((_) {
          if (mounted) closeDialog<void>(context);
        });
      }
    });
    if (!_scope.unlocked || !identical(_scope, ref.watch(dbScopeProvider))) return const SizedBox.shrink();
    final expired = !DateTime.now().isBefore(review.expiresAt);
    return GlassDialog(
      title: l.dbAiConsent,
      content: SizedBox(
        width: 600,
        child: SingleChildScrollView(
          child: Column(
            children: [
              Text('${review.provider.name} · ${review.connectionName}'),
              Text(review.provider.external ? l.dbAiExternal : l.dbAiLocal),
              Text(l.dbAiConsentHelp),
              if (review.historyMessages > 0) Text('${l.dbAiHistoryMessages}: ${review.historyMessages}'),
              CheckboxListTile(
                key: const ValueKey('db-ai-consent-schema'),
                title: Text(l.dbAiSchema),
                subtitle: review.schemaObjects.isEmpty ? null : Text(review.schemaObjects.join(', ')),
                value: _schema,
                onChanged: (v) => setState(() => _schema = v!),
              ),
              CheckboxListTile(
                key: const ValueKey('db-ai-consent-rows'),
                title: Text(l.dbAiRows),
                value: _rows,
                onChanged: review.hasSelectedRows ? (v) => setState(() => _rows = v!) : null,
              ),
              CheckboxListTile(
                key: const ValueKey('db-ai-consent-sample'),
                title: Text(l.dbAiSample),
                value: _sample,
                onChanged: (v) => setState(() => _sample = v!),
              ),
              CheckboxListTile(
                key: const ValueKey('db-ai-consent-run'),
                title: Text(l.dbAiReadonly),
                value: _readonly,
                onChanged: (v) => setState(() => _readonly = v!),
              ),
              if (expired) Text(l.dbReviewExpired),
            ],
          ),
        ),
      ),
      secondaryActions: [GlassButton(label: l.commonCancel, onPressed: () => closeDialog<void>(context))],
      primaryAction: GlassButton(
        key: const ValueKey('db-ai-consent-submit'),
        label: l.commonContinue,
        onPressed: expired
            ? null
            : () => closeDialog(
                context,
                DbAiConsent(schema: _schema, selectedRows: _rows, sampleRows: _sample, runReadonly: _readonly),
              ),
      ),
    );
  }
}
