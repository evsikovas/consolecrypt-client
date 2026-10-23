import 'dart:async';

import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/db/db_models.dart';
import 'package:consolecrypt/db/db_providers.dart';
import 'package:consolecrypt/db/db_query_service.dart';
import 'package:consolecrypt/vault/vault_passphrase_confirmation.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

/// Opens a temporary production write window: the vault passphrase — or
/// the OS check where the device offers it — (checked by the core) and the
/// length. Returns the updated session, or null when cancelled.
class DbProdUnlockDialog extends ConsumerStatefulWidget {
  const DbProdUnlockDialog({required this.service, required this.session, required this.connectionName, super.key});
  final DbProductionWriteService service;
  final DbSessionInfo session;
  final String connectionName;
  @override
  ConsumerState<DbProdUnlockDialog> createState() => _DbProdUnlockDialogState();
}

class _DbProdUnlockDialogState extends ConsumerState<DbProdUnlockDialog> with VaultPassphraseCheck<DbProdUnlockDialog> {
  static const lengths = [5, 15, 30];
  late final _scope = ref.read(dbScopeProvider);
  int _minutes = 15;
  bool _busy = false, _failed = false;

  @override
  StepUpAction get stepUpAction => StepUpAction.dbProductionWrite;

  @override
  String get stepUpObject => widget.session.id;

  @override
  String get stepUpReason => context.l10n.dbUnlockProductionOsReason(widget.connectionName);

  bool get _ready => !_busy && !stepUpBusy && stepUpReady;

  Future<void> _submit() async {
    if (!_ready) return;
    setState(() {
      _failed = false;
      passphraseWrong = false;
    });
    final proof = await obtainStepUp();
    if (proof == null || !mounted || !identical(_scope, ref.read(dbScopeProvider))) {
      proof?.wipe();
      return;
    }
    setState(() => _busy = true);
    try {
      final updated = await widget.service.unlockProduction(widget.session.id, proof: proof, minutes: _minutes);
      if (mounted && identical(_scope, ref.read(dbScopeProvider))) closeDialog(context, updated);
    } on DbWrongPassphrase {
      if (mounted) stepUpRejected(proof);
    } on AppException catch (e) {
      if (!mounted) return;
      if (e.code == AppErrorCode.passphraseRequired) {
        stepUpRejected(proof);
      } else {
        setState(() => _failed = true);
      }
    } catch (_) {
      if (mounted) setState(() => _failed = true);
    } finally {
      proof.wipe();
      if (mounted) setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    ref.listen(dbScopeProvider, (_, next) {
      if (!identical(next, _scope)) {
        passphrase.clear();
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
            stepUpField(
              key: const ValueKey('db-prod-passphrase'),
              enabled: !_busy,
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
        GlassButton(label: l.commonCancel, onPressed: _busy || stepUpBusy ? null : () => closeDialog<void>(context)),
      ],
      primaryAction: GlassButton.destructive(
        key: const ValueKey('db-prod-unlock-submit'),
        label: l.dbUnlockProduction,
        icon: presenceKind == null ? null : presenceIcon(presenceKind!),
        busy: _busy || stepUpBusy,
        onPressed: _ready ? () => unawaited(_submit()) : null,
      ),
    );
  }
}
