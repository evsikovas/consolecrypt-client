import 'dart:async';
import 'dart:convert';

import 'package:consolecrypt/monitoring/metrics_ai_context.dart';
import 'package:consolecrypt/monitoring/metrics_ai_review_flow.dart';
import 'package:consolecrypt/monitoring/monitoring_models.dart';

enum MetricsAiNativeOperation { prepare, send, discard }

typedef MetricsAiNativeRpc = Future<Json> Function(MetricsAiNativeOperation operation, Json request);

/// Optional capability on the existing profile-owned monitoring service.
/// Missing issuer support never falls back to chat or a metrics query.
abstract interface class MetricsAiNativeCapability {
  bool get metricsAiSupported;
  Future<Json> metricsAiRequest(MetricsAiNativeOperation operation, Json request);
}

final class MetricsAiNativeTransport implements MetricsAiReviewTransport {
  MetricsAiNativeTransport({
    required this.capability,
    required this.original,
    required this.profileId,
    required this.source,
    required this.providerId,
    required this.isCurrent,
  }) {
    if (!_id(profileId) ||
        !_id(source.id) ||
        !_id(providerId) ||
        source.readOnly ||
        source.revision <= 0 ||
        source.revision > 9007199254740991 ||
        original.sourceFingerprint != source.fingerprint) {
      throw const MetricsAiFlowException(MetricsAiFlowIssue.invalidInput);
    }
  }

  final MetricsAiNativeCapability capability;
  final MetricsAiFence original;
  final String profileId, providerId;
  final MonitoringSource source;
  final bool Function() isCurrent;
  bool _disposed = false;
  final _tickets = <String>{};

  static bool _id(String text) =>
      text.length == 36 &&
      text != '00000000-0000-0000-0000-000000000000' &&
      RegExp(r'^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$').hasMatch(text);

  void _check() {
    if (_disposed || !isCurrent()) {
      throw const MetricsAiFlowException(MetricsAiFlowIssue.contextChanged);
    }
    if (!capability.metricsAiSupported) {
      throw const MetricsAiFlowException(MetricsAiFlowIssue.transport);
    }
  }

  Future<Json> _call(MetricsAiNativeOperation operation, Json request) async {
    _check();
    final result = await capability.metricsAiRequest(operation, request);
    if (operation == MetricsAiNativeOperation.prepare && (_disposed || !isCurrent())) {
      final ticket = result['ticket'];
      if (ticket is String && ticket.isNotEmpty && ticket.length <= 256) {
        unawaited(_discardOwned(ticket));
      }
    }
    _check();
    return result;
  }

  static void _shape(Json wire, Set<String> fields) {
    if (wire['version'] != 1 ||
        wire.keys.toSet().difference(fields).isNotEmpty ||
        fields.difference(wire.keys.toSet()).isNotEmpty) {
      throw const MetricsAiFlowException(MetricsAiFlowIssue.invalidResponse);
    }
  }

  @override
  Future<MetricsAiPreparedReview> prepare(MetricsAiFence originalFence, MetricsAiRequest request) async {
    _check();
    if (!original.matches(originalFence) || !request.projection.matchesFence(original)) {
      throw const MetricsAiFlowException(MetricsAiFlowIssue.contextChanged);
    }
    final wire = await _call(MetricsAiNativeOperation.prepare, {
      'version': 1,
      'profileId': profileId,
      'sourceId': source.id,
      'sourceRevision': source.revision,
      'providerId': providerId,
      'action': request.action.name,
      'question': request.question,
      'selectedData': jsonDecode(request.projection.dataJson),
    });
    _shape(wire, {'version', 'ticket', 'previewText', 'external', 'expiresUtc'});
    final ticket = wire['ticket'],
        preview = wire['previewText'],
        external = wire['external'],
        expiry = wire['expiresUtc'];
    final expires = expiry is String ? DateTime.tryParse(expiry) : null;
    if (ticket is! String ||
        preview is! String ||
        external is! bool ||
        expires == null ||
        !expires.isUtc ||
        expiry is! String ||
        !expiry.endsWith('Z')) {
      throw const MetricsAiFlowException(MetricsAiFlowIssue.invalidResponse);
    }
    final review = MetricsAiPreparedReview(
      ticket: ticket,
      previewText: preview,
      external: external,
      expiresUtc: expires,
    );
    if (_tickets.length >= 4) {
      final previous = _tickets.first;
      _tickets.remove(previous);
      unawaited(_discardOwned(previous));
    }
    _tickets.add(ticket);
    return review;
  }

  @override
  Future<MetricsAiReadonlyProposal> send(String ticket, {required bool approveExternal}) async {
    final wire = await _call(MetricsAiNativeOperation.send, {
      'version': 1,
      'profileId': profileId,
      'ticket': ticket,
      'approveExternal': approveExternal,
    });
    _shape(wire, {'version', 'explanation', 'proposedPlaybook', 'limitations'});
    final proposal = MetricsAiReadonlyProposal.fromWire({...wire}..remove('version'));
    return proposal;
  }

  @override
  Future<void> discard(String ticket) async {
    if (!_tickets.remove(ticket)) throw const MetricsAiFlowException(MetricsAiFlowIssue.consumed);
    await _discardOwned(ticket);
  }

  Future<void> _discardOwned(String ticket) async {
    try {
      // Discard cannot query, resolve secrets or send to a model. Native checks
      // the ORIGINAL profile and its own ticket registry, including after lock.
      await capability.metricsAiRequest(MetricsAiNativeOperation.discard, {
        'version': 1,
        'profileId': profileId,
        'ticket': ticket,
      });
    } catch (_) {
      // Closing a profile may already have destroyed the original registry.
    }
  }

  void dispose() {
    _disposed = true;
    final tickets = _tickets.toList(growable: false);
    _tickets.clear();
    for (final ticket in tickets) {
      unawaited(_discardOwned(ticket));
    }
  }
}
