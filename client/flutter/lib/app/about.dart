import 'package:consolecrypt/app/app_info.dart';
import 'package:consolecrypt/app/brand.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

/// "About ConsoleCrypt" (Settings → About and the macOS app menu): brand,
/// version, author and licences (ADR-0005). A synced profile also shows
/// where its sync server offers its source code (AGPL, `GET /v1/meta`).
Future<void> showAboutConsoleCrypt(BuildContext context) =>
    showAppDialog<void>(context, builder: (context) => const AboutConsoleCryptDialog());

class AboutConsoleCryptDialog extends StatelessWidget {
  const AboutConsoleCryptDialog({super.key});

  @override
  Widget build(BuildContext context) {
    final l10n = context.l10n;
    return GlassDialog(
      key: const ValueKey('about-dialog'),
      title: l10n.aboutTitle(kAppName),
      width: 480,
      content: const AboutConsoleCryptContent(),
      primaryAction: GlassButton.prominent(
        key: const ValueKey('about-close'),
        label: l10n.commonClose,
        autofocus: true,
        onPressed: () => closeDialog<void>(context),
      ),
      onSubmit: () => closeDialog<void>(context),
    );
  }
}

/// Body of the About dialog (also embedded in Settings → About).
/// The source-code link of the active profile's sync server; null for local
/// profiles, unreachable servers or servers that do not offer one.
final syncServerSourceProvider = FutureProvider.autoDispose<String?>((ref) async {
  final server = ref.watch(activeProfileProvider.select((p) => p?.serverUrl));
  if (server == null) return null;
  try {
    return (await ref.read(authServiceProvider).probeServer(server)).sourceCodeUrl;
  } on Object {
    return null;
  }
});

class AboutConsoleCryptContent extends ConsumerWidget {
  const AboutConsoleCryptContent({super.key, this.compact = false});

  /// Settings card: a smaller logo next to the facts.
  final bool compact;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final tokens = GlassTokens.of(context);
    final t = tokens.typography;
    final l10n = context.l10n;
    final serverSource = ref.watch(syncServerSourceProvider).value;
    Widget row(String label, Widget value) => Padding(
      padding: const EdgeInsets.symmetric(vertical: GlassSpacing.s4),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          SizedBox(
            width: 150,
            child: Text(label, style: t.body.copyWith(color: tokens.secondaryLabel)),
          ),
          Expanded(
            child: DefaultTextStyle.merge(
              style: t.body.copyWith(color: tokens.palette.label),
              child: value,
            ),
          ),
        ],
      ),
    );
    final facts = Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      mainAxisSize: MainAxisSize.min,
      children: [
        if (!compact) ...[const BrandMark.wordmark(height: 40), const SizedBox(height: GlassSpacing.s8)],
        Text(l10n.aboutTagline, style: t.bodyEmph.copyWith(color: tokens.palette.label)),
        const SizedBox(height: GlassSpacing.s2),
        Text(
          l10n.aboutVersion(kAppFullVersion),
          key: const ValueKey('about-version'),
          style: t.callout.copyWith(color: tokens.secondaryLabel),
        ),
        const SizedBox(height: GlassSpacing.s16),
        row(
          l10n.aboutAuthor,
          SelectableText.rich(
            TextSpan(
              children: [
                const TextSpan(text: kAppAuthor),
                TextSpan(
                  text: ' · $kAppAuthorEmail',
                  style: t.body.copyWith(color: tokens.secondaryLabel),
                ),
              ],
            ),
            key: const ValueKey('about-author'),
          ),
        ),
        const SizedBox(height: GlassSpacing.s8),
        Text(l10n.aboutLicenses, style: t.bodyEmph.copyWith(color: tokens.palette.label)),
        row(l10n.aboutLicenseClient, Text(kAppLicenseDisplayName, style: t.mono.copyWith(color: tokens.palette.label))),
        row(
          l10n.aboutLicenseServer,
          Text(kServerLicenseDisplayName, style: t.mono.copyWith(color: tokens.palette.label)),
        ),
        const SizedBox(height: GlassSpacing.s12),
        Text(l10n.aboutOpenSource, style: t.callout.copyWith(color: tokens.secondaryLabel)),
        const SizedBox(height: GlassSpacing.s8),
        SelectableText(kAppWebsite, key: const ValueKey('about-website'), style: t.callout),
        SelectableText(kAppSourceUrl, key: const ValueKey('about-source'), style: t.callout),
        if (serverSource != null) ...[
          const SizedBox(height: GlassSpacing.s8),
          Text(l10n.aboutServerSource, style: t.callout.copyWith(color: tokens.secondaryLabel)),
          SelectableText(serverSource, key: const ValueKey('about-server-source'), style: t.callout),
        ],
        const SizedBox(height: GlassSpacing.s4),
        Text(kAppCopyright, style: t.caption.copyWith(color: tokens.palette.tertiary)),
      ],
    );
    return Row(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        BrandMark.shield(height: compact ? 88 : 120),
        const SizedBox(width: GlassSpacing.s20),
        Expanded(child: facts),
      ],
    );
  }
}
