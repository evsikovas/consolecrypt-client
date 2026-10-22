import 'dart:async';

import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/db/db_ddl_dialog.dart';
import 'package:consolecrypt/db/db_designer_models.dart';
import 'package:consolecrypt/db/db_models.dart';
import 'package:consolecrypt/db/db_providers.dart';
import 'package:consolecrypt/db/db_query_service.dart';
import 'package:consolecrypt/vault/vault_passphrase_confirmation.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

/// How the save dialog closed.
enum DbDesignSaveOutcome { applied, openInEditor }

/// The text of a designer failure code (`query_failed`, `policy_denied`…).
String dbDesignErrorText(String? code, AppLocalizations l) => switch (code) {
  'policy_denied' => l.dbQueryBlocked,
  'timeout' => l.dbErrorTimeout,
  'connection_failed' || 'session_closed' => l.dbErrorConnection,
  'approval_required' => l.dbReviewExpired,
  _ => l.dbDesignerStatementFailed,
};

/// «{table} — save changes»: the whole generated script (read-only, with
/// line numbers, highlighting and the wrap switch), and the one
/// confirmation step before it runs — risk notes, the vault passphrase and
/// «Run». «Run» asks the core for a review ticket bound to this exact text
/// and applies it; the session's write state (production window, read-only
/// session) still decides in the core.
class DbDesignSaveDialog extends ConsumerStatefulWidget {
  const DbDesignSaveDialog({
    required this.title,
    required this.plan,
    required this.session,
    required this.review,
    required this.apply,
    required this.onCopy,
    this.discard,
    super.key,
  });
  final String title;
  final DbDesignPlan plan;
  final DbSessionInfo session;
  final Future<DbQueryReview> Function() review;
  final Future<DbDesignApplyResult> Function(String ticket) apply;
  final Future<void> Function(String ticket)? discard;
  final VoidCallback onCopy;
  @override
  ConsumerState<DbDesignSaveDialog> createState() => _DbDesignSaveDialogState();
}

class _DbDesignSaveDialogState extends ConsumerState<DbDesignSaveDialog> with VaultPassphraseCheck<DbDesignSaveDialog> {
  late final _scope = ref.read(dbScopeProvider);
  bool _running = false, _blocked = false, _failed = false, _closing = false;
  DbDesignApplyResult? _result;

  bool get _current => mounted && identical(_scope, ref.read(dbScopeProvider)) && _scope.unlocked;

  Future<void> _run() async {
    if (_running || _closing || !passphraseEntered || !widget.session.writable) return;
    if (!await checkPassphrase() || !_current) return;
    setState(() {
      _running = true;
      _failed = false;
      _blocked = false;
      _result = null;
    });
    String? ticket;
    try {
      final review = await widget.review();
      ticket = review.ticket;
      if (!_current) return;
      if (review.risk == DbQueryRisk.blocked) {
        setState(() => _blocked = true);
        return;
      }
      final result = await widget.apply(review.ticket);
      if (!_current) return;
      if (result.committed) {
        if (!mounted) return;
        _closing = true;
        closeDialog(context, DbDesignSaveOutcome.applied);
        return;
      }
      setState(() => _result = result);
    } catch (_) {
      if (_current) setState(() => _failed = true);
    } finally {
      if (ticket != null && ticket.isNotEmpty) unawaited(widget.discard?.call(ticket).catchError((Object _) {}));
      if (mounted && !_closing) setState(() => _running = false);
    }
  }

  Widget _results(AppLocalizations l, GlassTokens tokens) {
    final result = _result!;
    final style = tokens.typography.caption;
    return Column(
      key: const ValueKey('db-design-results'),
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        for (var i = 0; i < result.statements.length; i++)
          Padding(
            padding: const EdgeInsets.symmetric(vertical: 2),
            child: Row(
              children: [
                Icon(
                  switch (result.statements[i].status) {
                    DbDesignStatementStatus.done => Icons.check_circle_outline_rounded,
                    DbDesignStatementStatus.failed => Icons.error_outline_rounded,
                    DbDesignStatementStatus.rolledBack => Icons.undo_rounded,
                    DbDesignStatementStatus.skipped => Icons.remove_circle_outline_rounded,
                  },
                  size: 14,
                  color: switch (result.statements[i].status) {
                    DbDesignStatementStatus.done => tokens.palette.success,
                    DbDesignStatementStatus.failed => tokens.palette.danger,
                    _ => tokens.secondaryLabel,
                  },
                ),
                const SizedBox(width: GlassSpacing.s6),
                Expanded(
                  child: Text(
                    l.dbDesignerStatementResult(i + 1, switch (result.statements[i].status) {
                      DbDesignStatementStatus.done => l.dbDesignerStatusDone,
                      DbDesignStatementStatus.failed => dbDesignErrorText(result.statements[i].errorCode, l),
                      DbDesignStatementStatus.rolledBack => l.dbDesignerStatusRolledBack,
                      DbDesignStatementStatus.skipped => l.dbDesignerStatusSkipped,
                    }),
                    style: style.copyWith(
                      color: result.statements[i].status == DbDesignStatementStatus.failed
                          ? tokens.palette.danger
                          : tokens.secondaryLabel,
                    ),
                  ),
                ),
              ],
            ),
          ),
      ],
    );
  }

  @override
  Widget build(BuildContext context) {
    ref.listen(dbScopeProvider, (_, next) {
      if (identical(next, _scope)) return;
      passphrase.clear();
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (mounted) closeDialog<DbDesignSaveOutcome>(context);
      });
    });
    if (!_scope.unlocked || !identical(_scope, ref.watch(dbScopeProvider))) return const SizedBox.shrink();
    final l = context.l10n, tokens = GlassTokens.of(context), plan = widget.plan, session = widget.session;
    final writable = session.writable;
    return GlassDialog(
      key: const ValueKey('db-design-save-dialog'),
      title: l.dbDesignerSaveTitle(widget.title),
      icon: plan.destructive ? Icons.warning_rounded : Icons.save_outlined,
      iconTone: plan.destructive ? GlassTone.danger : GlassTone.warning,
      width: 820,
      content: SingleChildScrollView(
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Text(
              l.dbDesignerSqlPreview,
              style: tokens.typography.caption.copyWith(color: tokens.secondaryLabel, fontWeight: FontWeight.w600),
            ),
            const SizedBox(height: GlassSpacing.s6),
            DbCodeBlock(key: const ValueKey('db-design-save-sql'), text: plan.script, maxHeight: 360),
            const SizedBox(height: GlassSpacing.s8),
            Text(
              plan.transactional ? l.dbDesignerTransactional : l.dbDesignerNotTransactional,
              key: const ValueKey('db-design-transaction-note'),
              style: tokens.typography.caption.copyWith(color: tokens.secondaryLabel),
            ),
            if (plan.destructive) ...[
              const SizedBox(height: GlassSpacing.s8),
              InfoBanner(
                key: const ValueKey('db-design-destructive'),
                tone: BannerTone.danger,
                message: l.dbDesignerDestructive,
              ),
            ],
            if (!writable) ...[
              const SizedBox(height: GlassSpacing.s8),
              InfoBanner(
                key: const ValueKey('db-design-read-only'),
                tone: BannerTone.warning,
                message: session.production ? l.dbDesignerProductionLocked : l.dbDesignerReadOnly,
              ),
            ],
            if (_blocked || _failed) ...[
              const SizedBox(height: GlassSpacing.s8),
              InfoBanner(tone: BannerTone.danger, message: _blocked ? l.dbQueryBlocked : l.dbOperationFailed),
            ],
            if (_result != null) ...[
              const SizedBox(height: GlassSpacing.s8),
              InfoBanner(
                tone: BannerTone.danger,
                message: plan.transactional ? l.dbDesignerFailedRolledBack : l.dbDesignerFailedPartly,
              ),
              const SizedBox(height: GlassSpacing.s6),
              _results(l, tokens),
            ],
            if (writable) ...[
              const SizedBox(height: GlassSpacing.s12),
              Text(l.dbDesignerConfirm),
              const SizedBox(height: GlassSpacing.s8),
              passphraseField(
                key: const ValueKey('db-design-passphrase'),
                enabled: !_running,
                onSubmitted: (_) => unawaited(_run()),
              ),
            ],
          ],
        ),
      ),
      secondaryActions: [
        GlassButton(
          key: const ValueKey('db-design-cancel'),
          label: l.commonCancel,
          onPressed: _running ? null : () => closeDialog<DbDesignSaveOutcome>(context),
        ),
        GlassButton(
          key: const ValueKey('db-design-copy'),
          label: l.commonCopy,
          icon: Icons.content_copy_rounded,
          onPressed: widget.onCopy,
        ),
        GlassButton(
          key: const ValueKey('db-design-open-editor'),
          label: l.dbOpenInEditor,
          icon: Icons.edit_note_rounded,
          onPressed: _running ? null : () => closeDialog(context, DbDesignSaveOutcome.openInEditor),
        ),
      ],
      primaryAction: GlassButton.destructive(
        key: const ValueKey('db-design-run'),
        label: l.dbRun,
        busy: _running || passphraseBusy,
        onPressed: !writable || _running || passphraseBusy || !passphraseEntered ? null : () => unawaited(_run()),
      ),
    );
  }
}
