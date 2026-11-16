import 'dart:async';
import 'dart:convert';

import 'package:consolecrypt/automation/automation_portable_import_session.dart';
import 'package:consolecrypt/automation/automation_portable_lifetime.dart';
import 'package:consolecrypt/automation/automation_portable_package.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/services/file_dialog_service.dart';
import 'package:flutter/foundation.dart';

enum PortableImportPhase {
  idle,
  picking,
  preparing,
  ready,
  accepting,
  retiring,
  installed,
  cancelled,
  expired,
  failed,
  recoveryRequired,
}

final class _OriginalRegistration {
  _OriginalRegistration(this.native) {
    try {
      joined = native.joined.then<bool>(
        (_) {
          joinSeen = true;
          return true;
        },
        onError: (Object _) {
          joinSeen = true;
          joinFailed = true;
          return false;
        },
      );
    } catch (_) {
      // An unexpected synchronous transport failure is not actual retirement.
      joinSeen = true;
      joinFailed = true;
      joined = Future<bool>.value(false);
      cancel();
    }
  }
  final PortableImportRegistration native;
  late final Future<bool> joined;
  PortableImportReviewSession? session;
  bool joinSeen = false, joinFailed = false, accepted = false;
  void cancel() {
    try {
      native.cancel();
    } catch (_) {
      /* actual joined is independent */
    }
  }
}

/// One UI operation, not a Native registry/approval. The host captures original
/// scope/AppServices/service/project/route/hub epoch BEFORE picker/dirty awaits.
/// This controller never recaptures a nonce or auto-prepares on mount/changes.
final class AutomationPortableImportController extends ChangeNotifier {
  AutomationPortableImportController({
    required this.scope,
    required this.gateway,
    required this.files,
    required this.lifetime,
    required this.isOriginalCurrent,
  }) {
    lifetime.addListener(_ownerChanged);
  }
  final PortableScope scope;
  final PortableImportReviewGateway? gateway;
  final FileDialogService files;
  final PortableUiLifetime lifetime;
  final bool Function() isOriginalCurrent;
  PortableImportPhase phase = PortableImportPhase.idle;
  PortableImportIssue? issue;
  PortableImportReady? ready;
  PortableInstalledProject? installed;
  PortableImportReady? _acceptedSnapshot;
  PortableImportReady? get attemptedReview => _acceptedSnapshot;
  String _name = '', _source = '';
  String get requestedName => _name;
  String get selectedSource => _source;
  SecretText? _password;
  _OriginalRegistration? _operation;
  Future<bool>? _retiring;
  int _epoch = 0;
  bool _disposed = false, _recovery = false, _unprovenRetirement = false;
  bool get available => gateway != null;
  bool get current => !_disposed && lifetime.current && isOriginalCurrent() && scope.profileId == lifetime.profileId;
  bool get busy => const {
    PortableImportPhase.picking,
    PortableImportPhase.preparing,
    PortableImportPhase.accepting,
    PortableImportPhase.retiring,
  }.contains(phase);
  bool get canPrepare =>
      current &&
      gateway != null &&
      !busy &&
      phase != PortableImportPhase.installed &&
      !_recovery &&
      !_unprovenRetirement &&
      _source.isNotEmpty &&
      _name.isNotEmpty;
  bool get canAccept =>
      current &&
      phase == PortableImportPhase.ready &&
      !_recovery &&
      ready?.publicationSupported == true &&
      _operation != null &&
      !_operation!.joinSeen &&
      !_operation!.accepted;
  bool _valid(int token) => current && token == _epoch;
  void _emit() {
    if (!_disposed) notifyListeners();
  }

  void _wipe() {
    _password?.wipe();
    _password = null;
  }

  void _ownerChanged() {
    if (!current) cancel();
  }

  void setRequestedName(String value) {
    if (value == _name) return;
    final bounded =
        value.length <= portableImportMaxRawNameBytes && utf8.encode(value).length <= portableImportMaxRawNameBytes;
    _name = bounded ? value : '';
    _inputChanged();
    if (!bounded && !_recovery && !_disposed) {
      issue = PortableImportIssue.invalidInput;
      _emit();
    }
  }

  void clearSource() {
    if (_source.isEmpty) return;
    _source = '';
    _inputChanged();
  }

  void editInputs() {
    if (current && !busy && !_recovery) _inputChanged();
  }

  void passwordEdited() {
    _inputChanged();
  }

  void _inputChanged() {
    if (_disposed) return;
    if (phase == PortableImportPhase.accepting) {
      cancel();
      return;
    }
    _epoch++;
    _wipe();
    ready = null;
    installed = null;
    if (_recovery) {
      _emit();
      return;
    }
    _acceptedSnapshot = null;
    issue = null;
    phase = PortableImportPhase.idle;
    final old = _operation;
    _operation = null;
    if (old != null) {
      _retire(old);
    } else {
      if (_retiring != null) phase = PortableImportPhase.retiring;
      _emit();
    }
  }

  void _retire(_OriginalRegistration old) {
    old.cancel();
    _retiring = old.joined;
    phase = PortableImportPhase.retiring;
    unawaited(
      old.joined.then<void>((joined) {
        if (!joined) _unprovenRetirement = true;
        if (_disposed) return;
        final originalCleanup = identical(_retiring, old.joined);
        if (originalCleanup) _retiring = null;
        if (originalCleanup && current && !_recovery && _operation == null && phase == PortableImportPhase.retiring) {
          phase = joined ? PortableImportPhase.idle : PortableImportPhase.failed;
          issue = joined ? null : PortableImportIssue.failed;
          _emit();
        }
      }),
    );
    _emit();
  }

  Future<void> chooseSource() async {
    if (!current || busy || _recovery || _unprovenRetirement) return;
    if (gateway == null) {
      issue = PortableImportIssue.unavailable;
      phase = PortableImportPhase.failed;
      _emit();
      return;
    }
    final token = ++_epoch;
    ready = null;
    installed = null;
    issue = null;
    _wipe();
    final old = _operation;
    _operation = null;
    if (old != null) {
      _retire(old);
      final retired = await old.joined;
      if (!retired) _unprovenRetirement = true;
      if (!_valid(token)) return;
      if (!retired) {
        phase = PortableImportPhase.failed;
        issue = PortableImportIssue.failed;
        _emit();
        return;
      }
    }
    if (!_valid(token)) return;
    phase = PortableImportPhase.picking;
    _emit();
    try {
      final path = await files.chooseOpenFile(extensions: const [portablePackageExtension]);
      if (!_valid(token)) return;
      if (path == null) {
        phase = PortableImportPhase.idle;
        _emit();
        return;
      }
      if (!portablePackageFilename(path)) throw const PortableImportFailure(PortableImportIssue.invalidInput);
      _source = path;
      phase = PortableImportPhase.idle;
      _emit();
    } catch (error) {
      if (_valid(token)) {
        phase = PortableImportPhase.failed;
        issue = _safe(error);
        _emit();
      }
    }
  }

  Future<void> prepare(SecretText password) async {
    int? ownerToken;
    _OriginalRegistration? owned, previous;
    try {
      if (gateway == null && current) {
        issue = PortableImportIssue.unavailable;
        phase = PortableImportPhase.failed;
        _emit();
        return;
      }
      if (!canPrepare) return;
      final token = ++_epoch;
      ownerToken = token;
      previous = _operation;
      _operation = null;
      if (previous != null) _retire(previous);
      ready = null;
      installed = null;
      issue = null;
      phase = PortableImportPhase.preparing;
      _password = password;
      _emit();
      final intent = PortableImportIntent.capture(scope: scope, source: _source, requestedName: _name);
      if (password.isWiped || password.byteLength == 0 || password.byteLength > 4096) {
        throw const PortableImportFailure(PortableImportIssue.invalidInput);
      }
      if (previous != null) {
        final retired = await previous.joined;
        if (!retired) _unprovenRetirement = true;
        if (!_valid(token)) return;
        if (!retired) throw const PortableImportFailure(PortableImportIssue.failed);
      }
      final retiring = _retiring;
      if (retiring != null) {
        if (!await retiring) throw const PortableImportFailure(PortableImportIssue.failed);
        if (!_valid(token)) return;
      }
      if (!_valid(token)) return;
      // Actual registration owns cancel/secret transfer synchronously BEFORE
      // our first wait for its ACK. No await-joined is used to obtain Ready.
      final op = _OriginalRegistration(gateway!.register(intent: intent, password: password));
      owned = op;
      _operation = op;
      password.wipe();
      _password = null;
      if (!_valid(token)) op.cancel();
      unawaited(_observeReadyRetirement(op, token));
      PortableImportReviewSession session;
      try {
        session = await op.native.registered;
      } catch (error) {
        op.cancel();
        await op.joined;
        rethrow;
      }
      op.session = session;
      if (!_valid(token)) {
        op.cancel();
        await op.joined;
        return;
      }
      if (op.joinSeen) {
        throw PortableImportFailure(op.joinFailed ? PortableImportIssue.failed : PortableImportIssue.expired);
      }
      final raw = await session.preview();
      if (!_valid(token)) {
        op.cancel();
        await op.joined;
        return;
      }
      if (op.joinSeen) {
        throw PortableImportFailure(op.joinFailed ? PortableImportIssue.failed : PortableImportIssue.expired);
      }
      final reviewed = PortableImportReady.fromJson(portableImportDecode(raw), intent: intent);
      if (!_valid(token) || op.joinSeen) {
        op.cancel();
        await op.joined;
        return;
      }
      ready = reviewed;
      phase = PortableImportPhase.ready;
      _emit();
    } catch (error) {
      final original = owned ?? previous;
      if (original != null) {
        original.cancel();
        final joined = await original.joined;
        if (!joined) _unprovenRetirement = true;
      }
      if (ownerToken != null && _valid(ownerToken) && !_recovery && phase == PortableImportPhase.preparing) {
        ready = null;
        installed = null;
        issue = _safe(error);
        phase = issue == PortableImportIssue.expired ? PortableImportPhase.expired : PortableImportPhase.failed;
        _emit();
      }
    } finally {
      password.wipe();
      if (identical(_password, password)) _password = null;
    }
  }

  Future<void> _observeReadyRetirement(_OriginalRegistration op, int token) async {
    final joined = await op.joined;
    if (!_valid(token) || !identical(_operation, op)) return;
    if (phase == PortableImportPhase.ready) {
      ready = null;
      issue = joined ? PortableImportIssue.expired : PortableImportIssue.failed;
      phase = joined ? PortableImportPhase.expired : PortableImportPhase.failed;
      if (!joined) _unprovenRetirement = true;
      _emit();
    }
  }

  Future<void> accept() async {
    if (!canAccept) return;
    final token = _epoch, reviewed = ready!, op = _operation!;
    op.accepted = true;
    _acceptedSnapshot = reviewed;
    phase = PortableImportPhase.accepting;
    issue = null;
    _emit();
    Object? failure;
    String? response;
    try {
      response = await op.session!.accept(reviewId: reviewed.reviewId, reviewDigest: reviewed.reviewDigest);
    } catch (error) {
      failure = error;
      op.cancel();
    }
    final joined = await op.joined;
    if (!_valid(token)) return;
    if (!joined || failure != null) {
      final reason = failure == null ? PortableImportIssue.recoveryRequired : _safe(failure);
      ready = null;
      installed = null;
      // Only the six EXACT fixed Native pre-durable refusals are known to
      // precede mutation. Generic unavailable/cancel/transport is NOT that fact.
      const preDurable = {
        PortableImportIssue.changed,
        PortableImportIssue.failed,
        PortableImportIssue.sourceChanged,
        PortableImportIssue.unsupported,
        PortableImportIssue.busy,
        PortableImportIssue.expired,
      };
      if (!joined || failure is! PortableImportFailure || !preDurable.contains(reason)) {
        _requireRecovery();
      } else {
        issue = reason;
        phase = reason == PortableImportIssue.expired ? PortableImportPhase.expired : PortableImportPhase.failed;
        _emit();
      }
      return;
    }
    try {
      final result = PortableInstalledProject.fromJson(portableImportDecode(response!), reviewed: reviewed);
      if (!_valid(token)) return;
      ready = null;
      installed = result;
      phase = PortableImportPhase.installed;
      _operation = null;
      _emit();
    } catch (_) {
      _requireRecovery();
    }
  }

  PortableImportIssue _safe(Object error) => error is PortableImportFailure ? error.issue : PortableImportIssue.failed;
  void _requireRecovery() {
    _recovery = true;
    ready = null;
    installed = null;
    issue = PortableImportIssue.recoveryRequired;
    phase = PortableImportPhase.recoveryRequired;
    _emit();
  }

  void cancel({bool notify = true}) {
    if (_disposed) return;
    final accepting = phase == PortableImportPhase.accepting;
    _epoch++;
    _wipe();
    _operation?.cancel();
    ready = null;
    installed = null;
    if (accepting || _recovery) {
      _recovery = true;
      issue = PortableImportIssue.recoveryRequired;
      phase = PortableImportPhase.recoveryRequired;
    } else {
      issue = PortableImportIssue.cancelled;
      phase = PortableImportPhase.cancelled;
    }
    if (notify) _emit();
  }

  @override
  void dispose() {
    if (_disposed) return;
    cancel(notify: false);
    _disposed = true;
    _acceptedSnapshot = null;
    _source = '';
    _name = '';
    lifetime.removeListener(_ownerChanged);
    super.dispose();
  }
}
