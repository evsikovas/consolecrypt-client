import 'package:consolecrypt/app/brand.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/l10n/labels.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/security/passphrase_strength.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:macos_window_utils/widgets/macos_toolbar_passthrough.dart';
import 'package:material_ui/material_ui.dart';

/// Six groups of five digits for side-by-side comparison (ADR-0004), as the
/// kit's [GlassVerificationCode] (§4.13): a fixed 3 × 2 grid on
/// `surface.inset`, tabular mono digits, no animation or blur, selectable
/// and read group by group.
class VerificationCodeView extends StatelessWidget {
  const VerificationCodeView({required this.code, super.key});

  final VerificationCode code;

  @override
  Widget build(BuildContext context) => Semantics(
    label: context.l10n.verificationCodeSemantics(code.groups.join(', ')),
    child: GlassVerificationCode(groups: code.groups),
  );
}

/// Live strength meter for a new Vault passphrase, in the role colours
/// (danger / warning / success; §4.15).
class StrengthMeter extends StatelessWidget {
  const StrengthMeter({required this.strength, super.key});

  final PassphraseStrength strength;

  @override
  Widget build(BuildContext context) {
    final tokens = GlassTokens.of(context);
    final p = tokens.palette;
    final color = switch (strength.score) {
      0 || 1 => p.danger,
      2 => p.warningFill,
      _ => p.success,
    };
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        ClipRSuperellipse(
          borderRadius: BorderRadius.circular(3),
          child: LinearProgressIndicator(
            value: strength.fraction,
            minHeight: 6,
            color: color,
            backgroundColor: tokens.surfaces.fillPressed,
          ),
        ),
        const SizedBox(height: GlassSpacing.s4),
        Text(
          context.l10n.strengthLabelWithHint(
            strength.localizedLabel(context.l10n),
            strength.localizedHint(context.l10n),
          ),
          key: const ValueKey('strength-label'),
          style: tokens.typography.callout.copyWith(color: tokens.secondaryLabel),
        ),
      ],
    );
  }
}

/// Demo credentials hint shown on gate screens when running on mocks.
class DemoHints extends ConsumerWidget {
  const DemoHints({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final dev = ref.watch(developerControlsProvider);
    if (dev == null) return const SizedBox.shrink();
    final tokens = GlassTokens.of(context);
    return Padding(
      padding: const EdgeInsets.only(top: GlassSpacing.s16),
      child: ContentSurface(
        kind: ContentSurfaceKind.inset,
        padding: const EdgeInsets.all(GlassSpacing.s12),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            for (final hint in dev.demoHints)
              SelectableText(hint, style: tokens.typography.callout.copyWith(color: tokens.secondaryLabel)),
          ],
        ),
      ),
    );
  }
}

/// Material of a gate card (LIQUID_GLASS_SPEC §3, §4.14–§4.15).
enum GateMaterial {
  /// `glass.regular` hero over the ambient backdrop (welcome, notices) —
  /// the one place for expressive glass. No secrets on it.
  glass,

  /// `glass.secure` (opaque, no blur, nothing animated): passphrase and
  /// password prompts, verification codes, recovery.
  secure,

  /// A content page (Recovery Kit): `surface.content`, secrets on paper.
  content,
}

/// Centered card used by the gate screens (welcome, login, unlock, …) over
/// the expressive ambient backdrop. The top bar (back button, language
/// switcher) sits in the 52-pt title-bar band, clear of the macOS traffic
/// lights, and never overlaps the card.
class GateScaffold extends StatelessWidget {
  const GateScaffold({
    required this.child,
    super.key,
    this.maxWidth = 520,
    this.leading,
    this.trailing,
    this.material = GateMaterial.secure,
  });

  final Widget child;
  final double maxWidth;
  final Widget? leading;

  /// Top-right slot (e.g. the language switcher on the welcome screen).
  final Widget? trailing;
  final GateMaterial material;

  @override
  Widget build(BuildContext context) {
    final tokens = GlassTokens.of(context);
    final scope = GlassScope.of(context);
    // With the unified title bar the band belongs to the native toolbar:
    // leave room for the traffic lights and route clicks to Flutter.
    final trafficLights = tokens.platform == TargetPlatform.macOS && scope.unifiedTitlebar;
    Widget pass(Widget w) => scope.unifiedTitlebar ? MacosToolbarPassthrough(child: w) : w;
    final radius = tokens.radii.dialog;
    const cardKey = ValueKey('gate-card');
    final Widget card = switch (material) {
      GateMaterial.glass => GlassSurface(
        key: cardKey,
        variant: GlassVariant.regular,
        shape: GlassRadii.shape(radius),
        backdrop: BackdropMode.live,
        padding: const EdgeInsets.all(GlassSpacing.s32),
        child: child,
      ),
      GateMaterial.secure => SecureSurface(
        key: cardKey,
        radius: radius,
        padding: const EdgeInsets.all(GlassSpacing.secureDialog),
        child: child,
      ),
      GateMaterial.content => ContentSurface(
        key: cardKey,
        radius: radius,
        padding: const EdgeInsets.all(GlassSpacing.secureDialog),
        child: child,
      ),
    };
    return Scaffold(
      backgroundColor: const Color(0x00000000),
      body: Stack(
        children: [
          // Onboarding is the one place for expressive glass: the backdrop is
          // 1.4× more saturated here (§4.15). Painted once.
          const Positioned.fill(child: AmbientBackdrop(expressive: true)),
          SafeArea(
            // Top bar and card are laid out in a column (not a Stack overlay), so
            // the back button / language switcher can never overlap the card,
            // whatever the window width or label length.
            child: Column(
              children: [
                SizedBox(
                  height: GlassSizes.toolbarBand,
                  child: Padding(
                    padding: EdgeInsetsDirectional.only(
                      start: trafficLights ? GlassSizes.trafficLightsLeading : GlassSpacing.s12,
                      end: GlassSpacing.s12,
                    ),
                    child: Row(
                      children: [
                        if (leading != null) pass(leading!),
                        const Spacer(),
                        if (trailing != null) pass(trailing!),
                      ],
                    ),
                  ),
                ),
                Expanded(
                  child: Center(
                    child: SingleChildScrollView(
                      padding: const EdgeInsets.fromLTRB(
                        GlassSpacing.s24,
                        GlassSpacing.s8,
                        GlassSpacing.s24,
                        GlassSpacing.s32,
                      ),
                      child: ConstrainedBox(
                        constraints: BoxConstraints(maxWidth: maxWidth),
                        child: Material(type: MaterialType.transparency, child: card),
                      ),
                    ),
                  ),
                ),
              ],
            ),
          ),
        ],
      ),
    );
  }
}

/// Mark + title used on gate screens: the brand wordmark ([BrandMark]) unless
/// a step [icon] is given, a large left-aligned title and a secondary
/// subtitle.
class GateHeader extends StatelessWidget {
  const GateHeader({required this.title, super.key, this.subtitle, this.icon});

  final String title;
  final String? subtitle;

  /// Step glyph (lock, key, …); `null` shows the brand wordmark.
  final IconData? icon;

  @override
  Widget build(BuildContext context) {
    final tokens = GlassTokens.of(context);
    final t = tokens.typography;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        if (icon == null) const BrandMark.wordmark(height: 44) else Icon(icon, size: 34, color: tokens.palette.accent),
        const SizedBox(height: GlassSpacing.s12),
        Semantics(
          header: true,
          child: Text(title, style: t.largeTitle.copyWith(color: tokens.palette.label)),
        ),
        if (subtitle != null) ...[
          const SizedBox(height: GlassSpacing.s6),
          Text(subtitle!, style: t.body.copyWith(color: tokens.secondaryLabel)),
        ],
        const SizedBox(height: GlassSpacing.s20),
      ],
    );
  }
}

/// Error line on gate screens and secure prompts: a danger icon plus the
/// text in the danger colour (colour is never the only cue).
class GateErrorText extends StatelessWidget {
  const GateErrorText({required this.text, super.key});

  final String text;

  @override
  Widget build(BuildContext context) {
    final tokens = GlassTokens.of(context);
    return Row(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Padding(
          padding: const EdgeInsets.only(top: 1),
          child: Icon(Icons.error_rounded, size: 16, color: tokens.palette.danger),
        ),
        const SizedBox(width: GlassSpacing.s6),
        Expanded(
          child: Text(text, style: tokens.typography.body.copyWith(color: tokens.palette.danger)),
        ),
      ],
    );
  }
}
