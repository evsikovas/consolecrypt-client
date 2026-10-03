import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/rdp/rdp_controller.dart';
import 'package:consolecrypt/rdp/rdp_guard.dart';
import 'package:consolecrypt/rdp/rdp_providers.dart';
import 'package:consolecrypt/rdp/rdp_screen.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

abstract final class RdpLauncher {
  /// Native probe supplies the fresh complete snapshot. Caller navigates to
  /// the RDP workspace only when a non-null adopted tab is returned.
  static Future<RdpTab?> openSavedHost(BuildContext context, WidgetRef ref, Host host) async {
    final scope = ref.read(rdpScopeProvider);
    if (!scope.unlocked) return null;
    final service = ref.read(rdpServiceProvider);
    final route = ModalRoute.of(context);
    try {
      final ticket = await service.probeSavedHost(host.id.value);
      if (!context.mounted || !rdpScopeCurrent(ref, scope) || route?.isCurrent == false) return null;
      return await showAppDialog<RdpTab>(
        context,
        secure: true,
        builder: (_) => RdpConnectionDialog(scope: scope, savedTicket: ticket),
      );
    } catch (_) {
      if (context.mounted && rdpScopeCurrent(ref, scope)) {
        showSnack(context, context.l10n.rdpConnectionFailed, error: true);
      }
      return null;
    }
  }
}
