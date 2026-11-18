import 'package:consolecrypt/automation/automation_portable_package.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/services/file_dialog_service.dart';
import 'package:flutter/foundation.dart';

enum PortablePhase { idle, loading, selecting, working, exported, inspected, cancelled, failed }

/// Owned by one dialog, not a provider/global registry. Native still performs
/// independent original-profile/nonce/SQL/source checks. Closing cancels the
/// bound Native operation and wipes caller buffers; no path deletion is done.
final class AutomationPortableController extends ChangeNotifier {
  AutomationPortableController({
    required this.scope,
    required this.service,
    required this.files,
    required this.isOriginalCurrent,
    this.projectId,
    this.projectRevision,
    this.originalLifetime,
  }) {
    originalLifetime?.addListener(_lifetimeChanged);
  }
  final Listenable? originalLifetime;
  void _lifetimeChanged() {
    if (!current) cancel();
  }

  final PortableScope scope;
  final AutomationPortablePackageService service;
  final FileDialogService files;
  final bool Function() isOriginalCurrent;
  final String? projectId;
  final int? projectRevision;
  PortablePhase phase = PortablePhase.idle;
  PortableContext? review;
  PortableImportPreview? imported;
  PortableExportOutcome? exported;
  String? errorCode;
  final Set<String> _selected = {};
  Set<String> get selected => Set.unmodifiable(_selected);
  bool _disposed = false;
  int _epoch = 0;
  PortableCancellation? _operation;
  SecretText? _password, _confirmation;
  bool get current => !_disposed && isOriginalCurrent();
  bool get busy => phase == PortablePhase.loading || phase == PortablePhase.working;
  bool _valid(int token) => current && token == _epoch;
  void _emit() {
    if (!_disposed) notifyListeners();
  }

  void select(String path, bool selected) {
    if (!current || busy || review?.choices.any((e) => e.path == path && e.selectable) != true) return;
    if (selected) {
      _selected.add(path);
    } else {
      _selected.remove(path);
    }
    _emit();
  }

  void selectAll() {
    if (!current || busy || review == null) return;
    _selectAvailable(review!);
    _emit();
  }

  void _selectAvailable(PortableContext context) {
    _selected
      ..clear()
      ..addAll(context.choices.where((choice) => choice.selectable).map((choice) => choice.path));
  }

  void _failed() {
    imported = null;
    exported = null;
    errorCode = 'portable_operation_failed';
    phase = PortablePhase.failed;
    _emit();
  }

  PortableCancellation _begin(PortablePhase state) {
    _operation?.cancel();
    _epoch++;
    final op = PortableCancellation();
    _operation = op;
    phase = state;
    errorCode = null;
    imported = null;
    exported = null;
    _emit();
    return op;
  }

  Future<void> load() async {
    if (!current || busy || projectId == null || projectRevision == null) return;
    review = null;
    _selected.clear();
    final op = _begin(PortablePhase.loading), token = _epoch;
    try {
      final value = await service.context(scope, projectId!, projectRevision!, op);
      if (!_valid(token)) return;
      if (!value.scope.same(scope) || value.projectId != projectId || value.projectRevision != projectRevision) {
        throw const PortablePackageFailure('portable_context_changed');
      }
      review = value;
      _selectAvailable(value);
      phase = PortablePhase.selecting;
      _emit();
    } catch (_) {
      if (_valid(token)) _failed();
    } finally {
      op.unbind();
      if (identical(_operation, op)) _operation = null;
    }
  }

  Future<void> export(SecretText password, SecretText confirmation) async {
    try {
      if (!current || busy) return;
      _password = password;
      _confirmation = confirmation;
      if (review == null ||
          password.isEmpty ||
          password.byteLength > 4096 ||
          !password.constantTimeEquals(confirmation)) {
        _failed();
        return;
      }
      late final Map<String, dynamic> request;
      try {
        request = review!.selection(_selected);
      } on PortablePackageFailure {
        _failed();
        return;
      }
      final op = _begin(PortablePhase.working), token = _epoch;
      try {
        final destination = await files.chooseSaveFile(
          suggestedName: 'project.$portablePackageExtension',
          extensions: const [portablePackageExtension],
        );
        if (destination == null) {
          if (_valid(token)) {
            phase = PortablePhase.selecting;
            _emit();
          }
          return;
        }
        if (!_valid(token)) return;
        if (!portablePackageFilename(destination)) throw const PortablePackageFailure('portable_extension_invalid');
        final value = await service.export(request, password, destination, op);
        password.wipe();
        confirmation.wipe();
        if (!_valid(token)) return;
        // Mobile completion owns its platform picker; never delete the user's
        // selected pathname on cancellation/lock/failure or a late result.
        final finished = await files.finishSaveFile(destination);
        if (!_valid(token) || !finished) return;
        exported = value;
        phase = PortablePhase.exported;
        _emit();
      } catch (_) {
        if (_valid(token)) _failed();
      } finally {
        op.unbind();
        if (identical(_operation, op)) _operation = null;
        if (_valid(token) && phase == PortablePhase.working) {
          phase = PortablePhase.selecting;
          _emit();
        }
      }
    } finally {
      password.wipe();
      confirmation.wipe();
      if (identical(_password, password)) _password = null;
      if (identical(_confirmation, confirmation)) _confirmation = null;
    }
  }

  Future<void> inspect(SecretText password) async {
    try {
      if (!current || busy) return;
      _password = password;
      if (password.isEmpty || password.byteLength > 4096) {
        _failed();
        return;
      }
      final op = _begin(PortablePhase.working), token = _epoch;
      try {
        final source = await files.chooseOpenFile(extensions: const [portablePackageExtension]);
        if (source == null) {
          if (_valid(token)) {
            phase = PortablePhase.idle;
            _emit();
          }
          return;
        }
        if (!_valid(token)) return;
        if (!portablePackageFilename(source)) throw const PortablePackageFailure('portable_extension_invalid');
        final value = await service.inspect(scope, source, password, op);
        if (!_valid(token)) return;
        imported = value;
        phase = PortablePhase.inspected;
        _emit();
      } catch (_) {
        if (_valid(token)) _failed();
      } finally {
        op.unbind();
        if (identical(_operation, op)) _operation = null;
        if (_valid(token) && phase == PortablePhase.working) {
          phase = PortablePhase.idle;
          _emit();
        }
      }
    } finally {
      password.wipe();
      if (identical(_password, password)) _password = null;
    }
  }

  void cancel({bool notify = true}) {
    _epoch++;
    _operation?.cancel();
    _password?.wipe();
    _confirmation?.wipe();
    phase = PortablePhase.cancelled;
    imported = null;
    exported = null;
    if (notify) _emit();
  }

  @override
  void dispose() {
    originalLifetime?.removeListener(_lifetimeChanged);
    _disposed = true;
    cancel(notify: false);
    _selected.clear();
    review = null;
    imported = null;
    exported = null;
    super.dispose();
  }
}
