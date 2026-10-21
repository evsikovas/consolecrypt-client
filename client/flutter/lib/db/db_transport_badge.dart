import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/db/db_errors.dart';
import 'package:consolecrypt/db/db_models.dart';
import 'package:material_ui/material_ui.dart';

/// What protects a database session: a badge in the workspace header, or a
/// small icon with a tooltip on a connection card.
class DbTransportBadge extends StatelessWidget {
  const DbTransportBadge(this.security, {this.compact = false, super.key});
  final DbTransportSecurity security;
  final bool compact;

  static (IconData, GlassTone) _look(DbTransportSecurity security) => switch (security) {
    DbTransportSecurity.verified => (Icons.lock_rounded, GlassTone.success),
    DbTransportSecurity.unverified => (Icons.lock_outline_rounded, GlassTone.warning),
    DbTransportSecurity.tunnel => (Icons.vpn_lock_outlined, GlassTone.neutral),
    DbTransportSecurity.plaintext => (Icons.lock_open_rounded, GlassTone.danger),
  };

  @override
  Widget build(BuildContext context) {
    final label = dbTransportText(security, context.l10n);
    final (icon, tone) = _look(security);
    if (!compact) return GlassBadge(label: label, tone: tone, icon: icon, dense: true);
    final tokens = GlassTokens.of(context);
    return Tooltip(
      message: label,
      child: Semantics(
        label: label,
        child: Icon(
          icon,
          size: 15,
          color: tone == GlassTone.neutral || tone == GlassTone.success
              ? tokens.secondaryLabel
              : tokens.palette.tone(tone),
        ),
      ),
    );
  }
}
