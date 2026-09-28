import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/util/formatting.dart';
import 'package:consolecrypt/sftp/sftp_actions.dart';
import 'package:consolecrypt/sftp/sftp_controller.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

/// One tap opens a folder or an in-memory preview. Every row has a visible
/// action menu; no double click, right click or desktop-width columns.
class SftpMobileList extends ConsumerWidget {
  const SftpMobileList({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final state = ref.watch(sftpControllerProvider);
    final rows = ref.watch(sftpRowsProvider);
    final controller = ref.read(sftpControllerProvider.notifier);
    final actions = SftpActions(context, ref);
    if (state.isLoadingCurrent) return const Center(child: CircularProgressIndicator());
    if (rows.isEmpty) return Center(child: Text(context.l10n.sftpEmptyFolder));
    return RefreshIndicator(
      onRefresh: controller.refresh,
      child: ListView.builder(
        key: const ValueKey('sftp-mobile-files'),
        physics: const AlwaysScrollableScrollPhysics(),
        itemCount: rows.length,
        itemBuilder: (context, index) {
          final e = rows[index].entry;
          return ListTile(
            key: ValueKey('sftp-row-${e.path}'),
            minVerticalPadding: 12,
            leading: Icon(
              e.isDirectory ? Icons.folder_rounded : Icons.description_outlined,
              color: e.isDirectory ? Theme.of(context).colorScheme.primary : null,
            ),
            title: Text(e.name, maxLines: 1, overflow: TextOverflow.ellipsis),
            subtitle: e.isDirectory ? null : Text(formatBytes(context.l10n, e.size)),
            selected: state.selection.contains(e.path),
            onLongPress: () => controller.selectOnly(e.path),
            onTap: () {
              controller.selectOnly(e.path);
              actions.runChoice(SftpAction.open, [e]);
            },
            trailing: GlassMenuButton<SftpAction>(
              entries: actions.glassMenuEntries([e]),
              onSelected: (action) => actions.runChoice(action, [e]),
              builder: (context, open) => GlassIconButton(
                icon: Icons.more_horiz_rounded,
                tooltip: context.l10n.sftpToolbarActions,
                style: GlassIconButtonStyle.plain,
                onPressed: open,
              ),
            ),
          );
        },
      ),
    );
  }
}
