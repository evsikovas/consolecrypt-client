import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/error_messages.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/snippets/starter_catalog.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

Future<void> showStarterCatalog(BuildContext context) =>
    showAppDialog<void>(context, builder: (_) => const _StarterCatalogDialog());

class _StarterCatalogDialog extends ConsumerStatefulWidget {
  const _StarterCatalogDialog();
  @override
  ConsumerState<_StarterCatalogDialog> createState() => _StarterCatalogDialogState();
}

class _StarterCatalogDialogState extends ConsumerState<_StarterCatalogDialog> {
  String? _busy;
  String? _error;

  Future<void> _add(StarterSnippetPackage package) async {
    final profile = ref.read(activeProfileProvider)?.id;
    final service = ref.read(snippetServiceProvider);
    final missing = package.missingFrom(ref.read(snippetsProvider).value ?? const []);
    setState(() {
      _busy = package.id;
      _error = null;
    });
    try {
      for (final snippet in missing) {
        if (!mounted || ref.read(activeProfileProvider)?.id != profile) return;
        await service.saveSnippet(snippet);
      }
    } on AppException catch (e) {
      if (mounted) setState(() => _error = errorMessage(context.l10n, e));
    } finally {
      if (mounted) setState(() => _busy = null);
    }
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    final snippets = ref.watch(snippetsProvider).value ?? const <Snippet>[];
    final t = GlassTokens.of(context);
    return GlassDialog(
      key: const ValueKey('snippet-catalog'),
      title: l.snippetStarterCatalog,
      width: 700,
      content: SingleChildScrollView(
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Text(l.snippetStarterIntro),
            const SizedBox(height: 16),
            for (final package in starterSnippetPackages(l))
              Padding(
                padding: const EdgeInsets.only(bottom: 12),
                child: ContentSurface(
                  padding: const EdgeInsets.all(14),
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.stretch,
                    children: [
                      Text(package.name, style: t.typography.title3),
                      const SizedBox(height: 6),
                      Text(package.description, style: t.typography.callout),
                      const SizedBox(height: 8),
                      Text(package.snippets.map((s) => s.name).join(' · '), style: t.typography.caption),
                      const SizedBox(height: 12),
                      Align(
                        alignment: Alignment.centerRight,
                        child: GlassButton(
                          key: ValueKey('snippet-catalog-add-${package.id}'),
                          label: package.missingFrom(snippets).isEmpty
                              ? l.snippetPackageAdded
                              : l.snippetAddCount(package.missingFrom(snippets).length),
                          icon: package.missingFrom(snippets).isEmpty ? Icons.check_rounded : Icons.add_rounded,
                          busy: _busy == package.id,
                          onPressed: _busy != null || package.missingFrom(snippets).isEmpty
                              ? null
                              : () => _add(package),
                        ),
                      ),
                    ],
                  ),
                ),
              ),
            if (_error != null) Text(_error!, style: TextStyle(color: t.palette.danger)),
          ],
        ),
      ),
      secondaryActions: [
        GlassButton(label: l.commonClose, onPressed: _busy == null ? () => closeDialog<void>(context) : null),
      ],
    );
  }
}
