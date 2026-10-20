import 'dart:convert';
import 'dart:math';

import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:material_ui/material_ui.dart';

final class DbJsonNumber {
  const DbJsonNumber(this.text);
  final String text;
  @override
  String toString() => text;
  String toJson() => text;
}

/// Preserve number lexemes in a display tree instead of coercing them to double.
Object? decodeDbDocument(String text) {
  final random = Random.secure();
  var prefix = '';
  do {
    prefix = '__cc_${List.generate(4, (_) => random.nextInt(0x100000000).toRadixString(16)).join()}_';
  } while (text.contains(prefix));
  final tokens = RegExp(r'''"(?:\\.|[^"\\])*"|-?(?:0|[1-9]\d*)(?:\.\d+)?(?:[eE][+-]?\d+)?''');
  final encoded = text.replaceAllMapped(tokens, (m) => m[0]!.startsWith('"') ? m[0]! : jsonEncode('$prefix${m[0]}'));
  return jsonDecode(
    encoded,
    reviver: (_, value) =>
        value is String && value.startsWith(prefix) ? DbJsonNumber(value.substring(prefix.length)) : value,
  );
}

/// A lazy, depth-limited document tree; large arrays never allocate a widget per
/// entry. Raw values stay in the enclosing scoped result, never on disk.
class DbDocumentView extends StatefulWidget {
  const DbDocumentView({required this.document, this.rawText, this.maxNodes = 10000, super.key});
  final Object? document;
  final String? rawText;
  final int maxNodes;
  @override
  State<DbDocumentView> createState() => _DbDocumentViewState();
}

class _DbDocumentViewState extends State<DbDocumentView> {
  final _expanded = <String>{''};
  bool _raw = false;
  List<(String, String, Object?, int)> _rows() {
    final rows = <(String, String, Object?, int)>[];
    void visit(String path, String label, Object? value, int depth) {
      if (rows.length >= widget.maxNodes || depth > 64) return;
      rows.add((path, label, value, depth));
      if (!_expanded.contains(path)) return;
      if (value is Map) {
        for (final key in value.keys) {
          if (rows.length >= widget.maxNodes) break;
          visit(
            '$path/${key.toString().replaceAll('~', '~0').replaceAll('/', '~1')}',
            key.toString(),
            value[key],
            depth + 1,
          );
        }
      } else if (value is List) {
        for (var i = 0; i < value.length && rows.length < widget.maxNodes; i++) {
          visit('$path/$i', '$i', value[i], depth + 1);
        }
      }
    }

    visit('', '', widget.document, 0);
    return rows;
  }

  @override
  Widget build(BuildContext context) {
    final rows = _rows(), l = context.l10n;
    return Column(
      children: [
        Align(
          alignment: Alignment.centerRight,
          child: IconButton(
            tooltip: _raw ? l.dbTree : l.dbShowValue,
            icon: Icon(_raw ? Icons.account_tree : Icons.code),
            onPressed: () => setState(() => _raw = !_raw),
          ),
        ),
        Expanded(
          child: _raw
              ? SingleChildScrollView(
                  child: SelectableText(
                    widget.rawText ?? const JsonEncoder.withIndent('  ').convert(widget.document),
                    style: const TextStyle(fontFamily: 'monospace'),
                  ),
                )
              : ListView.builder(
                  itemCount: rows.length,
                  itemBuilder: (_, i) {
                    final (path, label, value, depth) = rows[i];
                    final nested = value is Map || value is List;
                    final text = nested
                        ? (value is Map ? '{${value.length}}' : '[${(value as List).length}]')
                        : (value == null ? l.dbNull : value.toString());
                    return Padding(
                      padding: EdgeInsets.only(left: (depth * 12).clamp(0, 96).toDouble()),
                      child: ListTile(
                        dense: true,
                        leading: nested
                            ? Icon(_expanded.contains(path) ? Icons.expand_more : Icons.chevron_right)
                            : null,
                        title: Text(
                          label.isEmpty ? text : '$label: $text',
                          maxLines: 3,
                          overflow: TextOverflow.ellipsis,
                          style: const TextStyle(fontFamily: 'monospace'),
                        ),
                        onTap: nested
                            ? () => setState(() {
                                if (!_expanded.remove(path)) _expanded.add(path);
                              })
                            : null,
                      ),
                    );
                  },
                ),
        ),
        if (rows.length >= widget.maxNodes) Text(l.dbTruncated),
      ],
    );
  }
}
