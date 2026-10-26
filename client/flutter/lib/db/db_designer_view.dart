import 'dart:async';
import 'dart:math' as math;

import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/db/db_ddl_dialog.dart';
import 'package:consolecrypt/db/db_designer_controller.dart';
import 'package:consolecrypt/db/db_designer_models.dart';
import 'package:consolecrypt/db/db_models.dart';
import 'package:consolecrypt/db/db_query_service.dart';
import 'package:consolecrypt/db/db_tree_overlay.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

enum DbDesignerSection { columns, keys, foreignKeys, indexes, checks, options, ddl }

/// A validation finding as text.
String dbDesignIssueText(DbDesignIssue issue, AppLocalizations l) {
  final v = issue.value;
  return switch (issue.code) {
    'name_required' => l.dbDesignIssueNameRequired,
    'name_invalid' => l.dbDesignIssueNameInvalid(v),
    'name_too_long' => l.dbDesignIssueNameTooLong(v),
    'name_spaces' => l.dbDesignIssueNameSpaces(v),
    'name_duplicate' => l.dbDesignIssueNameDuplicate(v),
    'columns_required' => l.dbDesignIssueColumnsRequired,
    'type_invalid' => l.dbDesignIssueTypeInvalid(v),
    'length_required' => l.dbDesignIssueLengthRequired(v),
    'length_ignored' => l.dbDesignIssueLengthIgnored(v),
    'length_invalid' || 'scale_invalid' => l.dbDesignIssueLengthInvalid(v),
    'expression_invalid' => l.dbDesignIssueExpressionInvalid(v),
    'expression_required' => l.dbDesignIssueExpressionRequired,
    'identity_unsupported' || 'identity_type' => l.dbDesignIssueIdentityType(v),
    'identity_default' => l.dbDesignIssueIdentityDefault(v),
    'identity_change_unsupported' => l.dbDesignIssueIdentityChange(v),
    'key_columns_required' => l.dbDesignIssueKeyColumns,
    'column_unknown' => l.dbDesignIssueColumnUnknown(v),
    'column_repeated' => l.dbDesignIssueColumnRepeated(v),
    'ref_table_required' => l.dbDesignIssueRefTable,
    'ref_columns_mismatch' => l.dbDesignIssueRefColumns,
    'auto_increment_key' => l.dbDesignIssueAutoIncrementKey(v),
    'auto_increment_single' => l.dbDesignIssueAutoIncrementSingle,
    'unsigned_type' => l.dbDesignIssueUnsigned(v),
    'foreign_keys_engine' => l.dbDesignIssueForeignKeysEngine(v),
    'order_by_required' => l.dbDesignIssueOrderBy,
    'primary_key_prefix' => l.dbDesignIssuePrimaryKeyPrefix(v),
    'nullable_key' => l.dbDesignIssueNullableKey(v),
    'primary_key_engine' || 'option_engine' => l.dbDesignIssueEngineOption(v),
    'partition_key_required' => l.dbDesignIssuePartitionKey,
    'unlogged_partitioned' => l.dbDesignIssueUnloggedPartitioned,
    'partition_key_not_in_primary_key' => l.dbDesignIssuePartitionPrimaryKey,
    'comment_backslash_unsupported' => l.dbDesignIssueCommentBackslash,
    'option_invalid' || 'option_unknown' => l.dbDesignIssueOptionInvalid(v),
    'option_create_only' || 'option_change_unsupported' => l.dbDesignIssueOptionCreateOnly(v),
    'comment_change_unsupported' => l.dbDesignIssueCommentChange,
    'not_null_without_default' => l.dbDesignIssueNotNullDefault(v),
    'column_order_ignored' => l.dbDesignIssueColumnOrder(v),
    'rename_unsupported' || 'move_unsupported' => l.dbDesignIssueRename,
    'primary_key_change_unsupported' => l.dbDesignIssuePrimaryKeyChange,
    'capped_size_required' => l.dbDesignIssueCappedSize,
    'capped_time_series' => l.dbDesignIssueCappedTimeSeries,
    'time_field_required' => l.dbDesignIssueTimeField,
    'collation_locale_required' => l.dbDesignIssueCollationLocale,
    'default_ignored' => l.dbDesignIssueDefaultIgnored(v),
    'nullable_twice' => l.dbDesignIssueNullableTwice(v),
    'autoincrement_key' => l.dbDesignIssueAutoincrementKey(v),
    'autoincrement_single' => l.dbDesignIssueAutoincrementSingle,
    'rebuild_blocked' => l.dbDesignIssueRebuildBlocked(_unmodeled(v, l)),
    'table_rebuild' => l.dbDesignIssueTableRebuild(v),
    'type_strict' => l.dbDesignIssueTypeStrict(v),
    'primary_key_required' => l.dbDesignIssuePrimaryKeyRequired,
    'comment_ignored' => l.dbDesignIssueCommentIgnored(v),
    'column_rename_unsupported' => l.dbDesignIssueColumnRename(v),
    'type_change_unsupported' => l.dbDesignIssueTypeChange(v),
    'counter_mixed' => l.dbDesignIssueCounterMixed(v),
    'index_single_column' => l.dbDesignIssueIndexSingleColumn,
    _ => l.dbDesignIssueOther(issue.code),
  };
}

/// What the designer does not carry over, as words (`triggers, …`).
String _unmodeled(String codes, AppLocalizations l) => {
  for (final code in codes.split(', '))
    switch (code) {
      'triggers' => l.dbDesignUnmodeledTriggers,
      'generated_columns' => l.dbDesignUnmodeledGenerated,
      'collations' => l.dbDesignUnmodeledCollations,
      'expression_indexes' || 'partial_indexes' || 'index_order' => l.dbDesignUnmodeledIndexes,
      _ => l.dbDesignUnmodeledOther,
    },
}.join(', ');

/// Label of an engine option on the Options tab.
String dbDesignOptionLabel(String id, AppLocalizations l) => switch (id) {
  'engine' => l.dbDesignOptEngine,
  'engine_args' => l.dbDesignOptEngineArgs,
  'order_by' => l.dbDesignOptOrderBy,
  'partition_by' => l.dbDesignOptPartitionBy,
  'partition_key' => l.dbDesignOptPartitionKey,
  'charset' => l.dbDesignOptCharset,
  'collation' => l.dbDesignOptCollation,
  'auto_increment' => l.dbDesignOptAutoIncrement,
  'capped' => l.dbDesignOptCapped,
  'size' => l.dbDesignOptSize,
  'max' => l.dbDesignOptMax,
  'time_field' => l.dbDesignOptTimeField,
  'meta_field' => l.dbDesignOptMetaField,
  'granularity' => l.dbDesignOptGranularity,
  'expire_after_seconds' => l.dbDesignOptExpire,
  'validation_level' => l.dbDesignOptValidationLevel,
  'validation_action' => l.dbDesignOptValidationAction,
  'collation_locale' => l.dbDesignOptCollationLocale,
  'collation_strength' => l.dbDesignOptCollationStrength,
  'strict' => l.dbDesignOptStrict,
  'without_rowid' => l.dbDesignOptWithoutRowid,
  'unlogged' => l.dbDesignOptUnlogged,
  'partition_key_columns' => l.dbDesignOptPartitionColumns,
  'clustering_order' => l.dbDesignOptClusteringOrder,
  'default_time_to_live' => l.dbDesignOptDefaultTtl,
  'gc_grace_seconds' => l.dbDesignOptGcGrace,
  _ => id,
};

String _identityLabel(String kind, AppLocalizations l) => switch (kind) {
  'by_default' => l.dbDesignIdentityByDefault,
  'always' => l.dbDesignIdentityAlways,
  'serial' => l.dbDesignIdentitySerial,
  'auto_increment' => l.dbDesignIdentityAutoIncrement,
  'autoincrement' => l.dbDesignIdentityAutoincrement,
  _ => l.dbDesignNone,
};

/// The table designer of one workspace tab: header, sections and the live
/// DDL. Edits stay in the draft; [onSave] (also ⌘S / Ctrl+S) opens the
/// save dialog.
class DbTableDesignerView extends ConsumerStatefulWidget {
  const DbTableDesignerView({
    required this.designer,
    required this.session,
    required this.queryService,
    required this.onSave,
    required this.onOpenInEditor,
    super.key,
  });
  final DbDesignerController designer;
  final DbSessionInfo session;
  final DbQueryService queryService;
  final VoidCallback onSave;
  final ValueChanged<String> onOpenInEditor;
  @override
  ConsumerState<DbTableDesignerView> createState() => _DbTableDesignerViewState();
}

class _DbTableDesignerViewState extends ConsumerState<DbTableDesignerView> {
  DbDesignerSection _section = DbDesignerSection.columns;
  DbDesignerController get d => widget.designer;
  DbDesignDialect get dialect => d.dialect;

  @override
  void initState() {
    super.initState();
    d.addListener(_changed);
  }

  @override
  void didUpdateWidget(DbTableDesignerView old) {
    super.didUpdateWidget(old);
    if (!identical(old.designer, widget.designer)) {
      old.designer.removeListener(_changed);
      widget.designer.addListener(_changed);
    }
  }

  @override
  void dispose() {
    d.removeListener(_changed);
    super.dispose();
  }

  void _changed() {
    if (mounted) setState(() {});
  }

  List<DbDesignerSection> get _sections => [
    DbDesignerSection.columns,
    if (dialect.has('primary_key') || dialect.has('unique')) DbDesignerSection.keys,
    if (dialect.has('foreign_keys')) DbDesignerSection.foreignKeys,
    if (dialect.has('indexes')) DbDesignerSection.indexes,
    if (dialect.has('checks')) DbDesignerSection.checks,
    if (dialect.options.isNotEmpty) DbDesignerSection.options,
    DbDesignerSection.ddl,
  ];

  String _sectionLabel(DbDesignerSection section, AppLocalizations l) => switch (section) {
    DbDesignerSection.columns => dialect.collection ? l.dbDesignFields : l.dbDesignColumns,
    DbDesignerSection.keys => l.dbDesignKeys,
    DbDesignerSection.foreignKeys => l.dbDesignForeignKeys,
    DbDesignerSection.indexes => l.dbDesignIndexes,
    DbDesignerSection.checks => l.dbDesignChecks,
    DbDesignerSection.options => l.dbDesignOptions,
    DbDesignerSection.ddl => l.dbDesignDdl,
  };

  String _sectionPrefix(DbDesignerSection section) => switch (section) {
    DbDesignerSection.columns => 'columns',
    DbDesignerSection.keys => 'primary_key|uniques',
    DbDesignerSection.foreignKeys => 'foreign_keys',
    DbDesignerSection.indexes => 'indexes',
    DbDesignerSection.checks => 'checks',
    DbDesignerSection.options => 'options',
    DbDesignerSection.ddl => '',
  };

  List<DbDesignIssue> _issues(DbDesignerSection section) {
    if (section == DbDesignerSection.ddl) return d.plan.issues;
    final prefixes = _sectionPrefix(section).split('|');
    return [
      for (final issue in d.plan.issues)
        if (prefixes.any((p) => issue.target == p || issue.target.startsWith('$p.'))) issue,
    ];
  }

  bool _rowHasError(String prefix) =>
      d.plan.issues.any((i) => i.error && (i.target == prefix || i.target.startsWith('$prefix.')));

  // Shared bits ------------------------------------------------------------------

  Widget _marker(DbDraftState? state, {bool error = false}) {
    final p = GlassTokens.of(context).palette, l = context.l10n;
    final color = error
        ? p.danger
        : switch (state) {
            DbDraftState.added => p.success,
            DbDraftState.changed => p.warning,
            null => Colors.transparent,
          };
    return Tooltip(
      message: error
          ? l.dbDesignHasErrors
          : switch (state) {
              DbDraftState.added => l.dbDesignNew,
              DbDraftState.changed => l.dbDesignChanged,
              null => '',
            },
      child: Container(
        width: 4,
        height: 22,
        margin: const EdgeInsets.only(right: GlassSpacing.s6),
        decoration: BoxDecoration(color: color, borderRadius: BorderRadius.circular(2)),
      ),
    );
  }

  Widget _check(String key, bool value, String tooltip, ValueChanged<bool>? onChanged, {double width = 44}) => SizedBox(
    width: width,
    child: Tooltip(
      message: tooltip,
      child: Checkbox(
        key: ValueKey(key),
        value: value,
        visualDensity: VisualDensity.compact,
        materialTapTargetSize: MaterialTapTargetSize.shrinkWrap,
        onChanged: onChanged == null ? null : (v) => onChanged(v ?? false),
      ),
    ),
  );

  Widget _icon(String key, IconData icon, String tooltip, VoidCallback? onPressed) => GlassIconButton(
    key: ValueKey(key),
    icon: icon,
    tooltip: tooltip,
    size: 26,
    iconSize: 15,
    style: GlassIconButtonStyle.plain,
    onPressed: onPressed,
  );

  Widget _caption(String text, {double? width}) {
    final tokens = GlassTokens.of(context);
    final label = Text(
      text,
      maxLines: 1,
      overflow: TextOverflow.ellipsis,
      style: tokens.typography.caption.copyWith(color: tokens.secondaryLabel, fontWeight: FontWeight.w600),
    );
    return width == null ? label : SizedBox(width: width, child: label);
  }

  /// Picks several column names from [options] (a check-mark menu).
  Widget _columnsPicker(
    String key,
    List<String> selected,
    List<String> options,
    ValueChanged<List<String>> onChanged, {
    double width = 220,
    Future<List<String>> Function()? load,
  }) {
    final tokens = GlassTokens.of(context), l = context.l10n;
    return SizedBox(
      width: width,
      child: Builder(
        builder: (context) => GlassInteractive(
          key: ValueKey(key),
          semanticLabel: l.dbDesignPickColumns,
          onPressed: () async {
            final anchor = glassAnchorRect(context);
            final names = load == null ? options : await load();
            if (!mounted || !context.mounted) return;
            final picked = await showGlassMenu<String>(
              context: context,
              anchor: anchor,
              minWidth: math.max(anchor.width, 180),
              entries: [
                for (final name in {...names, ...selected})
                  GlassMenuItem(
                    key: ValueKey('$key-$name'),
                    value: name,
                    label: name,
                    checked: selected.contains(name),
                  ),
              ],
            );
            if (picked == null) return;
            final next = List.of(selected);
            if (!next.remove(picked)) next.add(picked);
            onChanged(next);
          },
          builder: (context, state) => DecoratedBox(
            decoration: ShapeDecoration(
              color: state.hovered
                  ? Color.alphaBlend(tokens.surfaces.fillHover, tokens.surfaces.fillField)
                  : tokens.surfaces.fillField,
              shape: GlassRadii.shape(tokens.radii.md),
            ),
            child: SizedBox(
              height: 28,
              child: Padding(
                padding: const EdgeInsets.symmetric(horizontal: GlassSpacing.s8),
                child: Row(
                  children: [
                    Expanded(
                      child: Text(
                        selected.isEmpty ? l.dbDesignPickColumns : selected.join(', '),
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis,
                        style: tokens.typography.callout.copyWith(
                          color: selected.isEmpty ? tokens.palette.tertiary : tokens.palette.label,
                        ),
                      ),
                    ),
                    Icon(Icons.unfold_more_rounded, size: 14, color: tokens.secondaryLabel),
                  ],
                ),
              ),
            ),
          ),
        ),
      ),
    );
  }

  Widget _select<T>(String key, T value, List<GlassSelectItem<T>> items, ValueChanged<T>? onChanged, {double? width}) {
    final select = GlassSelect<T>(key: ValueKey(key), value: value, items: items, onChanged: onChanged, expand: true);
    return width == null ? select : SizedBox(width: width, child: select);
  }

  List<String> get _columnNames => [for (final c in d.design.columns) c.name];

  Widget _issueList(List<DbDesignIssue> issues) {
    if (issues.isEmpty) return const SizedBox.shrink();
    final tokens = GlassTokens.of(context), l = context.l10n;
    return Padding(
      padding: const EdgeInsets.fromLTRB(GlassSpacing.s12, GlassSpacing.s6, GlassSpacing.s12, 0),
      child: Column(
        key: const ValueKey('db-design-issues'),
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          for (final issue in issues.take(8))
            Padding(
              padding: const EdgeInsets.symmetric(vertical: 1),
              child: Row(
                children: [
                  Icon(
                    issue.error ? Icons.error_outline_rounded : Icons.info_outline_rounded,
                    size: 14,
                    color: issue.error ? tokens.palette.danger : tokens.palette.warning,
                  ),
                  const SizedBox(width: GlassSpacing.s6),
                  Expanded(
                    child: Text(
                      dbDesignIssueText(issue, l),
                      style: tokens.typography.caption.copyWith(
                        color: issue.error ? tokens.palette.danger : tokens.secondaryLabel,
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

  // Header -------------------------------------------------------------------------

  Widget _header(AppLocalizations l, GlassTokens tokens) {
    final design = d.design;
    final markerState = d.alter ? (d.dirty ? DbDraftState.changed : null) : DbDraftState.added;
    return ContentSurface(
      key: const ValueKey('db-design-header'),
      padding: const EdgeInsets.fromLTRB(GlassSpacing.s12, GlassSpacing.s8, GlassSpacing.s8, GlassSpacing.s8),
      child: Wrap(
        spacing: GlassSpacing.s8,
        runSpacing: GlassSpacing.s8,
        crossAxisAlignment: WrapCrossAlignment.center,
        children: [
          Row(
            mainAxisSize: MainAxisSize.min,
            children: [
              _marker(markerState, error: _rowHasError('table')),
              Icon(dialect.collection ? Icons.dataset_outlined : Icons.table_chart_outlined, size: 18),
              const SizedBox(width: GlassSpacing.s8),
              _TextCell(
                key: const ValueKey('db-design-name'),
                value: design.name,
                placeholder: dialect.collection ? l.dbDesignCollectionName : l.dbDesignTableName,
                width: 220,
                onChanged: (v) => d.edit((x) => x.name = v, coalesce: 'name'),
              ),
            ],
          ),
          if (dialect.schemas)
            _TextCell(
              key: const ValueKey('db-design-schema'),
              value: design.schema ?? '',
              placeholder: l.dbDesignSchema,
              width: 140,
              enabled: !d.alter,
              onChanged: (v) => d.edit((x) => x.schema = v.isEmpty ? null : v, coalesce: 'schema'),
            )
          else if (design.database.isNotEmpty)
            GlassBadge(label: design.database, icon: Icons.storage_rounded, dense: true),
          if (dialect.has('table_comment'))
            _TextCell(
              key: const ValueKey('db-design-comment'),
              value: design.comment,
              placeholder: l.dbDesignComment,
              width: 260,
              onChanged: (v) => d.edit((x) => x.comment = v, coalesce: 'comment'),
            ),
          GlassBadge(
            key: const ValueKey('db-design-mode'),
            label: d.alter ? l.dbDesignModeAlter : l.dbDesignModeCreate,
            tone: d.alter ? GlassTone.neutral : GlassTone.success,
            dense: true,
          ),
          if (d.alter && d.dirty)
            GlassBadge(
              key: const ValueKey('db-design-pending'),
              label: l.dbDesignPending,
              tone: GlassTone.warning,
              icon: Icons.circle,
              dense: true,
            ),
          Row(
            mainAxisSize: MainAxisSize.min,
            children: [
              _icon('db-design-undo', Icons.undo_rounded, l.dbDesignUndo, d.canUndo ? d.undo : null),
              _icon('db-design-revert', Icons.restore_rounded, l.dbDesignRevert, d.dirty ? d.revertAll : null),
              const SizedBox(width: GlassSpacing.s6),
              GlassButton(
                key: const ValueKey('db-design-save'),
                label: l.dbDesignSave,
                icon: Icons.save_outlined,
                style: GlassButtonStyle.prominent,
                size: GlassControlSize.sm,
                tooltip: l.dbDesignSaveShortcut,
                onPressed: d.canSave ? widget.onSave : null,
              ),
            ],
          ),
        ],
      ),
    );
  }

  Widget _sectionBar(AppLocalizations l, GlassTokens tokens) => SizedBox(
    height: 32,
    child: ListView(
      scrollDirection: Axis.horizontal,
      children: [
        for (final section in _sections)
          Padding(
            padding: const EdgeInsets.only(right: GlassSpacing.s4),
            child: GlassInteractive(
              key: ValueKey('db-design-section-${section.name}'),
              selected: _section == section,
              semanticLabel: _sectionLabel(section, l),
              onPressed: () => setState(() => _section = section),
              builder: (context, state) {
                final errors = _issues(section).where((i) => i.error).length;
                final active = _section == section;
                return DecoratedBox(
                  decoration: ShapeDecoration(
                    color: active
                        ? tokens.sidebarSelection
                        : state.hovered
                        ? tokens.surfaces.fillHover
                        : Colors.transparent,
                    shape: GlassRadii.shape(tokens.radii.md),
                  ),
                  child: Padding(
                    padding: const EdgeInsets.symmetric(horizontal: GlassSpacing.s12),
                    child: Row(
                      children: [
                        Text(
                          _sectionLabel(section, l),
                          style: (active ? tokens.typography.bodyEmph : tokens.typography.body).copyWith(
                            color: active ? tokens.palette.accent : tokens.palette.label,
                          ),
                        ),
                        if (errors > 0 && section != DbDesignerSection.ddl) ...[
                          const SizedBox(width: GlassSpacing.s6),
                          GlassBadge(label: '$errors', tone: GlassTone.danger, dense: true),
                        ],
                      ],
                    ),
                  ),
                );
              },
            ),
          ),
      ],
    ),
  );

  // Columns ------------------------------------------------------------------------

  static const _wName = 180.0, _wType = 190.0, _wLen = 64.0, _wDefault = 200.0, _wIdentity = 170.0, _wComment = 220.0;

  double get _columnsWidth =>
      28 +
      10 +
      _wName +
      _wType +
      (dialect.collection ? 0 : _wLen * 2) +
      44 +
      (dialect.collection ? 0 : _wDefault) +
      (dialect.identityKinds.isEmpty ? 0 : _wIdentity) +
      (dialect.has('primary_key') ? 44 : 0) +
      (dialect.has('unique') ? 44 : 0) +
      (dialect.has('unsigned') ? 44 : 0) +
      (dialect.has('arrays') ? 44 : 0) +
      (dialect.has('low_cardinality') ? 44 : 0) +
      (dialect.has('column_comments') ? _wComment : 0) +
      120;

  Widget _columnsHeader(AppLocalizations l) => Padding(
    padding: const EdgeInsets.symmetric(horizontal: GlassSpacing.s8, vertical: GlassSpacing.s6),
    child: Row(
      children: [
        const SizedBox(width: 38),
        _caption(l.dbDesignColName, width: _wName),
        _caption(dialect.collection ? l.dbDesignBsonType : l.dbDesignColType, width: _wType),
        if (!dialect.collection) ...[
          _caption(l.dbDesignColLength, width: _wLen),
          _caption(l.dbDesignColScale, width: _wLen),
        ],
        _caption(
          dialect.collection
              ? l.dbDesignRequired
              : dialect.has('nullable_wrapper')
              ? l.dbDesignNullable
              : l.dbDesignNotNull,
          width: 44,
        ),
        if (!dialect.collection) _caption(l.dbDesignColDefault, width: _wDefault),
        if (dialect.identityKinds.isNotEmpty) _caption(l.dbDesignColIdentity, width: _wIdentity),
        if (dialect.has('primary_key')) _caption(l.dbDesignPk, width: 44),
        if (dialect.has('unique')) _caption(l.dbDesignUnique, width: 44),
        if (dialect.has('unsigned')) _caption(l.dbDesignUnsigned, width: 44),
        if (dialect.has('arrays')) _caption(l.dbDesignArray, width: 44),
        if (dialect.has('low_cardinality')) _caption(l.dbDesignLowCardinality, width: 44),
        if (dialect.has('column_comments'))
          _caption(dialect.collection ? l.dbDesignDescription : l.dbDesignComment, width: _wComment),
      ],
    ),
  );

  Widget _typeCell(int i, DbDesignColumn column, AppLocalizations l) {
    final key = 'db-design-col-$i-type';
    return SizedBox(
      width: _wType,
      child: Row(
        children: [
          Expanded(
            child: _TextCell(
              key: ValueKey(key),
              value: column.dataType,
              placeholder: l.dbDesignColType,
              mono: true,
              onChanged: (v) => d.updateColumn(i, (c) => c.dataType = v, field: 'type'),
            ),
          ),
          Builder(
            builder: (context) => _icon('$key-pick', Icons.arrow_drop_down_rounded, l.dbDesignPickType, () async {
              final anchor = glassAnchorRect(context);
              final query = column.dataType.trim().toLowerCase();
              final exact = dialect.type(query) != null;
              final types = [
                for (final t in dialect.types)
                  if (exact || query.isEmpty || t.name.toLowerCase().contains(query)) t,
              ];
              final picked = await showGlassMenu<String>(
                context: context,
                anchor: anchor,
                minWidth: 200,
                entries: [
                  for (final t in types.isEmpty ? dialect.types : types)
                    GlassMenuItem(
                      key: ValueKey('$key-${t.name}'),
                      value: t.name,
                      label: t.name,
                      checked: t.name == column.dataType,
                    ),
                ],
              );
              if (picked != null) d.updateColumn(i, (c) => c.dataType = picked);
            }),
          ),
        ],
      ),
    );
  }

  Widget _defaultCell(int i, DbDesignColumn column, AppLocalizations l) {
    final key = 'db-design-col-$i-default';
    return SizedBox(
      width: _wDefault,
      child: Row(
        children: [
          Expanded(
            child: _TextCell(
              key: ValueKey(key),
              value: column.defaultValue,
              placeholder: l.dbDesignColDefault,
              mono: true,
              enabled: column.identity.isEmpty,
              onChanged: (v) => d.updateColumn(i, (c) => c.defaultValue = v, field: 'default'),
            ),
          ),
          if (dialect.defaultPresets.isNotEmpty)
            Builder(
              builder: (context) => _icon(
                '$key-presets',
                Icons.auto_fix_high_outlined,
                l.dbDesignDefaultPresets,
                column.identity.isNotEmpty
                    ? null
                    : () async {
                        final picked = await showGlassMenu<String>(
                          context: context,
                          anchor: glassAnchorRect(context),
                          entries: [
                            for (final preset in dialect.defaultPresets)
                              GlassMenuItem(key: ValueKey('$key-$preset'), value: preset, label: preset),
                            if (column.defaultValue.isNotEmpty) ...[
                              const GlassMenuDivider(),
                              GlassMenuItem(value: '', label: l.dbDesignNoDefault),
                            ],
                          ],
                        );
                        if (picked != null) d.updateColumn(i, (c) => c.defaultValue = picked);
                      },
              ),
            ),
        ],
      ),
    );
  }

  Widget _columnRow(int i, AppLocalizations l, GlassTokens tokens) {
    final column = d.design.columns[i];
    final ch = dialect.has('nullable_wrapper');
    return Container(
      key: ValueKey('db-design-col-row-$i'),
      height: 36,
      padding: const EdgeInsets.symmetric(horizontal: GlassSpacing.s8),
      decoration: BoxDecoration(
        border: Border(bottom: BorderSide(color: tokens.surfaces.separator)),
      ),
      child: Row(
        children: [
          ReorderableDragStartListener(
            index: i,
            child: MouseRegion(
              cursor: SystemMouseCursors.grab,
              child: SizedBox(
                width: 24,
                child: Icon(Icons.drag_indicator_rounded, size: 16, color: tokens.secondaryLabel),
              ),
            ),
          ),
          _marker(d.columnState(i), error: _rowHasError('columns.$i')),
          SizedBox(
            width: _wName,
            child: Padding(
              padding: const EdgeInsets.only(right: GlassSpacing.s6),
              child: _TextCell(
                key: ValueKey('db-design-col-$i-name'),
                value: column.name,
                placeholder: l.dbDesignColName,
                onChanged: (v) => d.renameColumn(i, v),
              ),
            ),
          ),
          Padding(
            padding: const EdgeInsets.only(right: GlassSpacing.s6),
            child: _typeCell(i, column, l),
          ),
          if (!dialect.collection) ...[
            SizedBox(
              width: _wLen,
              child: Padding(
                padding: const EdgeInsets.only(right: GlassSpacing.s6),
                child: _TextCell(
                  key: ValueKey('db-design-col-$i-length'),
                  value: column.length?.toString() ?? '',
                  digits: true,
                  onChanged: (v) => d.updateColumn(i, (c) => c.length = int.tryParse(v), field: 'length'),
                ),
              ),
            ),
            SizedBox(
              width: _wLen,
              child: Padding(
                padding: const EdgeInsets.only(right: GlassSpacing.s6),
                child: _TextCell(
                  key: ValueKey('db-design-col-$i-scale'),
                  value: column.scale?.toString() ?? '',
                  digits: true,
                  onChanged: (v) => d.updateColumn(i, (c) => c.scale = int.tryParse(v), field: 'scale'),
                ),
              ),
            ),
          ],
          _check(
            'db-design-col-$i-null',
            ch ? column.nullable : !column.nullable,
            dialect.collection
                ? l.dbDesignRequired
                : ch
                ? l.dbDesignNullable
                : l.dbDesignNotNull,
            (v) => d.updateColumn(i, (c) => c.nullable = ch ? v : !v),
          ),
          if (!dialect.collection)
            Padding(
              padding: const EdgeInsets.only(right: GlassSpacing.s6),
              child: _defaultCell(i, column, l),
            ),
          if (dialect.identityKinds.isNotEmpty)
            Padding(
              padding: const EdgeInsets.only(right: GlassSpacing.s6),
              child: _select<String>(
                'db-design-col-$i-identity',
                column.identity,
                [
                  for (final kind in ['', ...dialect.identityKinds])
                    GlassSelectItem(value: kind, label: _identityLabel(kind, l)),
                ],
                (v) => d.updateColumn(i, (c) {
                  c.identity = v;
                  if (v.isNotEmpty) {
                    c
                      ..defaultValue = ''
                      ..nullable = false;
                  }
                }),
                width: _wIdentity - GlassSpacing.s6,
              ),
            ),
          if (dialect.has('primary_key'))
            _check('db-design-col-$i-pk', d.isPrimary(column.name), l.dbDesignPk, (_) => d.togglePrimary(column.name)),
          if (dialect.has('unique'))
            _check(
              'db-design-col-$i-unique',
              d.isUnique(column.name),
              l.dbDesignUnique,
              (_) => d.toggleUnique(column.name),
            ),
          if (dialect.has('unsigned'))
            _check(
              'db-design-col-$i-unsigned',
              column.unsigned,
              l.dbDesignUnsigned,
              (v) => d.updateColumn(i, (c) => c.unsigned = v),
            ),
          if (dialect.has('arrays'))
            _check(
              'db-design-col-$i-array',
              column.array,
              l.dbDesignArray,
              (v) => d.updateColumn(i, (c) => c.array = v),
            ),
          if (dialect.has('low_cardinality'))
            _check(
              'db-design-col-$i-lc',
              column.lowCardinality,
              l.dbDesignLowCardinality,
              (v) => d.updateColumn(i, (c) => c.lowCardinality = v),
            ),
          if (dialect.has('column_comments'))
            SizedBox(
              width: _wComment,
              child: Padding(
                padding: const EdgeInsets.only(right: GlassSpacing.s6),
                child: _TextCell(
                  key: ValueKey('db-design-col-$i-comment'),
                  value: column.comment,
                  placeholder: dialect.collection ? l.dbDesignDescription : l.dbDesignComment,
                  onChanged: (v) => d.updateColumn(i, (c) => c.comment = v, field: 'comment'),
                ),
              ),
            ),
          GlassMenuButton<String>(
            key: ValueKey('db-design-col-$i-menu'),
            entries: [
              GlassMenuItem(value: 'duplicate', label: l.dbDesignDuplicate, icon: Icons.copy_all_outlined),
              GlassMenuItem(value: 'up', label: l.dbDesignMoveUp, icon: Icons.arrow_upward_rounded, enabled: i > 0),
              GlassMenuItem(
                value: 'down',
                label: l.dbDesignMoveDown,
                icon: Icons.arrow_downward_rounded,
                enabled: i < d.design.columns.length - 1,
              ),
              const GlassMenuDivider(),
              GlassMenuItem(
                key: ValueKey('db-design-col-$i-remove'),
                value: 'remove',
                label: l.dbDesignRemove,
                icon: Icons.delete_outline_rounded,
                destructive: true,
              ),
            ],
            onSelected: (value) => switch (value) {
              'duplicate' => d.duplicateColumn(i),
              'up' => d.moveColumn(i, i - 1),
              'down' => d.moveColumn(i, i + 1),
              _ => d.removeColumn(i),
            },
            builder: (context, open) =>
                _icon('db-design-col-$i-more', Icons.more_horiz_rounded, l.dbDesignColumnActions, open),
          ),
        ],
      ),
    );
  }

  Widget _columns(AppLocalizations l, GlassTokens tokens) {
    final dropped = d.droppedColumns;
    return LayoutBuilder(
      builder: (context, bounds) => SingleChildScrollView(
        scrollDirection: Axis.horizontal,
        child: SizedBox(
          width: math.max(_columnsWidth, bounds.maxWidth),
          height: bounds.maxHeight,
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              _columnsHeader(l),
              Container(height: 1, color: tokens.surfaces.separator),
              Expanded(
                child: ReorderableListView.builder(
                  key: const ValueKey('db-design-columns'),
                  buildDefaultDragHandles: false,
                  itemCount: d.design.columns.length,
                  onReorderItem: d.moveColumn,
                  itemBuilder: (context, i) =>
                      KeyedSubtree(key: ValueKey('db-design-col-item-$i'), child: _columnRow(i, l, tokens)),
                  footer: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      for (final name in dropped)
                        Container(
                          key: ValueKey('db-design-dropped-$name'),
                          height: 32,
                          padding: const EdgeInsets.only(left: 38),
                          child: Row(
                            children: [
                              Icon(Icons.remove_circle_outline_rounded, size: 14, color: tokens.palette.danger),
                              const SizedBox(width: GlassSpacing.s6),
                              Text(
                                name,
                                style: tokens.typography.body.copyWith(
                                  color: tokens.palette.danger,
                                  decoration: TextDecoration.lineThrough,
                                ),
                              ),
                              const SizedBox(width: GlassSpacing.s8),
                              Text(
                                l.dbDesignWillDrop,
                                style: tokens.typography.caption.copyWith(color: tokens.secondaryLabel),
                              ),
                              const SizedBox(width: GlassSpacing.s8),
                              GlassButton.plain(label: l.dbDesignRestore, onPressed: () => d.restoreColumn(name)),
                            ],
                          ),
                        ),
                      Padding(
                        padding: const EdgeInsets.all(GlassSpacing.s8),
                        child: GlassButton(
                          key: const ValueKey('db-design-add-column'),
                          label: dialect.collection ? l.dbDesignAddField : l.dbDesignAddColumn,
                          icon: Icons.add_rounded,
                          size: GlassControlSize.sm,
                          onPressed: d.addColumn,
                        ),
                      ),
                    ],
                  ),
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }

  // Keys, foreign keys, indexes, checks ------------------------------------------------

  Widget _listSection({
    required List<Widget> rows,
    required String addKey,
    required String addLabel,
    required VoidCallback onAdd,
    Widget? lead,
  }) => ListView(
    padding: const EdgeInsets.all(GlassSpacing.s12),
    children: [
      ?lead,
      ...rows,
      Align(
        alignment: Alignment.centerLeft,
        child: Padding(
          padding: const EdgeInsets.only(top: GlassSpacing.s8),
          child: GlassButton(
            key: ValueKey(addKey),
            label: addLabel,
            icon: Icons.add_rounded,
            size: GlassControlSize.sm,
            onPressed: onAdd,
          ),
        ),
      ),
    ],
  );

  Widget _itemRow(String prefix, List<Widget> children, {VoidCallback? onRemove, DbDraftState? state}) => Padding(
    key: ValueKey('db-design-row-$prefix'),
    padding: const EdgeInsets.only(bottom: GlassSpacing.s6),
    child: Wrap(
      spacing: GlassSpacing.s8,
      runSpacing: GlassSpacing.s6,
      crossAxisAlignment: WrapCrossAlignment.center,
      children: [
        _marker(state, error: _rowHasError(prefix)),
        ...children,
        if (onRemove != null)
          _icon('db-design-remove-$prefix', Icons.delete_outline_rounded, context.l10n.dbDesignRemove, onRemove),
      ],
    ),
  );

  DbDraftState? _itemState(String? original) => !d.alter || original == null ? DbDraftState.added : null;

  Widget _keys(AppLocalizations l) {
    final design = d.design;
    return _listSection(
      lead: dialect.has('primary_key')
          ? Padding(
              padding: const EdgeInsets.only(bottom: GlassSpacing.s12),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  _caption(l.dbDesignPrimaryKey),
                  const SizedBox(height: GlassSpacing.s6),
                  _itemRow('primary_key', [
                    if (!dialect.has('nullable_wrapper'))
                      _TextCell(
                        key: const ValueKey('db-design-pk-name'),
                        value: design.primaryKey.name,
                        placeholder: l.dbDesignAutoName,
                        width: 200,
                        onChanged: (v) => d.edit((x) => x.primaryKey.name = v, coalesce: 'pk.name'),
                      ),
                    _columnsPicker(
                      'db-design-pk-columns',
                      design.primaryKey.columns,
                      _columnNames,
                      (v) => d.edit((x) => x.primaryKey.columns = v),
                    ),
                  ]),
                  if (dialect.has('unique')) ...[
                    const SizedBox(height: GlassSpacing.s12),
                    _caption(l.dbDesignUniqueKeys),
                  ],
                ],
              ),
            )
          : null,
      rows: [
        if (dialect.has('unique'))
          for (var i = 0; i < design.uniques.length; i++)
            _itemRow(
              'uniques.$i',
              [
                _TextCell(
                  key: ValueKey('db-design-unique-$i-name'),
                  value: design.uniques[i].name,
                  placeholder: l.dbDesignAutoName,
                  width: 200,
                  onChanged: (v) => d.edit((x) => x.uniques[i].name = v, coalesce: 'unique.$i'),
                ),
                _columnsPicker(
                  'db-design-unique-$i-columns',
                  design.uniques[i].columns,
                  _columnNames,
                  (v) => d.edit((x) => x.uniques[i].columns = v),
                ),
              ],
              state: _itemState(design.uniques[i].original),
              onRemove: () => d.edit((x) => x.uniques.removeAt(i)),
            ),
      ],
      addKey: 'db-design-add-unique',
      addLabel: l.dbDesignAddUnique,
      onAdd: () => d.edit((x) => x.uniques.add(DbDesignKey())),
    );
  }

  DbObjectRef _containerRef(String? schemaOrDb) {
    final design = d.design;
    return d.engine == DbEngine.postgresql
        ? DbObjectRef(database: '', kind: DbObjectKind.schema, name: schemaOrDb ?? design.schema ?? 'public')
        : DbObjectRef(
            database: schemaOrDb ?? design.database,
            kind: DbObjectKind.database,
            name: schemaOrDb ?? design.database,
          );
  }

  Future<List<String>> _tables(String? schema) async {
    try {
      final page = await widget.queryService.tree(widget.session.id, parent: _containerRef(schema));
      return [
        for (final node in page.nodes)
          if (node.reference.kind == DbObjectKind.table) node.reference.name,
      ];
    } catch (_) {
      return const [];
    }
  }

  Future<List<String>> _refColumns(String? schema, String table) async {
    if (table.isEmpty) return const [];
    if (table == d.design.name && (schema == null || schema.isEmpty)) return _columnNames;
    try {
      final container = _containerRef(schema);
      final page = await widget.queryService.tree(
        widget.session.id,
        parent: DbObjectRef(
          database: d.engine == DbEngine.postgresql ? '' : container.name,
          schema: d.engine == DbEngine.postgresql ? container.name : null,
          kind: DbObjectKind.table,
          name: table,
        ),
      );
      return [
        for (final node in page.nodes)
          if (node.reference.kind == DbObjectKind.column) node.reference.name,
      ];
    } catch (_) {
      return const [];
    }
  }

  Widget _foreignKeys(AppLocalizations l) {
    final design = d.design;
    List<GlassSelectItem<String>> actions() => [
      GlassSelectItem(value: '', label: l.dbDesignDefault),
      for (final action in dialect.fkActions) GlassSelectItem(value: action, label: action),
    ];
    return _listSection(
      rows: [
        for (var i = 0; i < design.foreignKeys.length; i++)
          Builder(
            builder: (context) {
              final fk = design.foreignKeys[i];
              return _itemRow(
                'foreign_keys.$i',
                [
                  _TextCell(
                    key: ValueKey('db-design-fk-$i-name'),
                    value: fk.name,
                    placeholder: l.dbDesignAutoName,
                    width: 180,
                    onChanged: (v) => d.edit((x) => x.foreignKeys[i].name = v, coalesce: 'fk.$i.name'),
                  ),
                  _columnsPicker(
                    'db-design-fk-$i-columns',
                    fk.columns,
                    _columnNames,
                    (v) => d.edit((x) => x.foreignKeys[i].columns = v),
                    width: 180,
                  ),
                  const Icon(Icons.arrow_forward_rounded, size: 16),
                  _TextCell(
                    key: ValueKey('db-design-fk-$i-schema'),
                    value: fk.refSchema ?? '',
                    placeholder: dialect.schemas ? l.dbDesignSchema : l.dbDesignDatabase,
                    width: 120,
                    onChanged: (v) =>
                        d.edit((x) => x.foreignKeys[i].refSchema = v.isEmpty ? null : v, coalesce: 'fk.$i.schema'),
                  ),
                  SizedBox(
                    width: 210,
                    child: Row(
                      children: [
                        Expanded(
                          child: _TextCell(
                            key: ValueKey('db-design-fk-$i-table'),
                            value: fk.refTable,
                            placeholder: l.dbDesignRefTable,
                            onChanged: (v) => d.edit((x) => x.foreignKeys[i].refTable = v, coalesce: 'fk.$i.table'),
                          ),
                        ),
                        Builder(
                          builder: (context) => _icon(
                            'db-design-fk-$i-table-pick',
                            Icons.arrow_drop_down_rounded,
                            l.dbDesignPickTable,
                            () async {
                              final anchor = glassAnchorRect(context);
                              final tables = await _tables(fk.refSchema);
                              if (!mounted || !context.mounted || tables.isEmpty) return;
                              final picked = await showGlassMenu<String>(
                                context: context,
                                anchor: anchor,
                                entries: [
                                  for (final t in tables) GlassMenuItem(value: t, label: t, checked: t == fk.refTable),
                                ],
                              );
                              if (picked != null) {
                                d.edit(
                                  (x) => x.foreignKeys[i]
                                    ..refTable = picked
                                    ..refColumns = [],
                                );
                              }
                            },
                          ),
                        ),
                      ],
                    ),
                  ),
                  _columnsPicker(
                    'db-design-fk-$i-ref-columns',
                    fk.refColumns,
                    const [],
                    (v) => d.edit((x) => x.foreignKeys[i].refColumns = v),
                    width: 180,
                    load: () => _refColumns(fk.refSchema, fk.refTable),
                  ),
                  _caption(l.dbDesignOnDelete),
                  _select<String>(
                    'db-design-fk-$i-delete',
                    fk.onDelete,
                    actions(),
                    (v) => d.edit((x) => x.foreignKeys[i].onDelete = v),
                    width: 140,
                  ),
                  _caption(l.dbDesignOnUpdate),
                  _select<String>(
                    'db-design-fk-$i-update',
                    fk.onUpdate,
                    actions(),
                    (v) => d.edit((x) => x.foreignKeys[i].onUpdate = v),
                    width: 140,
                  ),
                ],
                state: _itemState(fk.original),
                onRemove: () => d.edit((x) => x.foreignKeys.removeAt(i)),
              );
            },
          ),
      ],
      addKey: 'db-design-add-fk',
      addLabel: l.dbDesignAddForeignKey,
      onAdd: () => d.edit((x) => x.foreignKeys.add(DbDesignForeignKey())),
    );
  }

  Widget _indexes(AppLocalizations l) {
    final design = d.design;
    return _listSection(
      rows: [
        for (var i = 0; i < design.indexes.length; i++)
          _itemRow(
            'indexes.$i',
            [
              _TextCell(
                key: ValueKey('db-design-index-$i-name'),
                value: design.indexes[i].name,
                placeholder: l.dbDesignAutoName,
                width: 200,
                onChanged: (v) => d.edit((x) => x.indexes[i].name = v, coalesce: 'index.$i'),
              ),
              _columnsPicker(
                'db-design-index-$i-columns',
                design.indexes[i].columns,
                _columnNames,
                (v) => d.edit((x) => x.indexes[i].columns = v),
              ),
              if (!dialect.collection || design.indexes[i].method != 'text')
                Row(
                  mainAxisSize: MainAxisSize.min,
                  children: [
                    _check(
                      'db-design-index-$i-unique',
                      design.indexes[i].unique,
                      l.dbDesignUnique,
                      (v) => d.edit((x) => x.indexes[i].unique = v),
                    ),
                    _caption(l.dbDesignUnique),
                  ],
                ),
              if (dialect.indexMethods.isNotEmpty)
                _select<String>(
                  'db-design-index-$i-method',
                  design.indexes[i].method,
                  [
                    GlassSelectItem(value: '', label: l.dbDesignDefault),
                    for (final method in dialect.indexMethods) GlassSelectItem(value: method, label: method),
                  ],
                  (v) => d.edit((x) => x.indexes[i].method = v),
                  width: 150,
                ),
            ],
            state: _itemState(design.indexes[i].original),
            onRemove: () => d.edit((x) => x.indexes.removeAt(i)),
          ),
      ],
      addKey: 'db-design-add-index',
      addLabel: l.dbDesignAddIndex,
      onAdd: () => d.edit((x) => x.indexes.add(DbDesignIndex())),
    );
  }

  Widget _checks(AppLocalizations l) {
    final design = d.design;
    return _listSection(
      rows: [
        for (var i = 0; i < design.checks.length; i++)
          _itemRow(
            'checks.$i',
            [
              _TextCell(
                key: ValueKey('db-design-check-$i-name'),
                value: design.checks[i].name,
                placeholder: l.dbDesignAutoName,
                width: 200,
                onChanged: (v) => d.edit((x) => x.checks[i].name = v, coalesce: 'check.$i.name'),
              ),
              _TextCell(
                key: ValueKey('db-design-check-$i-expression'),
                value: design.checks[i].expression,
                placeholder: l.dbDesignCheckExpression,
                width: 420,
                mono: true,
                onChanged: (v) => d.edit((x) => x.checks[i].expression = v, coalesce: 'check.$i.expr'),
              ),
            ],
            state: _itemState(design.checks[i].original),
            onRemove: () => d.edit((x) => x.checks.removeAt(i)),
          ),
      ],
      addKey: 'db-design-add-check',
      addLabel: l.dbDesignAddCheck,
      onAdd: () => d.edit((x) => x.checks.add(DbDesignCheck())),
    );
  }

  Widget _options(AppLocalizations l, GlassTokens tokens) {
    final design = d.design;
    return ListView(
      padding: const EdgeInsets.all(GlassSpacing.s12),
      children: [
        for (final option in dialect.options)
          Padding(
            key: ValueKey('db-design-option-row-${option.id}'),
            padding: const EdgeInsets.only(bottom: GlassSpacing.s8),
            child: Row(
              children: [
                _marker(null, error: _rowHasError('options.${option.id}')),
                _caption(dbDesignOptionLabel(option.id, l), width: 200),
                Builder(
                  builder: (context) {
                    final value = design.options[option.id] ?? option.defaultValue;
                    final enabled = !(d.alter && option.createOnly);
                    void set(String v) => d.edit(
                      (x) => v.isEmpty ? x.options.remove(option.id) : x.options[option.id] = v,
                      coalesce: 'option.${option.id}',
                    );
                    return switch (option.kind) {
                      DbDesignOptionKind.boolean => _check(
                        'db-design-option-${option.id}',
                        value == 'true',
                        dbDesignOptionLabel(option.id, l),
                        enabled ? (v) => set(v ? 'true' : '') : null,
                      ),
                      DbDesignOptionKind.choice => _select<String>(
                        'db-design-option-${option.id}',
                        option.choices.contains(value) ? value : '',
                        [
                          for (final choice in {'', ...option.choices})
                            GlassSelectItem(value: choice, label: choice.isEmpty ? l.dbDesignDefault : choice),
                        ],
                        enabled ? set : null,
                        width: 260,
                      ),
                      _ => _TextCell(
                        key: ValueKey('db-design-option-${option.id}'),
                        value: value,
                        width: 360,
                        mono: true,
                        digits: option.kind == DbDesignOptionKind.number,
                        enabled: enabled,
                        onChanged: set,
                      ),
                    };
                  },
                ),
                if (d.alter && option.createOnly) ...[
                  const SizedBox(width: GlassSpacing.s8),
                  Text(l.dbDesignCreateOnly, style: tokens.typography.caption.copyWith(color: tokens.palette.tertiary)),
                ],
              ],
            ),
          ),
        if (d.engine == DbEngine.clickhouse)
          Padding(
            padding: const EdgeInsets.only(top: GlassSpacing.s8),
            child: Text(
              l.dbDesignClickhouseHint,
              style: tokens.typography.caption.copyWith(color: tokens.secondaryLabel),
            ),
          ),
      ],
    );
  }

  Widget _ddl(AppLocalizations l, GlassTokens tokens) {
    final plan = d.plan;
    final blocks = [for (final s in plan.statements) d.dialect.collection ? s.sql : '${s.sql};'];
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Padding(
          padding: const EdgeInsets.fromLTRB(GlassSpacing.s12, GlassSpacing.s8, GlassSpacing.s8, 0),
          child: Row(
            children: [
              Expanded(
                child: Text(
                  plan.statements.isEmpty
                      ? (plan.hasErrors ? l.dbDesignFixErrors : l.dbDesignNoChanges)
                      : plan.transactional
                      ? l.dbDesignerTransactional
                      : l.dbDesignerNotTransactional,
                  style: tokens.typography.caption.copyWith(color: tokens.secondaryLabel),
                ),
              ),
              const DbWrapToggle(),
              _icon(
                'db-design-ddl-copy',
                Icons.content_copy_rounded,
                l.commonCopy,
                plan.script.isEmpty ? null : () => copyPlainWithNotice(context, ref, plan.script, what: l.copyWhatDdl),
              ),
              _icon(
                'db-design-ddl-open',
                Icons.edit_note_rounded,
                l.dbOpenInEditor,
                plan.script.isEmpty ? null : () => widget.onOpenInEditor(plan.script),
              ),
            ],
          ),
        ),
        Expanded(
          child: Padding(
            padding: const EdgeInsets.symmetric(vertical: GlassSpacing.s8),
            child: blocks.isEmpty
                ? const SizedBox.shrink()
                : DbCodeView(key: const ValueKey('db-design-ddl'), blocks: blocks, wrap: dbCodeWrap(ref)),
          ),
        ),
      ],
    );
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n, tokens = GlassTokens.of(context);
    if (!_sections.contains(_section)) _section = DbDesignerSection.columns;
    final body = switch (_section) {
      DbDesignerSection.columns => _columns(l, tokens),
      DbDesignerSection.keys => _keys(l),
      DbDesignerSection.foreignKeys => _foreignKeys(l),
      DbDesignerSection.indexes => _indexes(l),
      DbDesignerSection.checks => _checks(l),
      DbDesignerSection.options => _options(l, tokens),
      DbDesignerSection.ddl => _ddl(l, tokens),
    };
    void save() {
      if (d.canSave) widget.onSave();
    }

    final issues = _section == DbDesignerSection.ddl ? d.plan.issues : [..._issues(_section), ...d.issuesFor('table')];
    return CallbackShortcuts(
      bindings: {
        const SingleActivator(LogicalKeyboardKey.keyS, meta: true): save,
        const SingleActivator(LogicalKeyboardKey.keyS, control: true): save,
        const SingleActivator(LogicalKeyboardKey.keyZ, meta: true): d.undo,
      },
      child: Focus(
        autofocus: true,
        child: Column(
          key: ValueKey('db-designer-${d.id}'),
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            _header(l, tokens),
            const SizedBox(height: GlassSpacing.s8),
            Expanded(
              child: ContentSurface(
                kind: ContentSurfaceKind.solid,
                padding: EdgeInsets.zero,
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.stretch,
                  children: [
                    Padding(
                      padding: const EdgeInsets.fromLTRB(GlassSpacing.s8, GlassSpacing.s6, GlassSpacing.s8, 0),
                      child: _sectionBar(l, tokens),
                    ),
                    _issueList(issues),
                    if (d.planFailed)
                      Padding(
                        padding: const EdgeInsets.fromLTRB(GlassSpacing.s12, GlassSpacing.s6, GlassSpacing.s12, 0),
                        child: InfoBanner(tone: BannerTone.danger, message: l.dbOperationFailed),
                      ),
                    const SizedBox(height: GlassSpacing.s6),
                    Container(height: 1, color: tokens.surfaces.separator),
                    Expanded(child: body),
                  ],
                ),
              ),
            ),
          ],
        ),
      ),
    );
  }
}

/// A compact text field bound to a value from the draft: it follows the
/// value when it changes from outside (undo, revert) and reports typing.
class _TextCell extends StatefulWidget {
  const _TextCell({
    required this.value,
    required this.onChanged,
    super.key,
    this.placeholder,
    this.width,
    this.mono = false,
    this.digits = false,
    this.enabled = true,
  });
  final String value;
  final ValueChanged<String> onChanged;
  final String? placeholder;
  final double? width;
  final bool mono, digits, enabled;
  @override
  State<_TextCell> createState() => _TextCellState();
}

class _TextCellState extends State<_TextCell> {
  late final _controller = TextEditingController(text: widget.value);

  @override
  void didUpdateWidget(_TextCell old) {
    super.didUpdateWidget(old);
    if (widget.value != _controller.text) {
      _controller.value = TextEditingValue(
        text: widget.value,
        selection: TextSelection.collapsed(offset: widget.value.length),
      );
    }
  }

  @override
  void dispose() {
    _controller.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final field = GlassField(
      controller: _controller,
      fieldKey: widget.key is ValueKey<String> ? ValueKey('${(widget.key! as ValueKey<String>).value}-input') : null,
      placeholder: widget.placeholder,
      size: GlassFieldSize.md,
      mono: widget.mono,
      enabled: widget.enabled,
      keyboardType: widget.digits ? TextInputType.number : null,
      inputFormatters: widget.digits ? [FilteringTextInputFormatter.digitsOnly] : null,
      semanticLabel: widget.placeholder,
      onChanged: widget.onChanged,
    );
    return widget.width == null ? field : SizedBox(width: widget.width, child: field);
  }
}
