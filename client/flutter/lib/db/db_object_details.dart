import 'dart:convert';

import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/util/formatting.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/db/db_ddl_dialog.dart';
import 'package:consolecrypt/db/db_models.dart';
import 'package:consolecrypt/db/db_providers.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

/// Whether "Object details" has something to show for [object] (mirrors
/// `has_details` in the native core).
bool dbHasDetails(DbEngine engine, DbObjectRef object) => switch (engine) {
  DbEngine.postgresql || DbEngine.mysql || DbEngine.mariadb => const [
    DbObjectKind.table,
    DbObjectKind.view,
    DbObjectKind.materializedView,
  ].contains(object.kind),
  DbEngine.clickhouse => const [
    DbObjectKind.table,
    DbObjectKind.view,
    DbObjectKind.materializedView,
    DbObjectKind.dictionary,
  ].contains(object.kind),
  DbEngine.redis =>
    object.kind == DbObjectKind.redisKey || (object.kind == DbObjectKind.redisDatabase && object.parentName == null),
  DbEngine.mongodb => const [DbObjectKind.database, DbObjectKind.collection, DbObjectKind.view].contains(object.kind),
  DbEngine.sqlite => const [
    DbObjectKind.database,
    DbObjectKind.table,
    DbObjectKind.view,
    DbObjectKind.indexObject,
  ].contains(object.kind),
  DbEngine.cassandra => const [
    DbObjectKind.database,
    DbObjectKind.table,
    DbObjectKind.materializedView,
  ].contains(object.kind),
};

/// One catalogue fact as the core sent it.
final class DbDetailField {
  const DbDetailField({required this.id, required this.value, required this.kind, this.unit});
  final String id, value, kind;
  final String? unit;

  /// The label id: a length is named after what it counts.
  String get labelId => id == 'length' && unit != null ? 'length_$unit' : id;
}

final class DbDetailSection {
  const DbDetailSection(this.id, this.fields);
  final String id;
  final List<DbDetailField> fields;
}

/// Sections of a details document (`{"sections": [...]}`); empty for a raw
/// MongoDB description.
List<DbDetailSection> dbDetailSections(DbJson document) {
  final sections = document['sections'];
  if (sections is! List) return const [];
  return [
    for (final section in sections.whereType<Map<String, dynamic>>())
      DbDetailSection(section['id'] as String? ?? '', [
        for (final field in (section['fields'] as List? ?? const []).whereType<Map<String, dynamic>>())
          DbDetailField(
            id: field['id'] as String? ?? '',
            value: field['value'] as String? ?? '',
            kind: field['kind'] as String? ?? 'text',
            unit: field['unit'] as String?,
          ),
      ]),
  ];
}

/// A field's value in the UI language: sizes in KiB/MiB, counts grouped,
/// estimates with "≈", times in local time.
String dbDetailValue(AppLocalizations l, DbDetailField field) {
  final number = int.tryParse(field.value);
  switch (field.kind) {
    case 'bytes' when number != null:
      return formatBytes(l, number);
    case 'count' when number != null:
      return formatCount(l, number);
    case 'estimate' when number != null:
      return '≈ ${formatCount(l, number)}';
    case 'keyword':
      return l.dbDetailsKeyword(field.value);
    case 'ttl' when number != null:
      return number < 0 ? l.dbDetailsNoExpiry : formatRemaining(l, DateTime.now().add(Duration(milliseconds: number)));
    case 'time':
      final time = DateTime.tryParse(field.value);
      return time == null ? field.value : formatDateTime(l, time);
  }
  return field.value;
}

/// "Object details" of a table, view or key: labelled facts grouped in
/// sections, with a copy button. A raw MongoDB description is shown as JSON.
/// Wiped and closed when the vault scope ends.
class DbObjectDetailsDialog extends ConsumerStatefulWidget {
  const DbObjectDetailsDialog({required this.title, required this.document, super.key});
  final String title;

  /// The core's answer as JSON text; wiped with the dialog.
  final SecretText document;
  @override
  ConsumerState<DbObjectDetailsDialog> createState() => _DbObjectDetailsDialogState();
}

class _DbObjectDetailsDialogState extends ConsumerState<DbObjectDetailsDialog> {
  late final _scope = ref.read(dbScopeProvider);
  bool _json = true;

  String _plain(AppLocalizations l, List<DbDetailSection> sections, String json) {
    if (sections.isEmpty) return json;
    return [
      for (final section in sections)
        [
          l.dbDetailsSection(section.id),
          for (final field in section.fields) '${l.dbDetailsField(field.labelId)}: ${dbDetailValue(l, field)}',
        ].join('\n'),
    ].join('\n\n');
  }

  @override
  Widget build(BuildContext context) {
    ref.listen(dbScopeProvider, (_, next) {
      if (!identical(next, _scope)) {
        widget.document.wipe();
        WidgetsBinding.instance.addPostFrameCallback((_) {
          if (mounted) closeDialog<void>(context);
        });
      }
    });
    if (!_scope.unlocked || !identical(_scope, ref.watch(dbScopeProvider)) || widget.document.isWiped) {
      return const SizedBox.shrink();
    }
    final l = context.l10n, tokens = GlassTokens.of(context);
    final text = widget.document.expose();
    Object? decoded;
    try {
      decoded = jsonDecode(text);
    } catch (_) {
      decoded = text;
    }
    final sections = decoded is Map<String, dynamic> ? dbDetailSections(decoded) : const <DbDetailSection>[];
    final json = decoded is String ? decoded : const JsonEncoder.withIndent('  ').convert(decoded);
    final label = tokens.typography.callout.copyWith(color: tokens.secondaryLabel);
    final value = tokens.typography.callout.copyWith(color: tokens.palette.label);
    Widget row(DbDetailField field) => Padding(
      key: ValueKey('db-details-${field.id}'),
      padding: const EdgeInsets.symmetric(vertical: GlassSpacing.s4),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          SizedBox(width: 200, child: Text(l.dbDetailsField(field.labelId), style: label)),
          const SizedBox(width: GlassSpacing.s12),
          Expanded(
            child: SelectableText(
              dbDetailValue(l, field),
              style: field.kind == 'code' ? tokens.typography.mono.copyWith(color: tokens.palette.label) : value,
            ),
          ),
        ],
      ),
    );
    return GlassDialog(
      key: const ValueKey('db-details-dialog'),
      title: widget.title,
      width: 620,
      content: ConstrainedBox(
        constraints: BoxConstraints(maxHeight: MediaQuery.sizeOf(context).height * .7),
        child: SingleChildScrollView(
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              for (final (i, section) in sections.indexed) ...[
                if (i > 0) const SizedBox(height: GlassSpacing.s12),
                Text(
                  l.dbDetailsSection(section.id),
                  key: ValueKey('db-details-section-${section.id}'),
                  style: tokens.typography.bodyEmph.copyWith(color: tokens.palette.label),
                ),
                const SizedBox(height: GlassSpacing.s4),
                for (final field in section.fields) row(field),
              ],
              if (sections.isEmpty && json.trim().isEmpty) Text(l.dbDetailsEmpty),
              if (sections.isEmpty && json.trim().isNotEmpty) ...[
                Align(
                  alignment: Alignment.centerLeft,
                  child: GlassButton.plain(
                    key: const ValueKey('db-details-json-toggle'),
                    label: l.dbDetailsJson,
                    icon: _json ? Icons.expand_less_rounded : Icons.expand_more_rounded,
                    onPressed: () => setState(() => _json = !_json),
                  ),
                ),
                if (_json) DbCodeBlock(key: const ValueKey('db-details-json'), text: json, maxHeight: 420),
              ],
            ],
          ),
        ),
      ),
      secondaryActions: [
        GlassButton(label: l.commonClose, onPressed: () => closeDialog<void>(context)),
        GlassButton(
          key: const ValueKey('db-details-copy'),
          label: l.commonCopy,
          icon: Icons.content_copy_rounded,
          onPressed: () => copyPlainWithNotice(context, ref, _plain(l, sections, json), what: l.copyWhatObjectDetails),
        ),
      ],
    );
  }
}
