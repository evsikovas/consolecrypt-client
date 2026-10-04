import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/hosts/host_picker.dart';
import 'package:consolecrypt/hosts/hosts_screen.dart';
import 'package:consolecrypt/rdp/rdp_guard.dart';
import 'package:consolecrypt/rdp/rdp_providers.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import 'package:material_ui/material_ui.dart';

/// The global action offers both protocols; workspace actions can restrict
/// the list. Selecting a host uses the same trust/auth flow as Hosts.
Future<void> showConnectionPicker(BuildContext context, WidgetRef ref, {HostProtocol? protocol}) async {
  final scope = ref.read(rdpScopeProvider);
  if (!scope.unlocked) return;
  final host = await showAppDialog<Host>(
    context,
    builder: (_) => RdpScopeGuard(
      scope: scope,
      child: HostPickerDialog(
        protocol: protocol,
        onCreate: () {
          if (!context.mounted || !rdpScopeCurrent(ref, scope)) return;
          context.go(protocol == null ? AppRoutes.newHost : '${AppRoutes.newHost}?protocol=${protocol.name}');
        },
      ),
    ),
  );
  if (host == null || !context.mounted || !rdpScopeCurrent(ref, scope)) return;
  await connectToHost(context, ref, host);
}
