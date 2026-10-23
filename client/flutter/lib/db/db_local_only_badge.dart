import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:material_ui/material_ui.dart';

/// "This device only": the connection is not synced, backed up to the vault
/// or shared (engines older versions cannot open).
class DbLocalOnlyBadge extends StatelessWidget {
  const DbLocalOnlyBadge({this.compact = false, super.key});

  /// Only the icon (the label is in the tooltip), for narrow rows.
  final bool compact;

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    return Tooltip(
      message: compact
          ? '${l.dbLocalOnly}. ${l.dbLocalOnlyHelp}'
          : l.dbLocalOnlyHelp, // l10n-ignore: joins localized parts
      child: compact
          ? Semantics(
              label: l.dbLocalOnly,
              child: Icon(Icons.devices_other_rounded, size: 15, color: GlassTokens.of(context).secondaryLabel),
            )
          : GlassBadge(label: l.dbLocalOnly, icon: Icons.devices_other_rounded, dense: true),
    );
  }
}
