import 'dart:convert';

import 'package:consolecrypt/automation/automation_awx_models.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/credential.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/monitoring/monitoring_models.dart';
import 'package:flutter/services.dart'
    show TextInputFormatter, TextEditingValue;
import 'package:material_ui/material_ui.dart';

/// Metadata eligibility only; actual credential and original-scope admission
/// remains Native. A password credential's secret is never read by this form.
bool awxCredentialEligible(Credential credential, SourceAuth auth) {
  if (credential.kind != CredentialKind.password || credential.secretId == null) {
    return false;
  }
  if (auth == SourceAuth.bearerRef) return true;
  final username = credential.username;
  return auth == SourceAuth.basicRef &&
      username != null &&
      username.trim().isNotEmpty &&
      !username.contains(':') &&
      !username.runes.any((c) => c < 32 || (c >= 127 && c <= 159));
}

class AwxConnectionForm extends StatefulWidget {
  const AwxConnectionForm({
    super.key,
    required this.original,
    required this.title,
    required this.disclosure,
    required this.httpsNotice,
    required this.credentials,
    required this.credentialsLoading,
    required this.credentialsFailed,
    required this.isCurrent,
    required this.onChanged,
    required this.onCreateCredential,
    required this.onSave,
    required this.onCancel,
  });
  final AwxConnection? original;
  final String title, disclosure, httpsNotice;
  final List<Credential> credentials;
  final bool credentialsLoading, credentialsFailed;
  final bool Function() isCurrent;
  final VoidCallback onChanged;
  final Future<Credential?> Function(SourceAuth auth) onCreateCredential;
  final ValueChanged<MonitoringSource> onSave;
  final VoidCallback onCancel;
  @override
  State<AwxConnectionForm> createState() => _AwxConnectionFormState();
}

class _AwxConnectionFormState extends State<AwxConnectionForm> {
  late final TextEditingController _name, _endpoint, _ca;
  late SourceAuth _auth;
  String? _credential;
  bool _creating = false, _invalid = false;
  int _epoch = 0;
  @override
  void initState() {
    super.initState();
    final c = widget.original?.configuration;
    _name = TextEditingController(text: c?.name ?? '');
    _endpoint = TextEditingController(text: c?.endpoint ?? '');
    _ca = TextEditingController(text: c?.customCaPem ?? '');
    _auth = c?.auth ?? SourceAuth.bearerRef;
    _credential = c?.credentialId;
  }

  List<Credential> get _eligible => widget.credentials
      .where((c) => awxCredentialEligible(c, _auth))
      .toList(growable: false);
  bool get _ready =>
      widget.isCurrent() &&
      !_creating &&
      !widget.credentialsLoading &&
      !widget.credentialsFailed;
  void _changed() {
    _epoch++;
    widget.onChanged();
    if (mounted) setState(() => _invalid = false);
  }

  Future<void> _create() async {
    if (!_ready) return;
    final generation = ++_epoch, auth = _auth;
    setState(() => _creating = true);
    try {
      final created = await widget.onCreateCredential(auth);
      if (!mounted ||
          !widget.isCurrent() ||
          generation != _epoch ||
          _auth != auth ||
          created == null) {
        return;
      }
      // The host refreshed the existing credential stream before returning.
      // Select its safe ref now; Save stays disabled until that SAME ID is
      // present in the latest metadata props (provider rebuild may be next frame).
      if (awxCredentialEligible(created, auth)) {
        _credential = created.id.value;
        widget.onChanged();
        setState(() => _invalid = false);
      }
    } catch (_) {
      if (mounted && widget.isCurrent()) setState(() => _invalid = true);
    } finally {
      if (mounted) setState(() => _creating = false);
    }
  }

  void _save() {
    if (!_ready) return;
    final eligible = _eligible
        .where((c) => c.id.value == _credential)
        .firstOrNull;
    try {
      if (eligible == null) throw const AwxModelException();
      final original = widget.original;
      final config = MonitoringSource(
        id: original?.id ?? '',
        name: _name.text.trim(),
        endpoint: _endpoint.text.trim(),
        route: SourceRoute.direct,
        auth: _auth,
        credentialId: eligible.id.value,
        customCaPem: _ca.text.trim().isEmpty ? null : _ca.text.trim(),
      );
      validateAwxConfiguration(config, create: original == null);
      // Current Native catalog JSON requests are <=64KiB; leave room for its
      // fixed scope/CAS envelope. This form supports a bounded32KiB config.
      if (utf8.encode(jsonEncode(config.toJson())).length > 32768) {
        throw const AwxModelException();
      }
      if (!widget.isCurrent()) return;
      widget.onSave(config);
    } catch (_) {
      setState(() => _invalid = true);
    }
  }

  Widget _field(
    String key,
    TextEditingController controller,
    String label, {
    int lines = 1,
  }) => Padding(
    padding: const EdgeInsets.only(bottom: 12),
    child: TextField(
      key: ValueKey(key),
      controller: controller,
      enabled: widget.isCurrent() && !_creating,
      minLines: lines,
      maxLines: lines == 1 ? 1 : 6,
      inputFormatters: [_AwxConfigTextLimit(lines == 1 ? 4096 : 32768)],
      onChanged: (_) => _changed(),
      decoration: InputDecoration(
        labelText: label,
        border: const OutlineInputBorder(),
      ),
    ),
  );
  @override
  Widget build(BuildContext context) {
    final l = context.l10n, eligible = _eligible;
    return GlassDialog(
      title: widget.title,
      width: 620,
      content: ConstrainedBox(
        constraints: const BoxConstraints(maxHeight: 520),
        child: SingleChildScrollView(
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Text(widget.disclosure),
              const SizedBox(height: 12),
              _field('awx-connection-name', _name, l.monitoringName),
              _field(
                'awx-connection-endpoint',
                _endpoint,
                l.monitoringEndpoint,
              ),
              InfoBanner(message: widget.httpsNotice),
              const SizedBox(height: 12),
              GlassSelect<SourceAuth>(
                key: const ValueKey('awx-connection-auth'),
                value: _auth,
                expand: true,
                items: [
                  GlassSelectItem(
                    value: SourceAuth.bearerRef,
                    label: l.monitoringBearerAuth,
                  ),
                  GlassSelectItem(
                    value: SourceAuth.basicRef,
                    label: l.monitoringBasicAuth,
                  ),
                ],
                onChanged: _ready
                    ? (auth) {
                        _auth = auth;
                        if (!_eligible.any((c) => c.id.value == _credential)) {
                          _credential = null;
                        }
                        _changed();
                      }
                    : null,
              ),
              const SizedBox(height: 12),
              if (widget.credentialsLoading || _creating)
                const LinearProgressIndicator(),
              if (widget.credentialsFailed)
                InfoBanner(
                  message: l.monitoringErrorAuth,
                  tone: BannerTone.danger,
                ),
              GlassSelect<String?>(
                key: const ValueKey('awx-connection-credential'),
                value: _credential,
                expand: true,
                semanticLabel: l.monitoringCredential,
                items: [
                  GlassSelectItem(
                    value: null,
                    label: l.monitoringChooseCredential,
                  ),
                  if (_credential != null &&
                      !eligible.any((c) => c.id.value == _credential))
                    GlassSelectItem(
                      value: _credential,
                      label: l.monitoringMissingCredential,
                    ),
                  for (final c in eligible)
                    GlassSelectItem(
                      value: c.id.value,
                      label: c.username == null
                          ? c.name
                          : '${c.name} · ${c.username}',
                    ),
                ],
                onChanged: _ready
                    ? (id) {
                        _credential = id;
                        _changed();
                      }
                    : null,
              ),
              const SizedBox(height: 8),
              Align(
                alignment: AlignmentDirectional.centerStart,
                child: GlassButton(
                  key: const ValueKey('awx-create-credential'),
                  label: l.monitoringAddCredential,
                  onPressed: _ready ? _create : null,
                ),
              ),
              if (_auth == SourceAuth.basicRef)
                Text(l.monitoringBasicUsernameHelp),
              Text(l.monitoringCredentialHelp),
              const SizedBox(height: 12),
              ExpansionTile(
                title: Text(l.monitoringCustomCa),
                children: [
                  _field('awx-connection-ca', _ca, l.monitoringCaPem, lines: 4),
                ],
              ),
              if (_invalid)
                InfoBanner(
                  key: const ValueKey('awx-connection-invalid'),
                  message: l.monitoringErrorConfig,
                  tone: BannerTone.danger,
                ),
            ],
          ),
        ),
      ),
      primaryAction: GlassButton.prominent(
        key: const ValueKey('awx-connection-save'),
        label: l.commonSave,
        onPressed: _ready && eligible.any((c) => c.id.value == _credential)
            ? _save
            : null,
      ),
      secondaryActions: [
        GlassButton(label: l.commonCancel, onPressed: widget.onCancel),
      ],
    );
  }

  @override
  void dispose() {
    _epoch++;
    _name.dispose();
    _endpoint.dispose();
    _ca.dispose();
    super.dispose();
  }
}

/// Reject oversized edits BEFORE EditableText/grapheme layout. Never silently
/// truncate a URL, name or certificate into a different configuration.
final class _AwxConfigTextLimit extends TextInputFormatter {
  _AwxConfigTextLimit(this.limit);
  final int limit;
  @override
  TextEditingValue formatEditUpdate(
    TextEditingValue oldValue,
    TextEditingValue newValue,
  ) => newValue.text.length > limit ? oldValue : newValue;
}
