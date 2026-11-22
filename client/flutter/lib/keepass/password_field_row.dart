import 'package:consolecrypt/core/glass/glass.dart';
import 'package:material_ui/material_ui.dart';

/// Shared alignment for masked fields and their actions in KDBX and Vault.
class PasswordFieldRow extends StatelessWidget {
  const PasswordFieldRow({required this.label, required this.value, required this.actions, super.key});

  final String label;
  final Widget value;
  final List<Widget> actions;

  @override
  Widget build(BuildContext context) {
    final tokens = GlassTokens.of(context);
    return Padding(
      padding: const EdgeInsets.symmetric(horizontal: GlassSpacing.s12, vertical: GlassSpacing.s8),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        spacing: GlassSpacing.s8,
        children: [
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              spacing: GlassSpacing.s4,
              children: [
                Text(label, style: tokens.typography.callout.copyWith(color: tokens.secondaryLabel)),
                value,
              ],
            ),
          ),
          Row(mainAxisSize: MainAxisSize.min, spacing: GlassSpacing.toolbarItemGap, children: actions),
        ],
      ),
    );
  }
}
