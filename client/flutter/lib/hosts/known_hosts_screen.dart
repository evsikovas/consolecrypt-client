import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/l10n/labels.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/util/formatting.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

class KnownHostsScreen extends ConsumerWidget {
  const KnownHostsScreen({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final l10n = context.l10n;
    return PageScaffold(
      key: const ValueKey('known-hosts-screen'),
      title: l10n.settingsKnownHostsTitle,
      subtitle: l10n.settingsKnownHostsSubtitle,
      body: AsyncValueView(
        value: ref.watch(knownHostsProvider),
        data: (known) => known.isEmpty
            ? EmptyState(icon: Icons.verified_user_outlined, title: l10n.settingsKnownHostsEmpty)
            : ContentList(
                itemCount: known.length,
                itemBuilder: (context, i) => _KnownHostTile(key: ValueKey(known[i].id), host: known[i]),
              ),
      ),
    );
  }
}

class _KnownHostTile extends ConsumerWidget {
  const _KnownHostTile({required this.host, super.key});

  final KnownHost host;

  Future<void> _remove(BuildContext context, WidgetRef ref) async {
    final l10n = context.l10n;
    final profileId = ref.read(activeProfileProvider)?.id;
    final ok = await showConfirmDialog(
      context,
      title: l10n.settingsRemoveKnownHostTitle(host.hostPattern),
      message: l10n.settingsRemoveKnownHostMessage,
      confirmLabel: l10n.commonRemove,
    );
    if (ok && context.mounted && ref.read(activeProfileProvider)?.id == profileId) {
      await runWithFeedback(context, () => ref.read(inventoryServiceProvider).deleteKnownHost(host.id));
    }
  }

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final l10n = context.l10n;
    final tokens = GlassTokens.of(context);
    return ListTile(
      key: ValueKey('known-host-${host.id.value}'),
      contentPadding: const EdgeInsets.symmetric(horizontal: GlassSpacing.s16, vertical: GlassSpacing.s8),
      title: Text(host.hostPattern, style: tokens.typography.bodyEmph),
      subtitle: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          SelectableText('${host.keyType} ${host.fingerprintSha256}', style: tokens.typography.mono),
          const SizedBox(height: GlassSpacing.s4),
          Text(
            '${host.source.localized(l10n)} · ${formatDate(l10n, host.addedAt)}',
            style: tokens.typography.callout.copyWith(color: tokens.secondaryLabel),
          ),
        ],
      ),
      trailing: GlassIconButton(
        key: ValueKey('remove-known-host-${host.id.value}'),
        tooltip: l10n.commonRemove,
        icon: Icons.delete_rounded,
        style: GlassIconButtonStyle.plain,
        onPressed: () => _remove(context, ref),
      ),
    );
  }
}
