import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/util/formatting.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/sharing/sharing_dialogs.dart';
import 'package:consolecrypt/sharing/sharing_models.dart';
import 'package:consolecrypt/sharing/sharing_providers.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

/// Owner-deleted shared items the server keeps restorable (protocol 1.8).
/// A restore republishes the item; recipients accept it again.
Future<void> showSharingRecentlyDeleted(BuildContext context) =>
    showAppDialog<void>(context, secure: true, builder: (_) => const SharingRecentlyDeletedDialog());

class SharingRecentlyDeletedDialog extends ConsumerStatefulWidget {
  const SharingRecentlyDeletedDialog({super.key});
  @override
  ConsumerState<SharingRecentlyDeletedDialog> createState() => _SharingRecentlyDeletedDialogState();
}

class _SharingRecentlyDeletedDialogState extends ConsumerState<SharingRecentlyDeletedDialog> {
  late final Object? _profile;
  late Future<List<SharingDeletedItem>> _items;
  String? _busy;

  @override
  void initState() {
    super.initState();
    _profile = ref.read(sharingSessionScopeProvider);
    _items = ref.read(sharingServiceProvider).deleted();
  }

  Future<void> _restore(SharingDeletedItem item) async {
    if (_busy != null || !sharingSessionCurrent(ref, _profile)) return;
    setState(() => _busy = item.id);
    final service = ref.read(sharingServiceProvider);
    await runWithFeedback(context, () async {
      await service.restore(item.id);
      await service.flush();
      return true;
    }, success: context.l10n.sharingRestored);
    if (!mounted || !sharingSessionCurrent(ref, _profile)) return;
    refreshSharing(ref);
    setState(() {
      _busy = null;
      _items = service.deleted();
    });
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    final tokens = GlassTokens.of(context);
    return SharingDialogGuard(
      profile: _profile,
      child: GlassDialog(
        title: l.sharingRecentlyDeleted,
        content: SizedBox(
          width: 560,
          child: ConstrainedBox(
            constraints: BoxConstraints(maxHeight: MediaQuery.sizeOf(context).height * .65),
            child: FutureBuilder<List<SharingDeletedItem>>(
              future: _items,
              builder: (context, snapshot) {
                if (snapshot.connectionState != ConnectionState.done) return const LinearProgressIndicator();
                if (snapshot.hasError) return Text(l.sharingUnavailable);
                final all = snapshot.data ?? const <SharingDeletedItem>[];
                return SingleChildScrollView(
                  child: Column(
                    mainAxisSize: MainAxisSize.min,
                    crossAxisAlignment: CrossAxisAlignment.stretch,
                    children: [
                      Text(
                        l.sharingRecentlyDeletedHelp,
                        style: tokens.typography.callout.copyWith(color: tokens.secondaryLabel),
                      ),
                      const SizedBox(height: 8),
                      if (all.isEmpty) Text(l.sharingRecentlyDeletedEmpty),
                      for (final item in all)
                        ListTile(
                          key: ValueKey('sharing-deleted-${item.id}'),
                          dense: true,
                          contentPadding: EdgeInsets.zero,
                          title: Text(item.name ?? l.sharingInvitation),
                          subtitle: Text(l.sharingRestoreUntil(formatDate(l, item.restorableUntil), item.recipients)),
                          trailing: GlassButton(
                            label: l.sharingRestore,
                            icon: Icons.restore_rounded,
                            onPressed: _busy == null ? () => _restore(item) : null,
                          ),
                        ),
                    ],
                  ),
                );
              },
            ),
          ),
        ),
        primaryAction: GlassButton(label: l.commonClose, onPressed: () => closeDialog<void>(context)),
      ),
    );
  }
}
