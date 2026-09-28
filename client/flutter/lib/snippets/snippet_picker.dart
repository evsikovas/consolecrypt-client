import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/core/widgets/risk_badge.dart';
import 'package:consolecrypt/snippets/run_flow.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

/// Quick snippet chooser from the terminal toolbar; runs through the
/// standard variables → confirmation flow.
Future<void> showSnippetPickerAndRun(BuildContext context, WidgetRef ref) async {
  final snippet = await showAppDialog<Snippet>(context, builder: (_) => const _SnippetPicker());
  if (snippet != null && context.mounted) await runSnippetFlow(context, ref, snippet);
}

class _SnippetPicker extends ConsumerStatefulWidget {
  const _SnippetPicker();

  @override
  ConsumerState<_SnippetPicker> createState() => _SnippetPickerState();
}

class _SnippetPickerState extends ConsumerState<_SnippetPicker> {
  List<SnippetSearchHit> _hits = const [];

  @override
  void initState() {
    super.initState();
    _search('');
  }

  Future<void> _search(String q) async {
    final hits = await ref.read(snippetServiceProvider).search(q);
    if (mounted) setState(() => _hits = hits);
  }

  @override
  Widget build(BuildContext context) {
    final l10n = context.l10n;
    final tokens = GlassTokens.of(context);
    return GlassDialog(
      key: const ValueKey('snippet-picker'),
      title: l10n.snippetPickerTitle,
      width: 560,
      content: SizedBox(
        height: 420,
        child: Column(
          children: [
            GlassField(
              key: const ValueKey('snippet-picker-search'),
              search: true,
              size: GlassFieldSize.lg,
              autofocus: true,
              leadingIcon: Icons.search_rounded,
              placeholder: l10n.snippetPickerSearchHint,
              onChanged: _search,
            ),
            const SizedBox(height: GlassSpacing.s8),
            Expanded(
              child: ListView(
                children: [
                  for (final h in _hits)
                    ListTile(
                      key: ValueKey('snippet-pick-${h.snippet.name}'),
                      title: Text(
                        h.snippet.name,
                        style: tokens.typography.bodyEmph.copyWith(color: tokens.palette.label),
                      ),
                      subtitle: Text(
                        h.snippet.template,
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis,
                        style: tokens.typography.mono.copyWith(fontSize: 12, color: tokens.secondaryLabel),
                      ),
                      trailing: RiskBadge(risk: h.snippet.riskLevel, dense: true),
                      onTap: () => closeDialog(context, h.snippet),
                    ),
                ],
              ),
            ),
          ],
        ),
      ),
      secondaryActions: [GlassButton(label: l10n.commonCancel, onPressed: () => closeDialog<Snippet>(context))],
    );
  }
}
