import 'dart:async';
import 'dart:convert';

import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/db/db_models.dart';
import 'package:consolecrypt/db/db_providers.dart';
import 'package:consolecrypt/db/db_query_service.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

/// Typed inputs only. The native parser replaces placeholders with bound driver
/// parameters; this dialog never interpolates text into the query.
class DbVariablesDialog extends ConsumerStatefulWidget {
  const DbVariablesDialog({required this.names, super.key});
  final List<String> names;
  @override
  ConsumerState<DbVariablesDialog> createState() => _DbVariablesDialogState();
}

class _DbVariablesDialogState extends ConsumerState<DbVariablesDialog> {
  final _form = GlobalKey<FormState>();
  late final _scope = ref.read(dbScopeProvider);
  late final _values = [for (final _ in widget.names) TextEditingController()];
  late final _types = [for (final _ in widget.names) DbCellKind.text];
  static const types = [
    DbCellKind.text,
    DbCellKind.intValue,
    DbCellKind.decimal,
    DbCellKind.boolValue,
    DbCellKind.json,
    DbCellKind.date,
    DbCellKind.nullValue,
  ];
  @override
  void dispose() {
    for (final value in _values) {
      value.clear();
      value.dispose();
    }
    super.dispose();
  }

  DbCell? _cell(int index) {
    final kind = _types[index], text = _values[index].text;
    switch (kind) {
      case DbCellKind.nullValue:
        return const DbCell(DbCellKind.nullValue, null);
      case DbCellKind.intValue:
        if (BigInt.tryParse(text) == null) return null;
      case DbCellKind.decimal:
        if (!RegExp(r'^[+-]?(?:\d+(?:\.\d*)?|\.\d+)(?:[eE][+-]?\d+)?$').hasMatch(text)) return null;
      case DbCellKind.boolValue:
        return switch (text) {
          'true' => const DbCell(DbCellKind.boolValue, true),
          'false' => const DbCell(DbCellKind.boolValue, false),
          _ => null,
        };
      case DbCellKind.json:
        try {
          jsonDecode(text);
        } catch (_) {
          return null;
        }
      case DbCellKind.date:
        if (DateTime.tryParse(text) == null) return null;
      default:
        break;
    }
    return DbCell(kind, text);
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    ref.listen(dbScopeProvider, (_, next) {
      if (!identical(next, _scope)) {
        for (final value in _values) {
          value.clear();
        }
        WidgetsBinding.instance.addPostFrameCallback((_) {
          if (mounted) closeDialog<void>(context);
        });
      }
    });
    if (!_scope.unlocked || !identical(_scope, ref.watch(dbScopeProvider))) return const SizedBox.shrink();
    return GlassDialog(
      title: l.dbVariables,
      content: SizedBox(
        width: 560,
        child: SingleChildScrollView(
          child: Form(
            key: _form,
            child: Column(
              mainAxisSize: MainAxisSize.min,
              children: [
                Text(l.dbVariablesHelp),
                for (var i = 0; i < widget.names.length; i++)
                  Padding(
                    padding: const EdgeInsets.symmetric(vertical: 8),
                    child: Column(
                      children: [
                        GlassDropdownField<DbCellKind>(
                          value: _types[i],
                          label: '${widget.names[i]} · ${l.dbValueType}',
                          items: [for (final t in types) GlassSelectItem(value: t, label: t.wireName)],
                          onChanged: (t) => setState(() => _types[i] = t),
                        ),
                        if (_types[i] != DbCellKind.nullValue)
                          TextFormField(
                            controller: _values[i],
                            decoration: InputDecoration(labelText: l.dbValue),
                            autocorrect: false,
                            enableSuggestions: false,
                            validator: (_) => _cell(i) == null ? l.dbInvalidValue : null,
                          ),
                      ],
                    ),
                  ),
              ],
            ),
          ),
        ),
      ),
      secondaryActions: [GlassButton(label: l.commonCancel, onPressed: () => closeDialog<void>(context))],
      primaryAction: GlassButton(
        label: l.commonContinue,
        onPressed: () {
          if (_form.currentState!.validate()) {
            closeDialog(context, [
              for (var i = 0; i < widget.names.length; i++) DbParameter(name: widget.names[i], value: _cell(i)!),
            ]);
          }
        },
      ),
    );
  }
}

class DbQueryReviewDialog extends ConsumerStatefulWidget {
  const DbQueryReviewDialog({required this.connectionName, required this.query, required this.review, super.key});
  final String connectionName, query;
  final DbQueryReview review;
  @override
  ConsumerState<DbQueryReviewDialog> createState() => _DbQueryReviewDialogState();
}

class _DbQueryReviewDialogState extends ConsumerState<DbQueryReviewDialog> {
  late final _scope = ref.read(dbScopeProvider);
  final _confirmation = TextEditingController();
  Timer? _timer;
  bool get _expired => !DateTime.now().isBefore(widget.review.expiresAt);
  @override
  void initState() {
    super.initState();
    _timer = Timer(widget.review.expiresAt.difference(DateTime.now()), () {
      if (mounted) setState(() {});
    });
  }

  @override
  void dispose() {
    _timer?.cancel();
    _confirmation.clear();
    _confirmation.dispose();
    super.dispose();
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
    if (!_scope.unlocked || !identical(_scope, ref.watch(dbScopeProvider))) return const SizedBox.shrink();
    return GlassDialog(
      title: l.dbReviewQuery,
      content: SizedBox(
        width: 760,
        child: SingleChildScrollView(
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Text(l.dbQueryDangerous),
              const SizedBox(height: 12),
              SelectableText(widget.query, style: const TextStyle(fontFamily: 'monospace')),
              const SizedBox(height: 12),
              TextField(
                key: const ValueKey('db-query-confirmation'),
                controller: _confirmation,
                decoration: InputDecoration(labelText: widget.connectionName),
                onChanged: (_) => setState(() {}),
              ),
              if (_expired) Text(l.dbReviewExpired),
            ],
          ),
        ),
      ),
      secondaryActions: [GlassButton(label: l.commonCancel, onPressed: () => closeDialog<bool>(context, false))],
      primaryAction: GlassButton.destructive(
        label: l.dbRun,
        onPressed: !_expired && _confirmation.text == widget.connectionName
            ? () => closeDialog<bool>(context, true)
            : null,
      ),
    );
  }
}
