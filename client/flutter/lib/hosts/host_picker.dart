import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/hosts/hosts_screen.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

/// Searchable host chooser (new terminal tab, SFTP, snippet target) on a
/// glass dialog. [title] defaults to "Connect to host".
Future<Host?> showHostPicker(BuildContext context, {String? title, ObjectId? exclude}) => showAppDialog<Host>(
  context,
  builder: (_) => _HostPickerDialog(title: title, exclude: exclude),
);

class _HostPickerDialog extends ConsumerStatefulWidget {
  const _HostPickerDialog({this.title, this.exclude});

  final String? title;
  final ObjectId? exclude;

  @override
  ConsumerState<_HostPickerDialog> createState() => _HostPickerDialogState();
}

class _HostPickerDialogState extends ConsumerState<_HostPickerDialog> {
  String _query = '';

  @override
  Widget build(BuildContext context) {
    final hosts = (ref.watch(hostsProvider).value ?? const <Host>[]).where((h) => h.id != widget.exclude).where((h) {
      final q = _query.toLowerCase();
      return q.isEmpty ||
          h.name.toLowerCase().contains(q) ||
          h.address.toLowerCase().contains(q) ||
          h.tags.any((t) => t.contains(q));
    }).toList()..sort((a, b) => a.name.compareTo(b.name));
    final tokens = GlassTokens.of(context);
    final t = tokens.typography;
    final l10n = context.l10n;
    return GlassDialog(
      key: const ValueKey('host-picker'),
      title: widget.title ?? l10n.hostPickerDefaultTitle,
      width: 520,
      content: SizedBox(
        height: 420,
        child: Column(
          children: [
            GlassField(
              key: const ValueKey('host-picker-search'),
              search: true,
              size: GlassFieldSize.lg,
              autofocus: true,
              leadingIcon: Icons.search_rounded,
              placeholder: l10n.hostPickerSearchHint,
              onChanged: (v) => setState(() => _query = v),
              onSubmitted: (_) {
                if (hosts.isNotEmpty) closeDialog(context, hosts.first);
              },
            ),
            const SizedBox(height: GlassSpacing.s8),
            Expanded(
              child: hosts.isEmpty
                  ? EmptyState(icon: Icons.dns_rounded, title: l10n.hostPickerEmpty, compact: true)
                  : ListView.builder(
                      itemCount: hosts.length,
                      itemBuilder: (context, i) {
                        final h = hosts[i];
                        return ListTile(
                          key: ValueKey('pick-${h.name}'),
                          leading: HostAvatar(host: h, size: 28),
                          title: Text(h.name, style: t.bodyEmph.copyWith(color: tokens.palette.label)),
                          subtitle: Text(h.address, style: t.mono.copyWith(fontSize: 12, color: tokens.secondaryLabel)),
                          trailing: h.tags.isEmpty ? null : SizedBox(width: 140, child: TagChips(tags: h.tags)),
                          onTap: () => closeDialog(context, h),
                        );
                      },
                    ),
            ),
          ],
        ),
      ),
      secondaryActions: [GlassButton(label: l10n.commonCancel, onPressed: () => closeDialog<Host>(context))],
    );
  }
}
