import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:material_ui/material_ui.dart';

/// Ordered list of jump hosts (first = closest to the client) with drag
/// reordering, removal and an "add hop" picker. Used by the host editor and
/// the jump-profile editor.
class JumpChainEditor extends StatelessWidget {
  const JumpChainEditor({
    required this.chain,
    required this.hosts,
    required this.onChanged,
    super.key,
    this.targetLabel,
  });

  final List<ObjectId> chain;

  /// Candidate hops (the edited host itself must already be excluded).
  final List<Host> hosts;
  final ValueChanged<List<ObjectId>> onChanged;

  /// Shown at the end of the route preview ("→ prod-db-1").
  final String? targetLabel;

  @override
  Widget build(BuildContext context) {
    final tokens = GlassTokens.of(context);
    final t = tokens.typography;
    final l10n = context.l10n;
    final byId = {for (final h in hosts) h.id: h};
    final available = hosts.where((h) => !chain.contains(h.id)).toList()..sort((a, b) => a.name.compareTo(b.name));
    final names = [for (final id in chain) byId[id]?.name ?? l10n.jumpChainDeletedHost];
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Text(
          [l10n.jumpChainThisDevice, ...names, targetLabel ?? l10n.jumpChainTarget].join('  →  '),
          key: const ValueKey('route-preview'),
          style: t.mono.copyWith(fontSize: 12, color: tokens.secondaryLabel),
        ),
        const SizedBox(height: GlassSpacing.s8),
        if (chain.isEmpty)
          Padding(
            padding: const EdgeInsets.symmetric(vertical: GlassSpacing.s8),
            child: Text(l10n.jumpChainEmpty, style: t.body.copyWith(color: tokens.secondaryLabel)),
          )
        else
          ReorderableListView(
            shrinkWrap: true,
            buildDefaultDragHandles: false,
            physics: const NeverScrollableScrollPhysics(),
            onReorderItem: (oldIndex, newIndex) {
              final next = [...chain];
              final item = next.removeAt(oldIndex);
              next.insert(newIndex, item);
              onChanged(next);
            },
            children: [
              for (final (i, id) in chain.indexed)
                ListTile(
                  key: ValueKey('hop-${id.value}'),
                  dense: true,
                  leading: SizedBox.square(
                    dimension: 24,
                    child: DecoratedBox(
                      decoration: ShapeDecoration(color: tokens.sidebarSelection, shape: const CircleBorder()),
                      child: Center(
                        child: Text('${i + 1}', style: t.caption.copyWith(color: tokens.palette.accent)),
                      ),
                    ),
                  ),
                  title: Text(names[i], style: t.bodyEmph.copyWith(color: tokens.palette.label)),
                  subtitle: Text(
                    byId[id]?.address ?? '',
                    style: t.mono.copyWith(fontSize: 12, color: tokens.secondaryLabel),
                  ),
                  trailing: Row(
                    mainAxisSize: MainAxisSize.min,
                    children: [
                      IconButton(
                        tooltip: l10n.jumpChainMoveUp,
                        icon: const Icon(Icons.arrow_upward_rounded, size: 18),
                        onPressed: i == 0
                            ? null
                            : () => onChanged(
                                [...chain]
                                  ..removeAt(i)
                                  ..insert(i - 1, id),
                              ),
                      ),
                      IconButton(
                        tooltip: l10n.jumpChainRemoveHop,
                        icon: const Icon(Icons.close_rounded, size: 18),
                        onPressed: () => onChanged([...chain]..removeAt(i)),
                      ),
                      ReorderableDragStartListener(
                        index: i,
                        child: Icon(Icons.drag_indicator_rounded, color: tokens.secondaryLabel),
                      ),
                    ],
                  ),
                ),
            ],
          ),
        const SizedBox(height: GlassSpacing.s8),
        Align(
          alignment: AlignmentDirectional.centerStart,
          child: GlassMenuButton<ObjectId>(
            entries: [
              for (final h in available)
                GlassMenuItem(
                  key: ValueKey('add-hop-${h.name}'),
                  value: h.id,
                  label: h.name,
                  subtitle: h.address,
                  icon: Icons.dns_rounded,
                ),
            ],
            onSelected: (id) => onChanged([...chain, id]),
            builder: (context, open) => GlassButton(
              key: const ValueKey('add-hop'),
              tooltip: l10n.jumpChainAddTooltip,
              onPressed: available.isEmpty ? null : open,
              icon: Icons.add_rounded,
              label: l10n.jumpChainAddHop,
            ),
          ),
        ),
      ],
    );
  }
}
