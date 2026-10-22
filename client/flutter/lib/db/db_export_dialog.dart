import 'dart:async';

import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/db/db_models.dart';
import 'package:consolecrypt/db/db_providers.dart';
import 'package:consolecrypt/db/db_query_service.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

class DbExportDialog extends ConsumerStatefulWidget {
  const DbExportDialog({
    required this.service,
    required this.session,
    required this.queryId,
    required this.resultIndex,
    this.selection,
    super.key,
  });
  final DbExportService service;
  final DbSessionInfo session;
  final String queryId;
  final int resultIndex;

  /// Non-null uses the clipboard; null streams an explicit destination.
  final DbResultSelection? selection;
  @override
  ConsumerState<DbExportDialog> createState() => _DbExportDialogState();
}

class _DbExportDialogState extends ConsumerState<DbExportDialog> {
  late final _scope = ref.read(dbScopeProvider);
  final _delimiter = TextEditingController(text: ','), _quote = TextEditingController(text: '"');
  DbExportFormat _format = DbExportFormat.csv;
  bool _header = true, _bom = false, _escapeFormulas = true, _busy = false, _failed = false;
  String? _exportId;
  DbExportProgress? _progress;
  StreamSubscription<DbExportProgress>? _subscription;
  bool get _current => mounted && _scope.unlocked && identical(_scope, ref.read(dbScopeProvider));
  Future<void> _cancel() async {
    final id = _exportId;
    _exportId = null;
    unawaited(_subscription?.cancel() ?? Future<void>.value());
    _subscription = null;
    if (id != null) {
      try {
        await widget.service.cancelExport(widget.session.id, id);
      } catch (_) {}
    }
  }

  @override
  void dispose() {
    unawaited(_cancel());
    _delimiter.dispose();
    _quote.dispose();
    super.dispose();
  }

  Future<void> _start() async {
    if (!_current || _busy || _delimiter.text.runes.length != 1 || _quote.text.runes.length != 1) return;
    setState(() {
      _busy = true;
      _failed = false;
    });
    final options = DbExportOptions(
      format: _format,
      header: _header,
      bom: _bom,
      delimiter: _delimiter.text,
      quote: _quote.text,
      escapeFormulas: _escapeFormulas,
    );
    try {
      final selection = widget.selection;
      if (selection != null) {
        final value = await widget.service.copyResult(
          widget.session.id,
          widget.queryId,
          widget.resultIndex,
          selection,
          options,
        );
        try {
          if (!_current) return;
          await ref.read(secureClipboardProvider).copySecret(value.expose(), isCurrent: () => _current);
          if (mounted && _current) closeDialog<void>(context);
        } finally {
          value.wipe();
        }
      } else {
        final destination = await widget.service.chooseExportDestination(options);
        if (!_current || destination == null) return;
        final id = await widget.service.startExport(
          widget.session.id,
          widget.queryId,
          widget.resultIndex,
          options,
          destination,
        );
        if (!_current) {
          await widget.service.cancelExport(widget.session.id, id);
          return;
        }
        _exportId = id;
        _subscription = widget.service
            .watchExport(widget.session.id, id)
            .listen(
              (progress) {
                if (!_current) return;
                setState(() {
                  _progress = progress;
                  _busy = !progress.done && !progress.cancelled && progress.error == null;
                  _failed = progress.error != null;
                });
              },
              onError: (Object _) {
                if (_current) {
                  setState(() {
                    _failed = true;
                    _busy = false;
                  });
                }
              },
            );
      }
    } catch (_) {
      if (_current) setState(() => _failed = true);
    } finally {
      if (_current && _exportId == null) setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    ref.listen(dbScopeProvider, (_, next) {
      if (!identical(next, _scope)) {
        unawaited(_cancel());
        WidgetsBinding.instance.addPostFrameCallback((_) {
          if (mounted) closeDialog<void>(context);
        });
      }
    });
    if (!_current) return const SizedBox.shrink();
    return GlassDialog(
      title: widget.selection == null ? l.dbExport : l.commonCopy,
      content: SizedBox(
        width: 520,
        child: SingleChildScrollView(
          child: Column(
            mainAxisSize: MainAxisSize.min,
            children: [
              Text(widget.selection == null ? l.dbExportHelp : l.dbSensitiveClipboard),
              GlassDropdownField<DbExportFormat>(
                value: _format,
                label: l.dbExportFormat,
                items: [
                  for (final format in DbExportFormat.values)
                    if (format != DbExportFormat.insert || widget.session.engine.semicolons)
                      GlassSelectItem(value: format, label: format.name.toUpperCase()),
                ],
                onChanged: _busy ? null : (v) => setState(() => _format = v),
              ),
              if (_format == DbExportFormat.csv || _format == DbExportFormat.tsv) ...[
                CheckboxListTile(
                  title: Text(l.dbExportHeader),
                  value: _header,
                  onChanged: _busy ? null : (v) => setState(() => _header = v!),
                ),
                CheckboxListTile(
                  key: const ValueKey('db-export-escape-formulas'),
                  title: Text(l.dbExportEscapeFormulas),
                  value: _escapeFormulas,
                  onChanged: _busy ? null : (v) => setState(() => _escapeFormulas = v!),
                ),
                if (widget.selection == null)
                  CheckboxListTile(
                    title: Text(l.dbExportBom),
                    value: _bom,
                    onChanged: _busy ? null : (v) => setState(() => _bom = v!),
                  ),
                if (_format == DbExportFormat.csv) ...[
                  TextField(
                    controller: _delimiter,
                    maxLength: 1,
                    decoration: InputDecoration(labelText: l.dbExportDelimiter),
                  ),
                  TextField(
                    controller: _quote,
                    maxLength: 1,
                    decoration: InputDecoration(labelText: l.dbExportQuote),
                  ),
                ],
              ],
              if (_busy) const LinearProgressIndicator(),
              if (_progress != null)
                Text('${_progress!.rows} · ${_progress!.bytes} B'), // l10n-ignore: Counts and byte unit.
              if (_progress?.done ?? false) Text(l.dbExportDone),
              if (_failed) Text(l.dbOperationFailed),
            ],
          ),
        ),
      ),
      secondaryActions: [
        GlassButton(
          label: _busy ? l.commonCancel : l.commonClose,
          onPressed: () {
            unawaited(_cancel());
            closeDialog<void>(context);
          },
        ),
      ],
      primaryAction: _progress?.done ?? false
          ? null
          : GlassButton(label: widget.selection == null ? l.dbExport : l.commonCopy, onPressed: _busy ? null : _start),
    );
  }
}
