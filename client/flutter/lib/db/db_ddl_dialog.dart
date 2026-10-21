import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/db/db_editor.dart';
import 'package:consolecrypt/db/db_models.dart';
import 'package:consolecrypt/db/db_providers.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

/// Top-level statements of SQL text, each with its `;`. Quotes, dollar
/// quotes and comments never split. Display only: what runs is parsed natively.
List<String> splitSqlStatements(String text) {
  final statements = <String>[];
  var start = 0, i = 0;
  String? dollar;
  void cut(int end) {
    final statement = text.substring(start, end).trim();
    if (statement.isNotEmpty) statements.add(statement);
    start = end;
  }

  while (i < text.length) {
    final c = text[i];
    if (dollar != null) {
      if (text.startsWith(dollar, i)) {
        i += dollar.length;
        dollar = null;
      } else {
        i++;
      }
      continue;
    }
    if (c == "'" || c == '"' || c == '`') {
      i++;
      while (i < text.length) {
        if (text[i] == c) {
          if (i + 1 < text.length && text[i + 1] == c) {
            i += 2;
            continue;
          }
          break;
        }
        i++;
      }
      i++;
      continue;
    }
    if (text.startsWith('--', i)) {
      final end = text.indexOf('\n', i);
      i = end < 0 ? text.length : end + 1;
      continue;
    }
    if (text.startsWith('/*', i)) {
      final end = text.indexOf('*/', i + 2);
      i = end < 0 ? text.length : end + 2;
      continue;
    }
    if (c == r'$') {
      final tag = RegExp(r'\$[A-Za-z_]*\$').matchAsPrefix(text, i);
      if (tag != null) {
        dollar = tag[0];
        i = tag.end;
        continue;
      }
    }
    if (c == ';') cut(i + 1);
    i++;
  }
  cut(text.length);
  return statements;
}

/// Read-only code with line numbers, syntax colours and horizontal scrolling;
/// [blocks] are separated by a divider.
class DbCodeView extends StatelessWidget {
  const DbCodeView({required this.blocks, super.key});
  final List<String> blocks;

  @override
  Widget build(BuildContext context) {
    final tokens = GlassTokens.of(context);
    final mono = tokens.typography.mono.copyWith(height: 1.5, color: tokens.palette.label);
    final strut = StrutStyle.fromTextStyle(mono, forceStrutHeight: true);
    final lineHeight = (mono.fontSize ?? 13) * 1.5;
    final lines = [for (final block in blocks) '\n'.allMatches(block).length + 1];
    final total = lines.fold<int>(0, (sum, n) => sum + n);
    final gutter = (mono.fontSize ?? 13) * .62 * '$total'.length.clamp(2, 6) + GlassSpacing.s16;
    Widget divider() => SizedBox(
      height: lineHeight,
      child: Center(child: Container(height: 1, color: tokens.surfaces.separator)),
    );
    var first = 1;
    final numbers = <Widget>[];
    for (var b = 0; b < blocks.length; b++) {
      if (b > 0) numbers.add(SizedBox(height: lineHeight));
      numbers.add(
        Text(
          [for (var n = first; n < first + lines[b]; n++) '$n'].join('\n'),
          textAlign: TextAlign.right,
          softWrap: false,
          strutStyle: strut,
          style: mono.copyWith(color: tokens.palette.tertiary),
        ),
      );
      first += lines[b];
    }
    return SingleChildScrollView(
      key: const ValueKey('db-code-vertical'),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Container(
            width: gutter,
            padding: const EdgeInsets.only(right: GlassSpacing.s8),
            decoration: BoxDecoration(
              border: Border(right: BorderSide(color: tokens.surfaces.separator)),
            ),
            child: Column(crossAxisAlignment: CrossAxisAlignment.end, children: numbers),
          ),
          Expanded(
            child: SingleChildScrollView(
              key: const ValueKey('db-code-horizontal'),
              scrollDirection: Axis.horizontal,
              padding: const EdgeInsets.symmetric(horizontal: GlassSpacing.s8),
              child: SelectionArea(
                child: IntrinsicWidth(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.stretch,
                    children: [
                      for (var b = 0; b < blocks.length; b++) ...[
                        if (b > 0) divider(),
                        Text.rich(
                          dbHighlight(context, blocks[b], style: mono),
                          key: ValueKey('db-code-block-$b'),
                          softWrap: false,
                          strutStyle: strut,
                        ),
                      ],
                    ],
                  ),
                ),
              ),
            ),
          ),
        ],
      ),
    );
  }
}

/// DDL of a structure object as a code block: copy it or open it in a new
/// query tab (closes with true). Wiped and closed when the vault scope ends.
class DbDdlDialog extends ConsumerStatefulWidget {
  const DbDdlDialog({required this.title, required this.value, required this.engine, super.key});
  final String title;
  final SecretText value;
  final DbEngine engine;
  @override
  ConsumerState<DbDdlDialog> createState() => _DbDdlDialogState();
}

class _DbDdlDialogState extends ConsumerState<DbDdlDialog> {
  late final _scope = ref.read(dbScopeProvider);

  @override
  Widget build(BuildContext context) {
    ref.listen(dbScopeProvider, (_, next) {
      if (!identical(next, _scope)) {
        widget.value.wipe();
        WidgetsBinding.instance.addPostFrameCallback((_) {
          if (mounted) closeDialog<void>(context);
        });
      }
    });
    if (!_scope.unlocked || !identical(_scope, ref.watch(dbScopeProvider)) || widget.value.isWiped) {
      return const SizedBox.shrink();
    }
    final l = context.l10n, text = widget.value.expose();
    final blocks = widget.engine.sql ? splitSqlStatements(text) : [text];
    return GlassDialog(
      key: const ValueKey('db-ddl-dialog'),
      title: widget.title,
      content: SizedBox(
        width: 860,
        height: MediaQuery.sizeOf(context).height * .6,
        child: DbCodeView(blocks: blocks.isEmpty ? [text] : blocks),
      ),
      secondaryActions: [
        GlassButton(label: l.commonClose, onPressed: () => closeDialog<bool>(context, false)),
        GlassButton(
          key: const ValueKey('db-ddl-copy'),
          label: l.commonCopy,
          icon: Icons.content_copy_rounded,
          onPressed: () => copyPlainWithNotice(context, ref, blocks.join('\n\n'), what: l.copyWhatDdl),
        ),
      ],
      primaryAction: GlassButton(
        key: const ValueKey('db-ddl-open'),
        label: l.dbOpenInEditor,
        icon: Icons.edit_note_rounded,
        style: GlassButtonStyle.prominent,
        onPressed: () => closeDialog<bool>(context, true),
      ),
    );
  }
}
