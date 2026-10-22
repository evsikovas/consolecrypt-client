import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/error_messages.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/core/widgets/risk_badge.dart';
import 'package:consolecrypt/db/db_models.dart';
import 'package:consolecrypt/snippets/run_flow.dart';
import 'package:consolecrypt/snippets/starter_catalog_db.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

const _engineDialects = {'mysql', 'mariadb', 'clickhouse'};

/// Whether [snippet] is written for [engine]. Mirrors the type / dialect the
/// Databases history saves (`db_save_history_snippet`): PostgreSQL →
/// postgresql; MySQL / MariaDB / ClickHouse → sql + "mysql" / "mariadb" /
/// "clickhouse" (MySQL and MariaDB share snippets); Redis → redis_cli;
/// MongoDB → shell + "mongosh". Plain SQL snippets without an engine dialect
/// match every SQL engine. Snippets tagged [terminalOnlySnippetTag] are
/// server admin commands the workspace guard refuses, so they are left out.
bool snippetMatchesEngine(Snippet snippet, DbEngine engine) {
  if (snippet.tags.contains(terminalOnlySnippetTag)) return false;
  final dialect = snippet.shell?.trim().toLowerCase() ?? '';
  final sql = snippet.snippetType == SnippetType.sql;
  final genericSql = sql && !_engineDialects.contains(dialect);
  return switch (engine) {
    DbEngine.postgresql => snippet.snippetType == SnippetType.postgresql || genericSql,
    DbEngine.mysql || DbEngine.mariadb => sql && (dialect == 'mysql' || dialect == 'mariadb') || genericSql,
    DbEngine.clickhouse => sql && dialect == 'clickhouse' || genericSql,
    DbEngine.redis => snippet.snippetType == SnippetType.redisCli,
    DbEngine.mongodb => snippet.snippetType == SnippetType.shell && dialect == 'mongosh',
  };
}

/// Lets the user choose one of their snippets for [engine], fills its
/// `{{variables}}` with the standard variables dialog and renders it with the
/// core's quoting policy. Returns the final query text, or `null` on cancel.
/// Nothing runs: the caller only puts the text into the editor.
Future<String?> pickDbSnippet(BuildContext context, WidgetRef ref, {required DbEngine engine}) async {
  final profile = ref.read(activeProfileProvider)?.id;
  final snippet = await showAppDialog<Snippet>(context, builder: (_) => DbSnippetPicker(engine: engine));
  if (snippet == null || !context.mounted) return null;
  var values = const <String, String>{};
  if (snippet.effectiveVariables.isNotEmpty) {
    final chosen = await showVariablesDialog(context, snippet);
    if (chosen == null || !context.mounted) return null;
    values = chosen;
  }
  final service = ref.read(snippetServiceProvider);
  final String text;
  try {
    text = await service.render(snippet, values);
  } on AppException catch (e) {
    if (context.mounted) showSnack(context, errorMessage(context.l10n, e), error: true);
    return null;
  }
  if (!context.mounted || profile != ref.read(activeProfileProvider)?.id) return null;
  try {
    await service.recordUsage(snippet.id);
  } on AppException {
    // Usage statistics are best effort; the text is still inserted.
  }
  return text;
}

/// The chooser dialog; pops with the selected [Snippet].
class DbSnippetPicker extends ConsumerStatefulWidget {
  const DbSnippetPicker({required this.engine, super.key});

  final DbEngine engine;

  @override
  ConsumerState<DbSnippetPicker> createState() => _DbSnippetPickerState();
}

class _DbSnippetPickerState extends ConsumerState<DbSnippetPicker> {
  String _query = '';

  List<Snippet> _matching(List<Snippet> all) {
    final words = _query.trim().toLowerCase().split(RegExp(r'\s+')).where((w) => w.isNotEmpty).toList();
    final out = [
      for (final s in all)
        if (snippetMatchesEngine(s, widget.engine) &&
            words.every(
              [s.name, s.description, s.packageName ?? '', s.template, ...s.tags].join('\n').toLowerCase().contains,
            ))
          s,
    ];
    out.sort((a, b) {
      final usage = b.usageCount.compareTo(a.usageCount);
      if (usage != 0) return usage;
      final package = (a.packageName ?? '').compareTo(b.packageName ?? '');
      return package != 0 ? package : a.name.toLowerCase().compareTo(b.name.toLowerCase());
    });
    return out;
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    final tokens = GlassTokens.of(context);
    final snippets = _matching(ref.watch(snippetsProvider).value ?? const <Snippet>[]);
    return GlassDialog(
      key: const ValueKey('db-snippet-picker'),
      title: l.dbSnippetPickerTitle,
      width: 620,
      content: SizedBox(
        height: 440,
        child: Column(
          children: [
            GlassField(
              key: const ValueKey('db-snippet-search'),
              search: true,
              size: GlassFieldSize.lg,
              autofocus: true,
              leadingIcon: Icons.search_rounded,
              placeholder: l.dbSnippetPickerSearch(widget.engine.label),
              onChanged: (q) => setState(() => _query = q),
            ),
            const SizedBox(height: GlassSpacing.s8),
            Expanded(
              child: snippets.isEmpty
                  ? Center(
                      child: Text(
                        l.dbSnippetPickerEmpty(widget.engine.label),
                        key: const ValueKey('db-snippet-empty'),
                        textAlign: TextAlign.center,
                        style: tokens.typography.callout.copyWith(color: tokens.secondaryLabel),
                      ),
                    )
                  : ListView.builder(
                      itemCount: snippets.length,
                      itemBuilder: (context, i) {
                        final s = snippets[i];
                        final firstLine = s.template
                            .split('\n')
                            .firstWhere(
                              (line) => line.trim().isNotEmpty && !line.trimLeft().startsWith('--'),
                              orElse: () => s.template,
                            );
                        return ListTile(
                          key: ValueKey('db-snippet-${s.id.value}'),
                          title: Text(s.name, style: tokens.typography.bodyEmph.copyWith(color: tokens.palette.label)),
                          subtitle: Column(
                            crossAxisAlignment: CrossAxisAlignment.start,
                            children: [
                              if (s.packageName?.isNotEmpty == true)
                                Text(
                                  s.packageName!,
                                  style: tokens.typography.caption.copyWith(color: tokens.secondaryLabel),
                                ),
                              Text(
                                firstLine.trim(),
                                maxLines: 1,
                                overflow: TextOverflow.ellipsis,
                                style: tokens.typography.mono.copyWith(fontSize: 12, color: tokens.secondaryLabel),
                              ),
                            ],
                          ),
                          trailing: RiskBadge(risk: s.riskLevel, dense: true),
                          onTap: () => closeDialog(context, s),
                        );
                      },
                    ),
            ),
          ],
        ),
      ),
      secondaryActions: [GlassButton(label: l.commonCancel, onPressed: () => closeDialog<Snippet>(context))],
    );
  }
}
