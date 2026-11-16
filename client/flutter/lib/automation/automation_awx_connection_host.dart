import 'package:consolecrypt/automation/automation_awx_connection_form.dart';
import 'package:consolecrypt/automation/automation_awx_controller.dart';
import 'package:consolecrypt/automation/automation_awx_models.dart';
import 'package:consolecrypt/automation/automation_awx_workspace.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/credentials/credential_dialogs.dart';
import 'package:consolecrypt/monitoring/monitoring_models.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

/// Concrete host composition over EXISTING inventory metadata and the secure
/// PasswordCredentialDialog. No AWX API/gateway implementation or active route.
class AwxConnectionWorkspace extends ConsumerWidget {
  const AwxConnectionWorkspace({
    super.key,
    required this.controller,
    required this.copy,
    required this.httpsNotice,
    required this.onOpenResource,
    this.isViewActive,
  });
  final bool Function()? isViewActive;
  final AwxController controller;
  final AwxCopyText copy;
  final String httpsNotice;
  final Future<void> Function(String originalNativeUrl) onOpenResource;
  @override
  Widget build(BuildContext context, WidgetRef ref) {
    // Observe existing profile/vault streams before the first edit click. A
    // first read inside the callback would still be AsyncLoading and correctly
    // refuse; eagerly observing metadata avoids a misleading inert button.
    ref.watch(activeProfileProvider);
    ref.watch(vaultStatusProvider);
    ref.watch(inventoryServiceProvider);
    return AwxWorkspace(
      controller: controller,
      copy: copy,
      onOpenResource: onOpenResource,
      onEditConnection: (original) => showAwxConnectionEditor(
        context,
        ref,
        controller,
        original: original,
        isViewActive: isViewActive,
        title: copy(
          original == null ? AwxCopy.addConnection : AwxCopy.editConnection,
        ),
        disclosure: copy(AwxCopy.localSettings),
        httpsNotice: httpsNotice,
      ),
    );
  }
}

Future<MonitoringSource?> showAwxConnectionEditor(
  BuildContext context,
  WidgetRef ref,
  AwxController owner, {
  required AwxConnection? original,
  bool Function()? isViewActive,
  required String title,
  required String disclosure,
  required String httpsNotice,
}) async {
  final inventory = ref.read(inventoryServiceProvider),
      selected = owner.selected,
      catalog = owner.catalog;
  bool current() =>
      context.mounted &&
      (isViewActive?.call() ?? true) &&
      owner.current &&
      identical(ref.read(inventoryServiceProvider), inventory) &&
      ref.read(activeProfileProvider)?.id.value == owner.profileId &&
      ref.read(vaultStatusProvider).value?.isUnlocked == true &&
      identical(owner.selected, selected) &&
      identical(owner.catalog, catalog) &&
      owner.catalog?.readOnly == false &&
      original?.readOnly != true;
  if (!current() || owner.busy) return null;
  // Opening a connection edit retires the previous review before its first
  // await, even if the user subsequently cancels the form. It never launches.
  owner.cancelRead();
  final result = await showAppDialog<MonitoringSource>(
    context,
    builder: (_) => _AwxOriginalScopeGuard(
      owner: owner,
      isCurrent: current,
      child: Consumer(
        builder: (context, childRef, _) {
          final credentials = childRef.watch(credentialsProvider);
          return AwxConnectionForm(
            original: original,
            title: title,
            disclosure: disclosure,
            httpsNotice: httpsNotice,
            credentials: credentials.value ?? const <Credential>[],
            credentialsLoading: credentials.isLoading,
            credentialsFailed: credentials.hasError,
            isCurrent: current,
            onChanged: owner.cancelRead,
            onCancel: () => Navigator.of(context).pop(),
            onSave: (value) {
              if (current()) Navigator.of(context).pop(value);
            },
            onCreateCredential: (auth) async {
              if (!current()) return null;
              final created = await showAppDialog<Credential>(
                context,
                secure: true,
                builder: (_) => _AwxOriginalScopeGuard(
                  owner: owner,
                  isCurrent: current,
                  child: PasswordCredentialDialog(
                    requireUsername: auth == SourceAuth.basicRef,
                  ),
                ),
              );
              if (!current() || created == null) return null;
              final refreshed = await childRef.refresh(
                credentialsProvider.future,
              );
              if (!current()) return null;
              return refreshed
                  .where(
                    (c) => c.id == created.id && awxCredentialEligible(c, auth),
                  )
                  .firstOrNull;
            },
          );
        },
      ),
    ),
  );
  return current() ? result : null;
}

class _AwxOriginalScopeGuard extends ConsumerWidget {
  const _AwxOriginalScopeGuard({
    required this.owner,
    required this.isCurrent,
    required this.child,
  });
  final AwxController owner;
  final bool Function() isCurrent;
  final Widget child;
  @override
  Widget build(BuildContext context, WidgetRef ref) {
    ref.watch(activeProfileProvider);
    ref.watch(vaultStatusProvider);
    ref.watch(inventoryServiceProvider);
    return AnimatedBuilder(
      animation: owner,
      builder: (context, _) {
        if (isCurrent()) return child;
        final route = ModalRoute.of(context);
        WidgetsBinding.instance.addPostFrameCallback((_) {
          if (context.mounted && route != null && route.isActive) {
            Navigator.of(context).removeRoute(route);
          }
        });
        return const SizedBox.shrink();
      },
    );
  }
}
