import 'dart:convert';

import 'package:consolecrypt/automation/automation_project_metadata.dart';
import 'package:consolecrypt/automation/automation_service.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:flutter/foundation.dart';
import 'package:material_ui/material_ui.dart';

class AutomationProjectSettingsSyncDialog extends StatefulWidget {
  const AutomationProjectSettingsSyncDialog({
    super.key,
    required this.service,
    required this.profileId,
    required this.nonce,
    required this.originalLive,
  });

  final AutomationProjectSettingsSyncService service;
  final String profileId, nonce;
  final ValueListenable<bool> originalLive;

  @override
  State<AutomationProjectSettingsSyncDialog> createState() => _AutomationProjectSettingsSyncDialogState();
}

class _AutomationProjectSettingsSyncDialogState extends State<AutomationProjectSettingsSyncDialog> {
  ProjectSettingsReview? _review;
  AutomationJson? _details;
  bool _busy = true;
  bool _failed = false;
  bool _enabled = false;

  @override
  void initState() {
    super.initState();
    widget.originalLive.addListener(_onScopeChanged);
    _load();
  }

  void _onScopeChanged() {
    if (widget.originalLive.value) return;
    _review?.cancel();
    _review = null;
    if (mounted) {
      setState(() {
        _busy = false;
        _details = null;
        _failed = true;
        _enabled = false;
      });
    }
  }

  AutomationJson _scoped(String raw) {
    final value = jsonDecode(raw);
    if (value is! AutomationJson ||
        value['version'] != 1 ||
        value['scope'] != 'allPersonalVaults' ||
        value['profileId'] != widget.profileId ||
        value['notificationScopeId'] != widget.nonce) {
      throw const FormatException('Project settings sync scope changed');
    }
    return value;
  }

  Future<void> _load() async {
    try {
      final review = await widget.service.prepareProjectSettingsSync(widget.profileId, widget.nonce);
      if (!mounted || !widget.originalLive.value) {
        review.cancel();
        return;
      }
      _review = review;
      final details = _scoped(await review.details());
      if (mounted && widget.originalLive.value) setState(() => _details = details);
    } catch (_) {
      if (mounted) setState(() => _failed = true);
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  Future<void> _apply({required bool confirm}) async {
    final review = _review;
    if (review == null || _busy || !widget.originalLive.value) return;
    setState(() => _busy = true);
    try {
      final value = _scoped(await (confirm ? review.confirm() : review.recheckCheckpoint()));
      final expected = confirm ? 'accountPolicyConfirmed' : 'checkpointRecaptured';
      if (value['action'] != expected) throw const FormatException('Unexpected settings sync result');
      if (mounted && widget.originalLive.value) setState(() => _enabled = true);
    } catch (_) {
      if (mounted) setState(() => _failed = true);
    } finally {
      review.cancel();
      _review = null;
      if (mounted) setState(() => _busy = false);
    }
  }

  @override
  void dispose() {
    widget.originalLive.removeListener(_onScopeChanged);
    _review?.cancel();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    final details = _details;
    final minimum = details?['releasedMinimumClientVersion'];
    final confirm = details?['canConfirm'] == true && minimum is String;
    final recheck = details?['canRecheck'] == true;
    final readiness = details?['readiness'];
    final olderDevices = readiness is AutomationJson && readiness['state'] == 'devicesUnsupported';
    final message = switch ((_failed, _enabled, confirm, recheck, olderDevices)) {
      (true, _, _, _, _) => l.automationSettingsSyncUnavailable,
      (_, true, _, _, _) => l.automationSettingsSyncEnabled,
      (_, _, true, _, _) => l.automationSettingsSyncConfirmHint(minimum as String),
      (_, _, _, true, _) => l.automationSettingsSyncRefreshHint,
      (_, _, _, _, true) => l.automationSettingsSyncUpdateDevices,
      _ => l.automationSettingsSyncUnavailable,
    };
    return PopScope(
      canPop: !_busy,
      child: GlassDialog(
        title: l.automationSettingsSyncSetup,
        width: 500,
        content: _busy ? const LinearProgressIndicator() : Text(message),
        secondaryActions: [GlassButton(label: l.commonClose, onPressed: _busy ? null : () => Navigator.pop(context))],
        primaryAction: !_busy && !_failed && !_enabled && (confirm || recheck)
            ? GlassButton(
                label: confirm ? l.automationSettingsSyncEnable : l.automationMetadataRefresh,
                onPressed: () => _apply(confirm: confirm),
              )
            : null,
      ),
    );
  }
}
