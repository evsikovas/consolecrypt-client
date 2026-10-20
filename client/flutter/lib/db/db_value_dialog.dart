import 'dart:convert';

import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/db/db_document_view.dart';
import 'package:consolecrypt/db/db_models.dart';
import 'package:consolecrypt/db/db_providers.dart';
import 'package:flutter/foundation.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

class DbValueDialog extends ConsumerStatefulWidget {
  const DbValueDialog({required this.column, required this.cell, super.key});
  final DbColumn column;
  final DbCell cell;
  @override
  ConsumerState<DbValueDialog> createState() => _DbValueDialogState();
}

class _DbValueDialogState extends ConsumerState<DbValueDialog> {
  late final DbScope _scope = ref.read(dbScopeProvider);
  bool _hex = false;
  Object? _document;
  bool _decoded = false;
  @override
  void initState() {
    super.initState();
    if (widget.cell.kind == DbCellKind.json && widget.cell.text.length <= 1048576) _decode();
  }

  Future<void> _decode() async {
    try {
      final document = await compute(decodeDbDocument, widget.cell.text);
      if (_current) {
        setState(() {
          _document = document;
          _decoded = true;
        });
      }
    } catch (_) {}
  }

  bool get _current => mounted && _scope.unlocked && identical(_scope, ref.read(dbScopeProvider));
  String _display() {
    final cell = widget.cell;
    if (_hex && cell.kind == DbCellKind.binary) {
      try {
        final bytes = base64Decode(cell.text);
        return bytes.take(4096).map((b) => b.toRadixString(16).padLeft(2, '0')).join(' ');
      } catch (_) {
        return '';
      }
    }
    return cell.kind == DbCellKind.nullValue ? context.l10n.dbNull : cell.text;
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    ref.listen(dbScopeProvider, (_, next) {
      if (!identical(next, _scope)) {
        WidgetsBinding.instance.addPostFrameCallback((_) {
          if (mounted) closeDialog<void>(context);
        });
      }
    });
    if (!_current) return const SizedBox.shrink();
    return GlassDialog(
      title: widget.column.name,
      content: SizedBox(
        width: 720,
        child: SingleChildScrollView(
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Text(widget.column.typeName),
              if (widget.column.sensitive) Text(l.dbSensitiveClipboard),
              if (widget.cell.kind == DbCellKind.binary)
                SwitchListTile(title: Text(l.dbBinaryPreview), value: _hex, onChanged: (v) => setState(() => _hex = v)),
              if (_decoded)
                SizedBox(
                  height: MediaQuery.sizeOf(context).height * .5,
                  child: DbDocumentView(document: _document, rawText: widget.cell.text),
                )
              else
                SelectableText(_display(), style: const TextStyle(fontFamily: 'monospace')),
            ],
          ),
        ),
      ),
      secondaryActions: [GlassButton(label: l.commonClose, onPressed: () => closeDialog<void>(context))],
      primaryAction: GlassButton(
        label: l.commonCopy,
        onPressed: () async {
          if (!_current) return;
          await ref.read(secureClipboardProvider).copySecret(widget.cell.text, isCurrent: () => _current);
        },
      ),
    );
  }
}
