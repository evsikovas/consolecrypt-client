import 'dart:convert';

import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/rdp/rdp_controller.dart';
import 'package:consolecrypt/rdp/rdp_guard.dart';
import 'package:consolecrypt/rdp/rdp_providers.dart';
import 'package:consolecrypt/rdp/rdp_service.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

String _clipboardFailureLabel(AppLocalizations l, Object error) => switch (error is RdpFailure ? error.code : null) {
  'clipboard_unavailable' => l.rdpClipboardUnavailable,
  'clipboard_limit' => l.rdpClipboardLimit,
  'input_queue_full' => l.rdpClipboardBusy,
  'clipboard_empty' => l.rdpClipboardEmpty,
  _ => l.rdpClipboardFailed,
};

String _checkedClipboardText(String? text) {
  if (text == null || text.isEmpty) throw const RdpFailure('clipboard_empty');
  if (text.length > 65536 || text.contains('\x00') || utf8.encode(text).length > 65536) {
    throw const RdpFailure('clipboard_limit');
  }
  return text;
}

Future<void> pasteLocalRdpClipboard(
  BuildContext context,
  WidgetRef ref,
  RdpTab tab,
  bool Function() isInputCurrent,
) async {
  final scope = ref.read(rdpScopeProvider);
  final controller = ref.read(rdpWorkspaceProvider);
  final service = ref.read(rdpServiceProvider);
  final epoch = tab.permissionEpoch;
  final interactionEpoch = tab.interactionEpoch;
  final l = context.l10n;
  bool current() =>
      context.mounted &&
      isInputCurrent() &&
      rdpScopeCurrent(ref, scope) &&
      !tab.closed &&
      tab.status.phase == RdpPhase.connected &&
      tab.permissions.clipboardEnabled &&
      tab.permissionEpoch == epoch &&
      tab.interactionEpoch == interactionEpoch &&
      identical(controller.active, tab);
  if (tab.clipboardPastePending || !current()) return;
  tab.setClipboardPastePending(true);
  try {
    await controller.queueClipboardPaste(tab, () async {
      final value = await Clipboard.getData(Clipboard.kTextPlain);
      if (!current()) return;
      final text = _checkedClipboardText(value?.text);
      final ticket = await service.offerClipboardTextConfirmed(tab.info.id, text);
      if (!current()) return;
      await service.commitClipboardPaste(tab.info.id, ticket);
    }, isCurrent: current);
  } catch (error) {
    if (context.mounted && current()) showSnack(context, _clipboardFailureLabel(l, error), error: true);
  } finally {
    tab.setClipboardPastePending(false);
  }
}

/// Text exchange is explicit in both directions. Enabling the channel never
/// reads or sends the system clipboard in the background.
class RdpClipboardActions extends ConsumerStatefulWidget {
  const RdpClipboardActions({required this.tab, super.key});
  final RdpTab tab;
  @override
  ConsumerState<RdpClipboardActions> createState() => _RdpClipboardActionsState();
}

class _RdpClipboardActionsState extends ConsumerState<RdpClipboardActions> {
  bool _busy = false;
  bool _current(RdpScope scope) =>
      mounted &&
      rdpScopeCurrent(ref, scope) &&
      TickerMode.valuesOf(context).enabled &&
      !widget.tab.closed &&
      widget.tab.permissions.clipboardEnabled &&
      widget.tab.status.phase == RdpPhase.connected &&
      identical(ref.read(rdpWorkspaceProvider).active, widget.tab);

  Future<void> _exchange({required bool receive}) async {
    final scope = ref.read(rdpScopeProvider);
    if (_busy || !_current(scope)) return;
    final service = ref.read(rdpServiceProvider);
    final clipboard = ref.read(secureClipboardProvider);
    final sessionId = widget.tab.info.id;
    setState(() => _busy = true);
    try {
      if (receive) {
        await service.requestClipboardText(sessionId);
        String? text;
        for (var attempt = 0; attempt < 50; attempt++) {
          if (!_current(scope)) return;
          text = await service.takeClipboardText(sessionId);
          if (text != null) break;
          await Future<void>.delayed(const Duration(milliseconds: 100));
        }
        if (!_current(scope)) return;
        if (text == null) throw const RdpFailure('clipboard_empty');
        if (utf8.encode(text).length > 65536) throw const RdpFailure('clipboard_limit');
        // The remote text may be a password; respect the existing expiry and
        // sensitive native clipboard path rather than copying it permanently.
        await clipboard.copySecret(text, isCurrent: () => _current(scope));
        if (mounted && _current(scope)) showSnack(context, context.l10n.rdpClipboardReceived);
      } else {
        final value = await Clipboard.getData(Clipboard.kTextPlain);
        if (!_current(scope)) return;
        final text = _checkedClipboardText(value?.text);
        await service.offerClipboardText(sessionId, text);
        if (mounted && _current(scope)) showSnack(context, context.l10n.rdpClipboardSent);
      }
    } catch (error) {
      if (mounted && _current(scope)) {
        final l = context.l10n;
        final message = _clipboardFailureLabel(l, error);
        showSnack(context, message, error: true);
      }
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final enabled =
        !_busy &&
        !widget.tab.clipboardPastePending &&
        widget.tab.permissions.clipboardEnabled &&
        widget.tab.status.phase == RdpPhase.connected;
    return GlassToolbarGroup(
      children: [
        GlassIconButton(
          key: const ValueKey('rdp-send-clipboard'),
          icon: Icons.upload_rounded,
          tooltip: context.l10n.rdpSendClipboard,
          style: GlassIconButtonStyle.plain,
          onPressed: enabled ? () => _exchange(receive: false) : null,
        ),
        GlassIconButton(
          key: const ValueKey('rdp-receive-clipboard'),
          icon: Icons.download_rounded,
          tooltip: context.l10n.rdpReceiveClipboard,
          style: GlassIconButtonStyle.plain,
          onPressed: enabled ? () => _exchange(receive: true) : null,
        ),
      ],
    );
  }
}
