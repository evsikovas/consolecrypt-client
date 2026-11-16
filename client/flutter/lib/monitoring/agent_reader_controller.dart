import 'dart:async';
import 'dart:convert';

import 'package:consolecrypt/monitoring/agent_reader_models.dart';
import 'package:consolecrypt/monitoring/agent_reader_port.dart';
import 'package:flutter/foundation.dart';

/// One UI owner and opaque Native action; local countdown never renews Native TTL.
final class AgentReaderController extends ChangeNotifier {
  AgentReaderController({
    required this.port,
    required this.selection,
    required this.isOriginal,
    this.elapsedForReview,
    this.onHeartbeat,
  });

  final void Function(AgentReaderDataset, AgentReaderLocator)? onHeartbeat;
  AgentReaderLocator? _readAssociation;

  /// Local display/test clock only; never sent to Native or used as authority.
  final Duration Function()? elapsedForReview;
  Timer? _countdown;
  final AgentReaderNativePort port;
  final AgentReaderSelection selection;
  final bool Function() isOriginal;
  AgentReaderIssuedHandle? _handle;
  int _generation = 0;
  bool _disposed = false, _retired = false;
  bool busy = false, spent = false, readConsumed = false, unknown = false;
  bool transferAttempted = false, transferCompleted = false;
  String? failure;
  AgentReaderReview? review;
  AgentReaderOutcome? outcome;
  AgentReaderStatus? status;
  AgentReaderDataset? dataset;
  Stopwatch? _clock;
  int get remainingMilliseconds {
    final accepted = review;
    if (accepted == null) return 0;
    final elapsed = (elapsedForReview?.call() ?? _clock?.elapsed ?? Duration.zero).inMilliseconds;
    if (elapsed < 0) return 0;
    return (accepted.remainingMilliseconds - elapsed).clamp(0, accepted.remainingMilliseconds);
  }

  int get remainingSeconds => (remainingMilliseconds + 999) ~/ 1000;
  void _stopCountdown() {
    _countdown?.cancel();
    _countdown = null;
  }

  void _expireReview() {
    _stopCountdown();
    _clock?.stop();
    _cancel(_handle);
    _handle = null;
    review = null;
    failure = 'reader_expired';
    _emit();
  }

  void _scheduleCountdown() {
    _stopCountdown();
    if (!current || review == null) return;
    final remaining = remainingMilliseconds;
    if (remaining <= 0) {
      if (!spent) {
        _expireReview();
      } else {
        _emit();
      }
      return;
    }
    _countdown = Timer(Duration(milliseconds: remaining < 1000 ? remaining : 1000), () {
      _countdown = null;
      if (!current) {
        retire();
        return;
      }
      // A Confirm already dispatched remains owned by its actual Native Future.
      // Local expiry cannot cancel it, fabricate retirement or override its ACK.
      if (remainingMilliseconds <= 0) {
        if (!spent) {
          _expireReview();
        } else {
          _emit();
        }
        return;
      }
      _emit();
      _scheduleCountdown();
    });
  }

  bool get current => !_disposed && !_retired && isOriginal();
  bool get canConfirm => current && !busy && !spent && review != null && remainingMilliseconds > 0;
  void _emit() {
    if (!_disposed) notifyListeners();
  }

  void _cancel(AgentReaderIssuedHandle? handle) {
    try {
      handle?.cancel();
    } catch (_) {
      /* Original local retirement stays synchronous even if the bridge is gone. */
    }
  }

  void cancel() {
    _stopCountdown();
    _generation++;
    _cancel(_handle);
    _handle = null;
    review = null;
    outcome = null;
    dataset = null;
    _readAssociation = null;
    busy = false;
    _clock?.stop();
    _emit();
  }

  void retire() {
    if (_retired) return;
    _retired = true;
    cancel();
    status = null;
    failure = null;
    _emit();
  }

  bool _same(int epoch) {
    if (!current) {
      retire();
      return false;
    }
    return epoch == _generation;
  }

  Future<void> prepare(AgentReaderRequest request) async {
    if (!current || busy || spent) return;
    cancel();
    failure = null;
    unknown = false;
    final epoch = _generation;
    busy = true;
    _emit();
    AgentReaderIssuedHandle? issued;
    try {
      issued = await port.prepareAction(request.encode(selection));
      if (!_same(epoch)) {
        _cancel(issued);
        return;
      }
      final parsed = AgentReaderReview.parse(issued.details(), selection, request.kind);
      if (!_same(epoch)) {
        _cancel(issued);
        return;
      }
      _handle = issued;
      if (request.kind == AgentReaderAction.read) {
        final locator = readerMap(request.action['association']);
        _readAssociation = AgentReaderLocator(
          readerHash(locator['peerDigest']),
          readerHash(locator['associationDigest']),
        );
      }
      review = parsed;
      _clock = Stopwatch()..start();
      _scheduleCountdown();
    } catch (error) {
      _cancel(issued);
      if (_same(epoch)) failure = error is AgentReaderException ? error.code : 'reader_response_invalid';
    } finally {
      if (_same(epoch)) {
        busy = false;
        _emit();
      }
    }
  }

  Future<void> confirm() async {
    if (!canConfirm) {
      if (current && !busy && !spent && review != null) {
        _expireReview();
      }
      return;
    }
    final handle = _handle!, original = review!, epoch = _generation;
    spent = true;
    busy = true;
    failure = null;
    _emit();
    try {
      final raw = await handle.confirm();
      if (!_same(epoch)) return;
      outcome = AgentReaderOutcome.parse(raw, original.kind);
    } catch (_) {
      if (_same(epoch)) {
        unknown = true;
        failure = 'reader_outcome_unknown';
      }
    } finally {
      if (_same(epoch)) {
        busy = false;
        _emit();
      }
    }
  }

  Future<void> consumeRead() async {
    final accepted = outcome, handle = _handle;
    if (!current || busy || readConsumed || accepted?.kind != 'readReady' || handle == null) return;
    final epoch = _generation;
    readConsumed = true;
    busy = true;
    _emit();
    try {
      final raw = await handle.consumeRead();
      if (_same(epoch)) {
        final parsed = AgentReaderDataset.parse(raw, accepted!);
        dataset = parsed;
        final association = _readAssociation;
        if (parsed.kind == 'heartbeatV1' && association != null) onHeartbeat?.call(parsed, association);
      }
    } catch (error) {
      if (_same(epoch)) failure = error is AgentReaderException ? error.code : 'reader_response_invalid';
    } finally {
      if (_same(epoch)) {
        busy = false;
        _emit();
      }
    }
  }

  Future<void> readStatus({AgentReaderLocator? locator}) async {
    if (!current || busy) return;
    final epoch = _generation;
    busy = true;
    failure = null;
    _emit();
    try {
      final raw = await port.status(
        jsonEncode({
          'notificationScopeId': selection.notificationScopeId,
          'selection': selection.json,
          'association': locator?.json,
        }),
      );
      if (_same(epoch)) status = AgentReaderStatus.parse(raw, selection);
    } catch (_) {
      if (_same(epoch)) failure = 'reader_unavailable';
    } finally {
      if (_same(epoch)) {
        busy = false;
        _emit();
      }
    }
  }

  Future<void> transfer({required String publicDestination, String? invitationDestination}) async {
    final handle = _handle, result = outcome;
    if (!current || busy || handle == null || result?.publicPath == null || !spent || transferAttempted) return;
    final epoch = _generation;
    transferAttempted = true;
    busy = true;
    _emit();
    try {
      final raw = await handle.transferExport(
        publicDestination: publicDestination,
        invitationDestination: result!.hasInvitation ? invitationDestination : null,
      );
      if (!_same(epoch)) return;
      final json = readerDecode(raw);
      readerKeys(json, {
        'kind',
        'publicDestination',
        'invitationDestination',
        'publicSha256',
        'publicBytes',
        'invitationBytes',
        'actorApply',
        'copiesIrreversible',
      });
      if (json['kind'] != 'nodeExportTransferred' ||
          json['publicDestination'] != publicDestination ||
          json['invitationDestination'] != (result.hasInvitation ? invitationDestination : null) ||
          json['actorApply'] != 'notObserved' ||
          json['copiesIrreversible'] != true) {
        readerInvalid();
      }
      readerHash(json['publicSha256']);
      readerInteger(json['publicBytes'], min: 1, max: 16384);
      if (result.hasInvitation) {
        readerInteger(json['invitationBytes'], min: 1, max: 16384);
      } else if (json['invitationBytes'] != null) {
        readerInvalid();
      }
      transferCompleted = true;
    } catch (_) {
      if (_same(epoch)) {
        unknown = true;
        failure = 'reader_outcome_unknown';
      }
    } finally {
      if (_same(epoch)) {
        busy = false;
        _emit();
      }
    }
  }

  @override
  void dispose() {
    if (_disposed) return;
    _stopCountdown();
    _disposed = true;
    _generation++;
    _cancel(_handle);
    _handle = null;
    dataset = null;
    review = null;
    status = null;
    outcome = null;
    super.dispose();
  }
}
