import 'dart:async';

import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/db/db_models.dart';
import 'package:consolecrypt/db/db_providers.dart';
import 'package:consolecrypt/db/db_query_service.dart';
import 'package:consolecrypt/vault/vault_passphrase_confirmation.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

/// Opens a temporary production write window: the vault passphrase (checked
/// by the core) and the length. Returns the
/// updated session, or null when cancelled.
class DbProdUnlockDialog extends ConsumerStatefulWidget {
  const DbProdUnlockDialog({required this.service, required this.session, required this.connectionName, super.key});
  final DbProductionWriteService service;
  final DbSessionInfo session;
  final String connectionName;
  @override
  ConsumerState<DbProdUnlockDialog> createState() => _DbProdUnlockDialogState();
}

class _DbProdUnlockDialogState extends ConsumerState<DbProdUnlockDialog> {
  static const lengths = [5, 15, 30];
  late final _scope = ref.read(dbScopeProvider);
  final _passphrase = TextEditingController();
  int _minutes = 15;
  bool _busy = false, _wrong = false, _failed = false;

  @override
  void dispose() {
    _passphrase
      ..clear()
      ..dispose();
    super.dispose();
  }

  bool get _ready => !_busy && _passphrase.text.isNotEmpty;

  Future<void> _submit() async {
    if (!_ready) return;
    setState(() {
      _busy = true;
      _wrong = false;
      _failed = false;
    });
    final secret = SecretText(_passphrase.text);
    _passphrase.clear();
    try {
      final updated = await widget.service.unlockProduction(widget.session.id, passphrase: secret, minutes: _minutes);
      if (mounted && identical(_scope, ref.read(dbScopeProvider))) closeDialog(context, updated);
    } on DbWrongPassphrase {
      if (mounted) setState(() => _wrong = true);
    } catch (_) {
      if (mounted) setState(() => _failed = true);
    } finally {
      secret.wipe();
      if (mounted) setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    ref.listen(dbScopeProvider, (_, next) {
      if (!identical(next, _scope)) {
        _passphrase.clear();
        WidgetsBinding.instance.addPostFrameCallback((_) {
          if (mounted) closeDialog<void>(context);
        });
      }
    });
    if (!_scope.unlocked || !identical(_scope, ref.watch(dbScopeProvider))) return const SizedBox.shrink();
    final l = context.l10n, tokens = GlassTokens.of(context);
    final error = _failed ? l.dbOperationFailed : null;
    return GlassDialog(
      key: const ValueKey('db-prod-unlock-dialog'),
      title: l.dbUnlockProductionTitle,
      content: SizedBox(
        width: 460,
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            InfoBanner(tone: BannerTone.danger, message: l.dbUnlockProductionWarning(widget.connectionName)),
            const SizedBox(height: GlassSpacing.s12),
            VaultPassphraseField(
              key: const ValueKey('db-prod-passphrase'),
              controller: _passphrase,
              wrong: _wrong,
              enabled: !_busy,
              onChanged: (_) => setState(() => _wrong = false),
              onSubmitted: (_) => unawaited(_submit()),
            ),
            const SizedBox(height: GlassSpacing.s12),
            Text(l.dbUnlockFor, style: tokens.typography.caption.copyWith(color: tokens.secondaryLabel)),
            const SizedBox(height: GlassSpacing.s6),
            GlassSegmented<int>(
              key: const ValueKey('db-prod-minutes'),
              inChrome: false,
              expand: true,
              segments: [for (final minutes in lengths) GlassSegment(value: minutes, label: l.dbMinutes(minutes))],
              selected: _minutes,
              onChanged: (value) => setState(() => _minutes = value),
            ),
            if (error != null) ...[
              const SizedBox(height: GlassSpacing.s8),
              Text(
                error,
                key: const ValueKey('db-prod-error'),
                style: tokens.typography.caption.copyWith(color: tokens.palette.danger),
              ),
            ],
          ],
        ),
      ),
      secondaryActions: [
        GlassButton(label: l.commonCancel, onPressed: _busy ? null : () => closeDialog<void>(context)),
      ],
      primaryAction: GlassButton.destructive(
        key: const ValueKey('db-prod-unlock-submit'),
        label: l.dbUnlockProduction,
        busy: _busy,
        onPressed: _ready ? () => unawaited(_submit()) : null,
      ),
    );
  }
}
