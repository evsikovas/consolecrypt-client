import 'dart:async';
import 'dart:io';
import 'dart:math' as math;

import 'package:consolecrypt/app/platform.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/db/db_ddl_dialog.dart';
import 'package:consolecrypt/db/db_models.dart';
import 'package:consolecrypt/db/db_providers.dart';
import 'package:consolecrypt/db/db_query_dialogs.dart';
import 'package:consolecrypt/db/db_query_service.dart';
import 'package:consolecrypt/db/db_result_edits.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';
import 'package:url_launcher/url_launcher.dart';

/// Largest file the import reads (the core checks it again).
const dbCsvMaxBytes = 200 * 1024 * 1024;

enum DbCsvEncoding { auto, utf8, windows1251, latin1 }

enum DbCsvNull { empty, word, backslash, never }

enum DbCsvDateOrder { iso, dmy, mdy }

enum DbCsvMode { insert, skipExisting, upsert }

String _snake(String name) => name.replaceAllMapped(RegExp('[A-Z]'), (m) => '_${m[0]!.toLowerCase()}');

/// How a file is split and read.
final class DbCsvOptions {
  const DbCsvOptions({
    this.encoding = DbCsvEncoding.auto,
    this.delimiter,
    this.quote = '"',
    this.header = true,
    this.skipRows = 0,
    this.nullMarker = DbCsvNull.empty,
    this.decimalComma = false,
    this.dateOrder = DbCsvDateOrder.iso,
  });
  final DbCsvEncoding encoding;

  /// Null detects `,`, `;`, TAB or `|`.
  final String? delimiter;
  final String quote;
  final bool header;
  final int skipRows;
  final DbCsvNull nullMarker;
  final bool decimalComma;
  final DbCsvDateOrder dateOrder;

  DbCsvOptions copyWith({
    DbCsvEncoding? encoding,
    String? delimiter,
    bool autoDelimiter = false,
    String? quote,
    bool? header,
    int? skipRows,
    DbCsvNull? nullMarker,
    bool? decimalComma,
    DbCsvDateOrder? dateOrder,
  }) => DbCsvOptions(
    encoding: encoding ?? this.encoding,
    delimiter: autoDelimiter ? null : (delimiter ?? this.delimiter),
    quote: quote ?? this.quote,
    header: header ?? this.header,
    skipRows: skipRows ?? this.skipRows,
    nullMarker: nullMarker ?? this.nullMarker,
    decimalComma: decimalComma ?? this.decimalComma,
    dateOrder: dateOrder ?? this.dateOrder,
  );

  DbJson toJson() => {
    'encoding': _snake(encoding.name),
    'delimiter': delimiter,
    'quote': quote,
    'header': header,
    'skip_rows': skipRows,
    'null': nullMarker.name,
    'decimal_comma': decimalComma,
    'date_order': dateOrder.name,
  };
}

/// The first rows of a file as the options split them.
final class DbCsvPreview {
  const DbCsvPreview({
    required this.delimiter,
    required this.encoding,
    required this.columns,
    required this.rows,
    this.fileBytes = 0,
  });
  final String delimiter;
  final DbCsvEncoding encoding;
  final List<String> columns;

  /// Null is NULL.
  final List<List<String?>> rows;
  final int fileBytes;

  factory DbCsvPreview.fromJson(DbJson json) => DbCsvPreview(
    delimiter: json['delimiter'] as String,
    encoding: DbCsvEncoding.values.firstWhere(
      (e) => _snake(e.name) == json['encoding'],
      orElse: () => DbCsvEncoding.utf8,
    ),
    columns: (json['columns'] as List).cast<String>(),
    rows: [for (final row in (json['rows'] as List)) (row as List).cast<String?>()],
    fileBytes: json['file_bytes'] as int? ?? 0,
  );
}

final class DbCsvMapping {
  const DbCsvMapping({required this.source, required this.target});
  final int source;
  final String target;
  DbJson toJson() => {'source': source, 'target': target};
}

/// Values of a target column that do not convert (from the first rows).
final class DbCsvCheck {
  const DbCsvCheck({
    required this.target,
    this.kind = '',
    this.bad = 0,
    this.samples = const [],
    this.missingRequired = false,
  });
  final String target, kind;
  final int bad;
  final List<String> samples;
  final bool missingRequired;
  factory DbCsvCheck.fromJson(DbJson json) => DbCsvCheck(
    target: json['target'] as String,
    kind: json['kind'] as String? ?? '',
    bad: json['bad'] as int? ?? 0,
    samples: (json['samples'] as List? ?? const []).cast<String>(),
    missingRequired: json['missing_required'] as bool? ?? false,
  );
}

final class DbCsvPlan {
  const DbCsvPlan({
    required this.table,
    required this.options,
    required this.mapping,
    this.mode = DbCsvMode.insert,
    this.batchRows = 500,
    this.skipBadRows = false,
    this.perBatch = false,
    this.truncate = false,
  });
  final DbObjectRef table;
  final DbCsvOptions options;
  final List<DbCsvMapping> mapping;
  final DbCsvMode mode;
  final int batchRows;
  final bool skipBadRows, perBatch, truncate;
  DbJson toJson() => {
    'table': table.toJson(),
    'options': options.toJson(),
    'mapping': [for (final m in mapping) m.toJson()],
    'mode': _snake(mode.name),
    'batch_rows': batchRows,
    'skip_bad_rows': skipBadRows,
    'transaction': perBatch ? 'per_batch' : 'whole',
    'truncate': truncate,
  };
}

final class DbCsvReview {
  const DbCsvReview({required this.ticket, required this.sql, required this.risk, required this.expiresAt});
  final String ticket;
  final SecretText sql;
  final DbQueryRisk risk;
  final DateTime expiresAt;
}

final class DbCsvProgress {
  const DbCsvProgress({
    this.bytesRead = 0,
    this.totalBytes = 0,
    this.rowsRead = 0,
    this.imported = 0,
    this.skipped = 0,
    this.failed = 0,
    this.done = false,
    this.cancelled = false,
    this.error,
    this.report,
  });
  final int bytesRead, totalBytes, rowsRead, imported, skipped, failed;
  final bool done, cancelled;
  final String? error, report;
  factory DbCsvProgress.fromJson(DbJson json) => DbCsvProgress(
    bytesRead: json['bytes_read'] as int? ?? 0,
    totalBytes: json['total_bytes'] as int? ?? 0,
    rowsRead: json['rows_read'] as int? ?? 0,
    imported: json['imported'] as int? ?? 0,
    skipped: json['skipped'] as int? ?? 0,
    failed: json['failed'] as int? ?? 0,
    done: json['done'] as bool? ?? false,
    cancelled: json['cancelled'] as bool? ?? false,
    error: json['error'] as String?,
    report: json['report'] as String?,
  );
}

/// CSV import into a table of a live session (PostgreSQL, MySQL, MariaDB).
abstract interface class DbCsvImportService implements DbRowEditService {
  Future<DbCsvPreview> csvPreview(String path, DbCsvOptions options);
  Future<List<DbCsvCheck>> csvCheck(
    String sessionId,
    DbObjectRef table,
    String path,
    DbCsvOptions options,
    List<DbCsvMapping> mapping,
  );
  Future<DbCsvReview> reviewCsvImport(String sessionId, String path, DbCsvPlan plan);
  Future<void> discardCsvImport(String ticket);
  Future<String> startCsvImport(String sessionId, String ticket);
  Stream<DbCsvProgress> watchCsvImport(String importId);
  Future<void> cancelCsvImport(String importId);

  /// A simple `CREATE TABLE` typed from the file (a table designer draft).
  Future<String> csvCreateTable(String path, DbCsvOptions options, DbEngine engine, DbObjectRef table);
}

/// Engines the CSV import writes to.
bool dbCsvImportSupported(DbEngine engine) => const {
  DbEngine.postgresql,
  DbEngine.mysql,
  DbEngine.mariadb,
  DbEngine.sqlite,
  DbEngine.cassandra,
  DbEngine.clickhouse,
  DbEngine.mongodb,
  DbEngine.redis,
}.contains(engine);

/// A Redis hash field a CSV column may create: one word, no quotes,
/// backslashes or placeholder braces; `key` and `value` are taken.
bool dbCsvRedisField(String name) =>
    name.isNotEmpty &&
    name.length <= 256 &&
    name != 'key' &&
    name != 'value' &&
    !name.contains('{{') &&
    !name.contains('}}') &&
    !name.contains("'") &&
    !name.contains(RegExp(r'[\s"\\\x00-\x1f]'));

/// A MongoDB field a CSV column may create: no `$` in front, no `.` path.
bool dbCsvMongoField(String name) =>
    name.isNotEmpty && name.length <= 256 && !name.startsWith(r'$') && !name.contains('.') && !name.contains('\x00');

/// The import can run in one transaction for the whole file: not in CQL, and
/// not through `sqlite3` on a host, which commits every statement.
bool dbCsvWholeTransaction(DbConnection connection) => switch (connection.engine) {
  DbEngine.cassandra || DbEngine.clickhouse || DbEngine.mongodb || DbEngine.redis => false,
  DbEngine.sqlite => connection.route.kind == DbRouteKind.direct,
  _ => true,
};

String _bytes(AppLocalizations l, int bytes) => bytes >= 1024 * 1024
    ? l.dbSizeMb((bytes / (1024 * 1024)).toStringAsFixed(1))
    : l.dbSizeKb(math.max(1, (bytes / 1024).ceil()));

/// The CSV import wizard: file, reading settings with a live preview,
/// column mapping with a type check, options, then the usual review with
/// the vault passphrase and a run with progress. Closes with true when
/// rows were written.
class DbCsvImportDialog extends ConsumerStatefulWidget {
  const DbCsvImportDialog({
    required this.service,
    required this.session,
    required this.table,
    this.wholeTransaction = true,
    super.key,
  });
  final DbCsvImportService service;
  final DbSessionInfo session;
  final DbObjectRef table;

  /// See [dbCsvWholeTransaction]; when false every batch commits on its own.
  final bool wholeTransaction;
  @override
  ConsumerState<DbCsvImportDialog> createState() => _DbCsvImportDialogState();
}

enum _Step { file, read, map, options, run }

class _DbCsvImportDialogState extends ConsumerState<DbCsvImportDialog> {
  late final _scope = ref.read(dbScopeProvider);
  final _quote = TextEditingController(text: '"');
  final _skip = TextEditingController(text: '0');
  final _batch = TextEditingController(text: '500');
  _Step _step = _Step.file;
  String? _path, _fileError;
  int _size = 0;
  DbCsvOptions _options = const DbCsvOptions();
  DbCsvPreview? _preview;
  bool _previewing = false, _previewFailed = false;
  Timer? _debounce;
  int _generation = 0;
  List<DbEditColumn>? _columns;
  List<String?> _targets = const [];
  List<DbCsvCheck>? _checks;
  bool _checking = false;
  DbCsvMode _mode = DbCsvMode.insert;
  bool _skipBad = false, _truncate = false, _starting = false;
  late bool _perBatch = !widget.wholeTransaction;
  String? _importId;
  DbCsvProgress? _progress;
  StreamSubscription<DbCsvProgress>? _watch;
  DateTime? _started;

  bool get _live => mounted && _scope.unlocked && identical(_scope, ref.read(dbScopeProvider));

  @override
  void dispose() {
    _debounce?.cancel();
    unawaited(_watch?.cancel());
    final id = _importId;
    if (id != null && !(_progress?.done ?? false)) unawaited(widget.service.cancelCsvImport(id).catchError((_) {}));
    _quote.dispose();
    _skip.dispose();
    _batch.dispose();
    super.dispose();
  }

  // ---- steps -----------------------------------------------------------------

  Future<void> _choose() async {
    final path = await ref.read(fileDialogServiceProvider).chooseOpenFile(extensions: const ['csv', 'tsv', 'txt']);
    if (path == null || !mounted || !_live) return;
    var size = 0;
    try {
      size = File(path).lengthSync();
    } catch (_) {}
    final l = context.l10n;
    setState(() {
      _path = path;
      _size = size;
      _preview = null;
      _columns = null;
      _checks = null;
      _fileError = size > dbCsvMaxBytes ? l.dbCsvTooLarge(dbCsvMaxBytes ~/ (1024 * 1024)) : null;
    });
    if (_fileError == null) await _loadPreview();
  }

  Future<void> _loadPreview() async {
    final path = _path;
    if (path == null) return;
    final generation = ++_generation, options = _options;
    setState(() {
      _previewing = true;
      _previewFailed = false;
    });
    try {
      final preview = await widget.service.csvPreview(path, options);
      if (!_live || generation != _generation) return;
      setState(() {
        _preview = preview;
        _checks = null;
        _autoMap(preview);
      });
    } catch (_) {
      if (_live && generation == _generation) setState(() => _previewFailed = true);
    } finally {
      if (_live && generation == _generation) setState(() => _previewing = false);
    }
  }

  void _setOptions(DbCsvOptions options) {
    setState(() => _options = options);
    _debounce?.cancel();
    _debounce = Timer(const Duration(milliseconds: 300), () => unawaited(_loadPreview()));
  }

  bool get _mongo => widget.session.engine == DbEngine.mongodb;
  bool get _redis => widget.session.engine == DbEngine.redis;

  /// Whether a file column may create a target of its own name: a MongoDB
  /// field or a Redis hash field.
  bool _newTarget(String name, List<DbEditColumn> columns) =>
      (_mongo ? dbCsvMongoField(name) : _redis && dbCsvRedisField(name)) && !columns.any((c) => c.name == name);

  /// Matches file columns to table columns by name, ignoring case and `_`;
  /// in MongoDB the others become new fields of the same name.
  void _autoMap(DbCsvPreview preview) {
    final columns = _columns;
    if (columns == null) {
      _targets = List.filled(preview.columns.length, null);
      return;
    }
    if (_redis) {
      // The first column is the key; with one more it is the value (a
      // string), with several they are hash fields of their names.
      final names = preview.columns;
      _targets = [
        for (var i = 0; i < names.length; i++)
          i == 0
              ? 'key'
              : names.length == 2
              ? 'value'
              : dbCsvRedisField(names[i])
              ? names[i]
              : null,
      ];
      return;
    }
    String norm(String s) => s.toLowerCase().replaceAll(RegExp(r'[\s_\-]'), '');
    final used = <String>{};
    _targets = [
      for (final name in preview.columns)
        switch (columns
            .where((c) => !c.generated && norm(c.name) == norm(name) && !used.contains(c.name))
            .firstOrNull) {
          final DbEditColumn column => (used..add(column.name)).isEmpty ? null : column.name,
          null when _mongo && _newTarget(name, columns) => name,
          null => null,
        },
    ];
  }

  Future<void> _enterMapping() async {
    if (_columns == null) {
      try {
        final columns = await widget.service.editColumns(widget.session.id, widget.table);
        if (!_live) return;
        setState(() => _columns = columns);
      } catch (_) {
        if (mounted && _live) showSnack(context, context.l10n.dbOperationFailed, error: true);
        return;
      }
    }
    if (_preview != null && _targets.every((t) => t == null)) setState(() => _autoMap(_preview!));
    setState(() => _step = _Step.map);
    unawaited(_check());
  }

  List<DbCsvMapping> get _mapping => [
    for (var i = 0; i < _targets.length; i++)
      if (_targets[i] case final target?) DbCsvMapping(source: i, target: target),
  ];

  bool get _duplicateTargets {
    final targets = _targets.whereType<String>().toList();
    return targets.toSet().length != targets.length;
  }

  Future<void> _check() async {
    final path = _path, mapping = _mapping;
    if (path == null || mapping.isEmpty || _duplicateTargets) {
      setState(() => _checks = null);
      return;
    }
    final generation = ++_generation;
    setState(() => _checking = true);
    try {
      final checks = await widget.service.csvCheck(widget.session.id, widget.table, path, _options, mapping);
      if (_live && generation == _generation) setState(() => _checks = checks);
    } catch (_) {
      if (_live && generation == _generation) setState(() => _checks = null);
    } finally {
      if (_live && generation == _generation) setState(() => _checking = false);
    }
  }

  /// Skip-existing and upsert match on a key: in MongoDB `_id` must be mapped.
  bool get _hasKey => _mongo ? _targets.contains('_id') : _columns?.any((c) => c.key) ?? false;

  Future<void> _import() async {
    final path = _path, l = context.l10n;
    if (path == null || _starting) return;
    final batch = int.tryParse(_batch.text.trim());
    if (batch == null || batch < 1 || batch > 5000) {
      showSnack(context, l.dbCsvBatchInvalid, error: true);
      return;
    }
    final plan = DbCsvPlan(
      table: widget.table,
      options: _options,
      mapping: _mapping,
      mode: _mode,
      batchRows: batch,
      skipBadRows: _skipBad,
      perBatch: _perBatch,
      truncate: _truncate,
    );
    setState(() => _starting = true);
    DbCsvReview? review;
    var started = false;
    try {
      review = await widget.service.reviewCsvImport(widget.session.id, path, plan);
      if (!mounted || !_live) return;
      final approved = await showAppDialog<bool>(
        context,
        secure: true,
        builder: (_) => DbQueryReviewDialog(
          query: review!.sql.expose(),
          review: DbQueryReview(ticket: review.ticket, risk: review.risk, expiresAt: review.expiresAt),
        ),
      );
      if (!mounted || !_live || approved != true) return;
      if (!DateTime.now().isBefore(review.expiresAt)) {
        showSnack(context, l.dbReviewExpired, error: true);
        return;
      }
      final id = await widget.service.startCsvImport(widget.session.id, review.ticket);
      started = true;
      if (!_live) {
        unawaited(widget.service.cancelCsvImport(id).catchError((_) {}));
        return;
      }
      setState(() {
        _importId = id;
        _started = DateTime.now();
        _step = _Step.run;
        _progress = const DbCsvProgress();
      });
      _watch = widget.service.watchCsvImport(id).listen((progress) {
        if (_live) setState(() => _progress = progress);
      });
    } catch (_) {
      if (mounted && _live) showSnack(context, l.dbOperationFailed, error: true);
    } finally {
      review?.sql.wipe();
      if (review != null && !started) unawaited(widget.service.discardCsvImport(review.ticket).catchError((_) {}));
      if (_live) setState(() => _starting = false);
    }
  }

  // ---- building ----------------------------------------------------------------

  Widget _caption(String text, {Color? color}) {
    final tokens = GlassTokens.of(context);
    return Text(text, style: tokens.typography.caption.copyWith(color: color ?? tokens.secondaryLabel));
  }

  Widget _fileStep() {
    final l = context.l10n, tokens = GlassTokens.of(context);
    final path = _path;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      mainAxisSize: MainAxisSize.min,
      children: [
        Text(l.dbCsvFileHelp(_bytes(l, dbCsvMaxBytes))),
        const SizedBox(height: GlassSpacing.s12),
        Row(
          children: [
            GlassButton(
              key: const ValueKey('db-csv-choose'),
              label: l.dbCsvChooseFile,
              icon: Icons.folder_open_rounded,
              onPressed: _choose,
            ),
            const SizedBox(width: GlassSpacing.s12),
            if (path != null)
              Expanded(
                child: Text(
                  '${_fileName(path)} · ${_bytes(l, _size)}', // l10n-ignore: name · size
                  key: const ValueKey('db-csv-file'),
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: tokens.typography.callout.copyWith(color: tokens.palette.label),
                ),
              ),
          ],
        ),
        if (_fileError case final error?) ...[
          const SizedBox(height: GlassSpacing.s12),
          InfoBanner(key: const ValueKey('db-csv-file-error'), tone: BannerTone.danger, message: error),
        ],
        if (_previewFailed) ...[
          const SizedBox(height: GlassSpacing.s12),
          InfoBanner(tone: BannerTone.danger, message: l.dbCsvUnreadable),
        ],
        const SizedBox(height: GlassSpacing.s12),
        _caption(l.dbCsvPrivacy),
      ],
    );
  }

  Widget _readStep() {
    final l = context.l10n;
    final preview = _preview;
    String delimiterLabel(String? d) => switch (d) {
      null => l.dbCsvAuto,
      ',' => l.dbCsvComma,
      ';' => l.dbCsvSemicolon,
      '\t' => l.dbCsvTab,
      '|' => l.dbCsvPipe,
      _ => d,
    };
    Widget field(double width, Widget child) => SizedBox(width: width, child: child);
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Wrap(
          spacing: GlassSpacing.s12,
          runSpacing: GlassSpacing.s8,
          crossAxisAlignment: WrapCrossAlignment.end,
          children: [
            field(
              170,
              GlassDropdownField<DbCsvEncoding>(
                key: const ValueKey('db-csv-encoding'),
                label: l.dbCsvEncoding,
                value: _options.encoding,
                items: [
                  GlassSelectItem(value: DbCsvEncoding.auto, label: l.dbCsvEncodingAuto),
                  const GlassSelectItem(value: DbCsvEncoding.utf8, label: 'UTF-8'), // l10n-ignore: encoding
                  const GlassSelectItem(value: DbCsvEncoding.windows1251, label: 'Windows-1251'), // l10n-ignore
                  const GlassSelectItem(value: DbCsvEncoding.latin1, label: 'Latin-1'), // l10n-ignore: encoding
                ],
                onChanged: (v) => _setOptions(_options.copyWith(encoding: v)),
              ),
            ),
            field(
              150,
              GlassDropdownField<String>(
                key: const ValueKey('db-csv-delimiter'),
                label: l.dbCsvDelimiter,
                value: _options.delimiter ?? '',
                items: [
                  for (final d in const ['', ',', ';', '\t', '|'])
                    GlassSelectItem(value: d, label: delimiterLabel(d.isEmpty ? null : d)),
                ],
                onChanged: (v) => _setOptions(_options.copyWith(delimiter: v, autoDelimiter: v.isEmpty)),
              ),
            ),
            field(
              70,
              Tooltip(
                message: l.dbCsvQuote,
                child: GlassField(
                  key: const ValueKey('db-csv-quote'),
                  controller: _quote,
                  inputFormatters: [LengthLimitingTextInputFormatter(1)],
                  onChanged: (v) {
                    if (v.length == 1) _setOptions(_options.copyWith(quote: v));
                  },
                ),
              ),
            ),
            field(
              90,
              Tooltip(
                message: l.dbCsvSkipRows,
                child: GlassField(
                  key: const ValueKey('db-csv-skip'),
                  controller: _skip,
                  keyboardType: TextInputType.number,
                  inputFormatters: [FilteringTextInputFormatter.digitsOnly, LengthLimitingTextInputFormatter(3)],
                  onChanged: (v) => _setOptions(_options.copyWith(skipRows: int.tryParse(v) ?? 0)),
                ),
              ),
            ),
            field(
              170,
              GlassDropdownField<DbCsvNull>(
                key: const ValueKey('db-csv-null'),
                label: l.dbCsvNull,
                value: _options.nullMarker,
                items: [
                  GlassSelectItem(value: DbCsvNull.empty, label: l.dbCsvNullEmpty),
                  const GlassSelectItem(value: DbCsvNull.word, label: 'NULL'), // l10n-ignore: literal marker
                  const GlassSelectItem(value: DbCsvNull.backslash, label: r'\N'), // l10n-ignore: literal marker
                  GlassSelectItem(value: DbCsvNull.never, label: l.dbCsvNullNever),
                ],
                onChanged: (v) => _setOptions(_options.copyWith(nullMarker: v)),
              ),
            ),
            field(
              150,
              GlassDropdownField<bool>(
                key: const ValueKey('db-csv-decimal'),
                label: l.dbCsvDecimal,
                value: _options.decimalComma,
                items: [
                  GlassSelectItem(value: false, label: l.dbCsvDecimalDot),
                  GlassSelectItem(value: true, label: l.dbCsvDecimalComma),
                ],
                onChanged: (v) => _setOptions(_options.copyWith(decimalComma: v)),
              ),
            ),
            field(
              170,
              GlassDropdownField<DbCsvDateOrder>(
                key: const ValueKey('db-csv-dates'),
                label: l.dbCsvDates,
                value: _options.dateOrder,
                items: const [
                  GlassSelectItem(value: DbCsvDateOrder.iso, label: '2026-10-22'), // l10n-ignore: date sample
                  GlassSelectItem(value: DbCsvDateOrder.dmy, label: '22.10.2026'), // l10n-ignore: date sample
                  GlassSelectItem(value: DbCsvDateOrder.mdy, label: '10/22/2026'), // l10n-ignore: date sample
                ],
                onChanged: (v) => _setOptions(_options.copyWith(dateOrder: v)),
              ),
            ),
          ],
        ),
        CheckboxListTile(
          key: const ValueKey('db-csv-header'),
          contentPadding: EdgeInsets.zero,
          dense: true,
          title: Text(l.dbCsvHeader),
          value: _options.header,
          onChanged: (v) => _setOptions(_options.copyWith(header: v ?? true)),
        ),
        if (preview != null)
          _caption(
            l.dbCsvDetected(delimiterLabel(preview.delimiter), switch (preview.encoding) {
              DbCsvEncoding.windows1251 => 'Windows-1251', // l10n-ignore: encoding
              DbCsvEncoding.latin1 => 'Latin-1', // l10n-ignore: encoding
              _ => 'UTF-8', // l10n-ignore: encoding
            }),
          ),
        const SizedBox(height: GlassSpacing.s6),
        SizedBox(height: 2, child: _previewing ? const LinearProgressIndicator() : null),
        if (_previewFailed) InfoBanner(tone: BannerTone.danger, message: l.dbCsvUnreadable),
        if (preview != null) Expanded(child: _table(preview.columns, preview.rows)),
      ],
    );
  }

  /// The preview rows as a compact table.
  Widget _table(List<String> columns, List<List<String?>> rows) {
    final l = context.l10n, tokens = GlassTokens.of(context);
    final style = tokens.typography.caption.copyWith(color: tokens.palette.label);
    Widget cell(String? text, {bool head = false}) => Container(
      width: 140,
      padding: const EdgeInsets.symmetric(horizontal: 6, vertical: 3),
      decoration: BoxDecoration(
        color: head ? tokens.surfaces.fillField : null,
        border: Border(
          right: BorderSide(color: tokens.surfaces.separator),
          bottom: BorderSide(color: tokens.surfaces.separator),
        ),
      ),
      child: Text(
        text ?? l.dbNullCell,
        maxLines: 1,
        overflow: TextOverflow.ellipsis,
        style: head
            ? style.copyWith(fontWeight: FontWeight.w600)
            : text == null
            ? style.copyWith(color: tokens.palette.tertiary, fontStyle: FontStyle.italic)
            : style,
      ),
    );
    return Scrollbar(
      child: SingleChildScrollView(
        key: const ValueKey('db-csv-preview'),
        scrollDirection: Axis.horizontal,
        child: SizedBox(
          width: 140.0 * columns.length,
          child: ListView.builder(
            itemCount: rows.length + 1,
            itemExtent: 22,
            itemBuilder: (_, i) => Row(
              children: [
                for (var c = 0; c < columns.length; c++)
                  i == 0 ? cell(columns[c], head: true) : cell(c < rows[i - 1].length ? rows[i - 1][c] : null),
              ],
            ),
          ),
        ),
      ),
    );
  }

  Widget _mapStep() {
    final l = context.l10n, tokens = GlassTokens.of(context);
    final preview = _preview, columns = _columns ?? const [];
    if (preview == null) return const SizedBox.shrink();
    final checks = {for (final c in _checks ?? const <DbCsvCheck>[]) c.target: c};
    final missing = (_checks ?? const <DbCsvCheck>[]).where((c) => c.missingRequired).map((c) => c.target).toList();
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Text(
          _mongo
              ? l.dbCsvMapHelpMongo
              : _redis
              ? l.dbCsvMapHelpRedis
              : l.dbCsvMapHelp,
        ),
        const SizedBox(height: GlassSpacing.s8),
        if (_duplicateTargets) InfoBanner(tone: BannerTone.danger, message: l.dbCsvDuplicateTarget),
        if (missing.isNotEmpty)
          InfoBanner(
            key: const ValueKey('db-csv-missing'),
            tone: BannerTone.warning,
            message: l.dbCsvMissingRequired(missing.join(', ')),
          ),
        SizedBox(height: 2, child: _checking ? const LinearProgressIndicator() : null),
        Expanded(
          child: ListView.builder(
            itemCount: preview.columns.length,
            itemBuilder: (context, i) {
              final target = _targets.elementAtOrNull(i);
              final check = target == null ? null : checks[target];
              final sample = preview.rows.map((r) => r.elementAtOrNull(i)).whereType<String>().firstOrNull;
              return Padding(
                padding: const EdgeInsets.symmetric(vertical: 3),
                child: Row(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    SizedBox(
                      width: 220,
                      child: Column(
                        crossAxisAlignment: CrossAxisAlignment.start,
                        children: [
                          Text(
                            preview.columns[i],
                            maxLines: 1,
                            overflow: TextOverflow.ellipsis,
                            style: tokens.typography.callout.copyWith(fontWeight: FontWeight.w600),
                          ),
                          if (sample != null)
                            Text(
                              sample,
                              maxLines: 1,
                              overflow: TextOverflow.ellipsis,
                              style: tokens.typography.caption.copyWith(color: tokens.palette.tertiary),
                            ),
                        ],
                      ),
                    ),
                    const Icon(Icons.arrow_forward_rounded, size: 14),
                    const SizedBox(width: GlassSpacing.s8),
                    SizedBox(
                      width: 240,
                      child: GlassDropdownField<String>(
                        key: ValueKey('db-csv-target-$i'),
                        value: target ?? '',
                        items: [
                          GlassSelectItem(value: '', label: l.dbCsvSkipColumn),
                          for (final c in columns.where((c) => !c.generated))
                            GlassSelectItem(
                              value: c.name,
                              label: c.typeName.isEmpty ? c.name : '${c.name} · ${c.typeName}', // l10n-ignore
                            ),
                          if (_newTarget(preview.columns[i], columns))
                            GlassSelectItem(value: preview.columns[i], label: l.dbCsvNewField(preview.columns[i])),
                        ],
                        onChanged: (v) {
                          setState(() => _targets[i] = v.isEmpty ? null : v);
                          unawaited(_check());
                        },
                      ),
                    ),
                    const SizedBox(width: GlassSpacing.s8),
                    if (check != null && check.bad > 0)
                      Expanded(
                        child: Text(
                          l.dbCsvBadValues(check.bad, check.samples.join(', ')),
                          key: ValueKey('db-csv-bad-$i'),
                          style: tokens.typography.caption.copyWith(color: tokens.palette.warning),
                        ),
                      ),
                  ],
                ),
              );
            },
          ),
        ),
      ],
    );
  }

  Widget _optionsStep() {
    final l = context.l10n, tokens = GlassTokens.of(context);
    return SingleChildScrollView(
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          SizedBox(
            width: 360,
            child: GlassDropdownField<DbCsvMode>(
              key: const ValueKey('db-csv-mode'),
              label: l.dbCsvMode,
              value: _mode,
              items: [
                GlassSelectItem(value: DbCsvMode.insert, label: l.dbCsvModeInsert),
                if (_hasKey) ...[
                  GlassSelectItem(value: DbCsvMode.skipExisting, label: l.dbCsvModeSkip),
                  GlassSelectItem(value: DbCsvMode.upsert, label: l.dbCsvModeUpsert),
                ],
              ],
              onChanged: (v) => setState(() => _mode = v),
            ),
          ),
          if (!_hasKey)
            _caption(switch (widget.session.engine) {
              DbEngine.clickhouse => l.dbCsvNoKeyClickhouse,
              DbEngine.mongodb => l.dbCsvNoKeyMongo,
              _ => l.dbCsvNoKey,
            }),
          const SizedBox(height: GlassSpacing.s12),
          Row(
            children: [
              SizedBox(
                width: 120,
                child: Tooltip(
                  message: l.dbCsvBatch,
                  child: GlassField(
                    key: const ValueKey('db-csv-batch'),
                    controller: _batch,
                    keyboardType: TextInputType.number,
                    inputFormatters: [FilteringTextInputFormatter.digitsOnly, LengthLimitingTextInputFormatter(4)],
                  ),
                ),
              ),
              const SizedBox(width: GlassSpacing.s8),
              _caption(l.dbCsvBatch),
            ],
          ),
          const SizedBox(height: GlassSpacing.s12),
          SizedBox(
            width: 360,
            child: GlassDropdownField<bool>(
              key: const ValueKey('db-csv-errors'),
              label: l.dbCsvOnError,
              value: _skipBad,
              items: [
                GlassSelectItem(value: false, label: l.dbCsvStop),
                GlassSelectItem(value: true, label: l.dbCsvSkipBad),
              ],
              onChanged: (v) => setState(() => _skipBad = v),
            ),
          ),
          const SizedBox(height: GlassSpacing.s12),
          if (widget.wholeTransaction)
            SizedBox(
              width: 360,
              child: GlassDropdownField<bool>(
                key: const ValueKey('db-csv-transaction'),
                label: l.dbCsvTransaction,
                value: _perBatch,
                items: [
                  GlassSelectItem(value: false, label: l.dbCsvWhole),
                  GlassSelectItem(value: true, label: l.dbCsvPerBatch),
                ],
                onChanged: (v) => setState(() => _perBatch = v),
              ),
            )
          else
            Text(
              l.dbCsvNoTransaction,
              key: const ValueKey('db-csv-no-transaction'),
              style: TextStyle(color: tokens.palette.tertiary),
            ),
          const SizedBox(height: GlassSpacing.s8),
          // Redis never offers FLUSHDB.
          if (!_redis)
            CheckboxListTile(
              key: const ValueKey('db-csv-truncate'),
              contentPadding: EdgeInsets.zero,
              dense: true,
              title: Text(
                _mongo ? l.dbCsvTruncateDocuments : l.dbCsvTruncate,
                style: TextStyle(color: tokens.palette.danger),
              ),
              subtitle: Text(widget.wholeTransaction ? l.dbCsvTruncateHelp : l.dbCsvTruncateHelpNoTransaction),
              value: _truncate,
              onChanged: (v) => setState(() => _truncate = v ?? false),
            ),
        ],
      ),
    );
  }

  Widget _runStep() {
    final l = context.l10n, tokens = GlassTokens.of(context);
    final progress = _progress ?? const DbCsvProgress();
    final fraction = progress.totalBytes == 0 ? null : progress.bytesRead / progress.totalBytes;
    final elapsed = DateTime.now().difference(_started ?? DateTime.now()).inMilliseconds / 1000;
    final rate = elapsed > 0 ? progress.rowsRead / elapsed : 0;
    final eta = fraction == null || fraction <= 0 || progress.done
        ? null
        : Duration(seconds: (elapsed / fraction - elapsed).round());
    final report = progress.report;
    final error = progress.error;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      mainAxisSize: MainAxisSize.min,
      children: [
        LinearProgressIndicator(key: const ValueKey('db-csv-progress'), value: progress.done ? 1 : fraction),
        const SizedBox(height: GlassSpacing.s8),
        Text(
          l.dbCsvCounts(progress.rowsRead, progress.imported, progress.skipped, progress.failed),
          key: const ValueKey('db-csv-counts'),
        ),
        if (!progress.done)
          _caption(
            eta == null
                ? l.dbCsvRate(rate.round())
                : l.dbCsvRateEta(rate.round(), '${eta.inMinutes}:${(eta.inSeconds % 60).toString().padLeft(2, '0')}'),
          ),
        if (progress.done) ...[
          const SizedBox(height: GlassSpacing.s8),
          InfoBanner(
            key: const ValueKey('db-csv-summary'),
            tone: progress.cancelled || error != null ? BannerTone.warning : BannerTone.success,
            message: progress.cancelled
                ? l.dbCsvCancelled(progress.imported)
                : error != null
                ? _errorText(l, error)
                : l.dbCsvDone(progress.imported, progress.skipped, progress.failed),
          ),
        ],
        if (report != null) ...[
          const SizedBox(height: GlassSpacing.s8),
          _caption(l.dbCsvReport),
          Row(
            children: [
              Expanded(
                child: SelectableText(
                  report,
                  key: const ValueKey('db-csv-report'),
                  style: tokens.typography.caption.copyWith(color: tokens.palette.label),
                ),
              ),
              if (!AppPlatform.isMobile)
                GlassButton(
                  label: l.dbCsvShowReport,
                  size: GlassControlSize.sm,
                  onPressed: () =>
                      unawaited(launchUrl(Uri.file(File(report).parent.path, windows: AppPlatform.isWindows))),
                ),
            ],
          ),
        ],
      ],
    );
  }

  String _errorText(AppLocalizations l, String code) {
    final row = RegExp(r'^row_(\w+):(\d+)$').firstMatch(code);
    if (row != null) {
      final line = int.parse(row[2]!);
      return switch (row[1]) {
        'type' => l.dbCsvRowType(line),
        'required' => l.dbCsvRowRequired(line),
        _ => l.dbCsvRowUnreadable(line),
      };
    }
    return l.dbCsvFailed;
  }

  @override
  Widget build(BuildContext context) {
    ref.listen(dbScopeProvider, (_, next) {
      if (!identical(next, _scope)) {
        WidgetsBinding.instance.addPostFrameCallback((_) {
          if (mounted) closeDialog<bool>(context, false);
        });
      }
    });
    if (!_scope.unlocked || !identical(_scope, ref.watch(dbScopeProvider))) return const SizedBox.shrink();
    final l = context.l10n;
    final progress = _progress;
    final running = _step == _Step.run && !(progress?.done ?? false);
    final titles = {
      _Step.file: l.dbCsvStepFile,
      _Step.read: l.dbCsvStepRead,
      _Step.map: l.dbCsvStepMap,
      _Step.options: l.dbCsvStepOptions,
      _Step.run: l.dbCsvStepRun,
    };
    final canNext = switch (_step) {
      _Step.file => _path != null && _fileError == null && _preview != null,
      _Step.read => _preview != null && !_previewing && _preview!.columns.isNotEmpty,
      _Step.map => _mapping.isNotEmpty && !_duplicateTargets && !_checking,
      _Step.options => !_starting,
      _Step.run => false,
    };
    void next() => switch (_step) {
      _Step.file => setState(() => _step = _Step.read),
      _Step.read => unawaited(_enterMapping()),
      _Step.map => setState(() => _step = _Step.options),
      _Step.options => unawaited(_import()),
      _Step.run => null,
    };
    return GlassDialog(
      key: const ValueKey('db-csv-dialog'),
      width: 900,
      title: l.dbCsvTitle(widget.table.name),
      content: SizedBox(
        width: 900,
        height: _step == _Step.file || _step == _Step.run ? null : MediaQuery.sizeOf(context).height * .6,
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          mainAxisSize: MainAxisSize.min,
          children: [
            Text(
              l.dbCsvStep(_step.index + 1, _Step.values.length, titles[_step]!),
              key: const ValueKey('db-csv-step'),
              style: GlassTokens.of(context).typography.bodyEmph,
            ),
            const SizedBox(height: GlassSpacing.s12),
            if (_step == _Step.file || _step == _Step.run)
              switch (_step) {
                _Step.file => _fileStep(),
                _ => _runStep(),
              }
            else
              Expanded(
                child: switch (_step) {
                  _Step.read => _readStep(),
                  _Step.map => _mapStep(),
                  _ => _optionsStep(),
                },
              ),
          ],
        ),
      ),
      secondaryActions: [
        if (running)
          GlassButton(
            key: const ValueKey('db-csv-cancel'),
            label: l.commonCancel,
            onPressed: () {
              final id = _importId;
              if (id != null) unawaited(widget.service.cancelCsvImport(id).catchError((_) {}));
            },
          )
        else
          GlassButton(
            key: const ValueKey('db-csv-close'),
            label: _step == _Step.run ? l.commonClose : l.commonCancel,
            onPressed: () => closeDialog<bool>(context, (progress?.imported ?? 0) > 0),
          ),
        if (_step.index > 0 && _step != _Step.run)
          GlassButton(
            key: const ValueKey('db-csv-back'),
            label: l.commonBack,
            onPressed: () => setState(() => _step = _Step.values[_step.index - 1]),
          ),
      ],
      primaryAction: _step == _Step.run
          ? null
          : GlassButton(
              key: const ValueKey('db-csv-next'),
              label: _step == _Step.options ? l.dbCsvImport : l.commonContinue,
              style: _step == _Step.options && _truncate ? GlassButtonStyle.destructive : GlassButtonStyle.prominent,
              onPressed: canNext ? next : null,
            ),
    );
  }
}

/// Hook for the table designer: a simple `CREATE TABLE` typed from a CSV
/// file, shown as code to copy or open in the editor (where it runs through
/// the usual review). [container] is the schema (PostgreSQL) or database
/// (MySQL) node the table goes to.
Future<void> showCsvCreateTable(
  BuildContext context, {
  required DbCsvImportService service,
  required DbSessionInfo session,
  required DbObjectRef container,
  required ValueChanged<String> onDraft,
  required WidgetRef ref,
}) async {
  final l = context.l10n;
  final path = await ref.read(fileDialogServiceProvider).chooseOpenFile(extensions: const ['csv', 'tsv', 'txt']);
  if (path == null || !context.mounted) return;
  final stem = path.split(Platform.pathSeparator).last.replaceFirst(RegExp(r'\.[^.]*$'), '');
  final name = await showTextInputDialog(
    context,
    title: l.dbCsvCreateTable,
    label: l.dbCsvNewTableName,
    initial: stem.replaceAll(RegExp(r'[^\w]+'), '_').toLowerCase(),
    confirmLabel: l.commonContinue,
  );
  if (name == null || name.trim().isEmpty || !context.mounted) return;
  final table = DbObjectRef(
    database: container.kind == DbObjectKind.database ? container.name : container.database,
    schema: container.kind == DbObjectKind.schema ? container.name : null,
    kind: DbObjectKind.table,
    name: name.trim(),
  );
  try {
    final sql = SecretText(await service.csvCreateTable(path, const DbCsvOptions(), session.engine, table));
    if (!context.mounted) return;
    final open = await showAppDialog<bool>(
      context,
      secure: true,
      builder: (_) => DbDdlDialog(title: l.dbCsvCreateTable, value: sql, engine: session.engine),
    );
    if (open == true && !sql.isWiped) onDraft(sql.expose());
    sql.wipe();
  } catch (_) {
    if (context.mounted) showSnack(context, l.dbCsvUnreadable, error: true);
  }
}

/// The last path segment, whichever separator the platform's picker used.
String _fileName(String path) => path.split(RegExp(r'[\\/]')).last;
