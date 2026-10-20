import 'dart:async';

import 'package:consolecrypt/app/platform.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/core/widgets/secret_field.dart';
import 'package:consolecrypt/db/db_models.dart';
import 'package:consolecrypt/db/db_providers.dart';
import 'package:consolecrypt/db/db_service.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

class DbImportDialog extends ConsumerStatefulWidget {
  const DbImportDialog({super.key});
  @override
  ConsumerState<DbImportDialog> createState() => _DbImportDialogState();
}

class _DbImportDialogState extends ConsumerState<DbImportDialog> {
  final _source = TextEditingController();
  final _selected = <int>{}, _nonProduction = <int>{}, _withoutPassword = <int>{};
  late final DbScope _scope;
  late final DbService? _service;
  DbImportReview? _review;
  DbConnectionChoices? _choices;
  final _routes = <int, DbRoute>{};
  SecretText? _pendingSecret;
  Timer? _expiry;
  bool _busy = false, _failed = false, _expired = false;
  bool get _current => mounted && _scope.unlocked && identical(_scope, ref.read(dbScopeProvider));
  @override
  void initState() {
    super.initState();
    _scope = ref.read(dbScopeProvider);
    _service = ref.read(dbServiceProvider);
    unawaited(_loadChoices());
  }

  Future<void> _loadChoices() async {
    try {
      final choices = await _service?.connectionChoices();
      if (_current) setState(() => _choices = choices);
    } catch (_) {}
  }

  void _discard(DbImportReview? review) {
    if (review != null) unawaited(_service?.discardImport(review.ticket).catchError((Object _) {}) ?? Future.value());
  }

  @override
  void dispose() {
    _expiry?.cancel();
    _source.clear();
    _source.dispose();
    _pendingSecret?.wipe();
    _discard(_review);
    super.dispose();
  }

  Future<void> _preview({bool file = false}) async {
    final service = _service;
    if (!_current || _busy || service == null || (!file && _source.text.trim().isEmpty)) return;
    final secret = SecretText(_source.text);
    _pendingSecret = secret;
    _source.clear();
    setState(() {
      _busy = true;
      _failed = false;
    });
    try {
      final review = file && service is DbPgpassFileService
          ? await (service as DbPgpassFileService).selectPgpassFile()
          : await service.importConnectionString(secret);
      if (!_current) {
        _discard(review);
        return;
      }
      if (review == null) return;
      _discard(_review);
      _expiry?.cancel();
      final remaining = review.expiresAt.difference(DateTime.now());
      setState(() {
        _review = review;
        _expired = remaining <= Duration.zero;
        _selected.clear();
        _nonProduction.clear();
        _withoutPassword.clear();
        _routes.clear();
        _selected.addAll(review.entries.where((e) => e.errorCode == null).take(64).map((e) => e.index));
      });
      if (!_expired) {
        _expiry = Timer(remaining, () {
          if (_current) setState(() => _expired = true);
        });
      }
    } catch (_) {
      if (_current) setState(() => _failed = true);
    } finally {
      secret.wipe();
      if (identical(_pendingSecret, secret)) _pendingSecret = null;
      if (_current) setState(() => _busy = false);
    }
  }

  Future<void> _save() async {
    final review = _review;
    if (!_current || _busy || review == null || _expired || _selected.isEmpty) return;
    if (!DateTime.now().isBefore(review.expiresAt)) {
      setState(() => _expired = true);
      return;
    }
    setState(() {
      _busy = true;
      _failed = false;
    });
    try {
      await _service!.confirmImport(review.ticket, [
        for (final index in _selected)
          DbImportChoice(
            index: index,
            production: !_nonProduction.contains(index),
            savePassword: !_withoutPassword.contains(index),
            route: _routes[index] ?? const DbRoute(),
          ),
      ]);
      if (mounted && _current) {
        _review = null;
        ref.invalidate(dbConnectionsProvider(_scope));
        closeDialog(context, true);
      }
    } catch (_) {
      if (_current) setState(() => _failed = true);
    } finally {
      if (_current) setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    ref.listen(dbScopeProvider, (_, next) {
      if (!identical(next, _scope)) {
        _source.clear();
        _pendingSecret?.wipe();
        _discard(_review);
        _review = null;
        WidgetsBinding.instance.addPostFrameCallback((_) {
          if (mounted) closeDialog<void>(context);
        });
      }
    });
    return GlassDialog(
      title: l.dbImport,
      content: SizedBox(
        width: 650,
        child: SingleChildScrollView(
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              if (_busy) const LinearProgressIndicator(),
              Text(l.dbImportHelp),
              const SizedBox(height: 12),
              if (_failed) Text(l.dbFailed),
              if (_expired) Text(l.dbImportExpired),
              SecretField(
                key: const ValueKey('db-import-uri'),
                controller: _source,
                label: l.dbConnectionString,
                enabled: !_busy,
              ),
              const SizedBox(height: 12),
              Wrap(
                spacing: 8,
                runSpacing: 8,
                children: [
                  GlassButton(
                    key: const ValueKey('db-import-review'),
                    label: l.commonReview,
                    onPressed: _busy ? null : _preview,
                  ),
                  if (!AppPlatform.isMobile && _service is DbPgpassFileService)
                    GlassButton(label: l.dbImportPgpass, onPressed: _busy ? null : () => _preview(file: true)),
                ],
              ),
              for (final entry in _review?.entries ?? const <DbImportEntry>[])
                Padding(
                  padding: const EdgeInsets.only(top: 12),
                  child: ContentSurface(
                    child: Column(
                      mainAxisSize: MainAxisSize.min,
                      children: [
                        CheckboxListTile(
                          value: _selected.contains(entry.index),
                          title: Text(entry.name),
                          subtitle: Text(entry.engine.label),
                          onChanged: _busy || entry.errorCode != null
                              ? null
                              : (v) => setState(() {
                                  if (v! && _selected.length < 64) {
                                    _selected.add(entry.index);
                                  } else {
                                    _selected.remove(entry.index);
                                  }
                                }),
                        ),
                        Text('${entry.host}:${entry.port}'), // l10n-ignore: Native-vetted host and port.
                        Text(entry.database),
                        Text(entry.username),
                        if (entry.errorCode != null) Text(l.dbInvalid),
                        if (entry.warningCodes.isNotEmpty) Text(l.dbTlsWarning),
                        CheckboxListTile(
                          value: !_nonProduction.contains(entry.index),
                          title: Text(l.dbProduction),
                          onChanged: _busy
                              ? null
                              : (v) => setState(() {
                                  if (v!) {
                                    _nonProduction.remove(entry.index);
                                  } else {
                                    _nonProduction.add(entry.index);
                                  }
                                }),
                        ),
                        if (_choices?.routes.isNotEmpty ?? false)
                          DropdownButtonFormField<int>(
                            isExpanded: true,
                            initialValue: -1,
                            decoration: InputDecoration(labelText: l.dbRoute),
                            items: [
                              DropdownMenuItem(value: -1, child: Text(l.dbDirect)),
                              for (var i = 0; i < _choices!.routes.length; i++)
                                DropdownMenuItem(value: i, child: Text(_choices!.routes[i].label)),
                            ],
                            onChanged: _busy
                                ? null
                                : (i) => setState(
                                    () => _routes[entry.index] = i == null || i < 0
                                        ? const DbRoute()
                                        : _choices!.routes[i].route,
                                  ),
                          ),
                        if (entry.hasPassword)
                          CheckboxListTile(
                            value: !_withoutPassword.contains(entry.index),
                            title: Text(l.dbSavePassword),
                            onChanged: _busy
                                ? null
                                : (v) => setState(() {
                                    if (v!) {
                                      _withoutPassword.remove(entry.index);
                                    } else {
                                      _withoutPassword.add(entry.index);
                                    }
                                  }),
                          ),
                      ],
                    ),
                  ),
                ),
            ],
          ),
        ),
      ),
      secondaryActions: [GlassButton(label: l.commonCancel, onPressed: () => closeDialog<void>(context))],
      primaryAction: GlassButton.prominent(
        key: const ValueKey('db-import-save'),
        label: l.commonSave,
        onPressed: _busy || _expired || _selected.isEmpty ? null : _save,
      ),
    );
  }
}
