import 'dart:convert';

import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/kubernetes/kube_cluster_menu.dart';
import 'package:consolecrypt/kubernetes/kube_import_dialog.dart';
import 'package:consolecrypt/kubernetes/kube_inventory_service.dart';
import 'package:consolecrypt/kubernetes/kube_organization_service.dart';
import 'package:consolecrypt/kubernetes/kube_providers.dart';
import 'package:consolecrypt/kubernetes/kube_service.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

class KubeConnectionEditor extends ConsumerStatefulWidget {
  const KubeConnectionEditor({this.connection, super.key});
  final KubeConnection? connection;
  @override
  ConsumerState<KubeConnectionEditor> createState() => _KubeConnectionEditorState();
}

class _KubeConnectionEditorState extends ConsumerState<KubeConnectionEditor> {
  late final KubeScope _scope = ref.read(kubeScopeProvider);
  final _name = TextEditingController(),
      _server = TextEditingController(),
      _namespace = TextEditingController(text: 'default'),
      _ca = TextEditingController(),
      _token = TextEditingController(),
      _certificate = TextEditingController(),
      _privateKey = TextEditingController(),
      _tags = TextEditingController();
  String? _color;
  String _auth = 'token';
  bool _production = true,
      _allowExec = false,
      _allowForward = false,
      _recordEvents = false,
      _busy = false,
      _failed = false;
  bool get _current => mounted && _scope.unlocked && identical(_scope, ref.read(kubeScopeProvider));
  bool get _valid {
    final uri = Uri.tryParse(_server.text);
    return _name.text.trim().isNotEmpty &&
        _name.text.length <= 253 &&
        RegExp(r'^[a-z0-9]([-a-z0-9]*[a-z0-9])?$').hasMatch(_namespace.text) &&
        _namespace.text.length <= 63 &&
        (widget.connection != null ||
            (uri != null &&
                uri.scheme == 'https' &&
                uri.host.isNotEmpty &&
                uri.userInfo.isEmpty &&
                uri.fragment.isEmpty &&
                uri.query.isEmpty &&
                (_auth == 'token'
                    ? _token.text.trim().isNotEmpty
                    : _certificate.text.trim().isNotEmpty && _privateKey.text.trim().isNotEmpty)));
  }

  @override
  void initState() {
    super.initState();
    final c = widget.connection;
    if (c != null) {
      _name.text = c.name;
      _server.text = c.server;
      _namespace.text = c.namespace;
      _production = c.production;
      _allowExec = c.allowExecInProd;
      _allowForward = c.allowPortForwardInProd;
      _recordEvents = c.recordEvents;
      _tags.text = c.tags.join(', ');
      _color = c.color;
    }
  }

  @override
  void dispose() {
    for (final controller in [_name, _server, _namespace, _ca, _token, _certificate, _privateKey, _tags]) {
      controller.clear();
      controller.dispose();
    }
    super.dispose();
  }

  Future<void> _save() async {
    if (!_current || !_valid || _busy) return;
    setState(() {
      _busy = true;
      _failed = false;
    });
    try {
      final connection = widget.connection;
      if (connection != null) {
        final service = ref.read(kubeServiceProvider);
        if (service is! KubeInventoryManagementService) return;
        await (service as KubeInventoryManagementService).updateContext(
          connection.id,
          name: _name.text.trim(),
          namespace: _namespace.text,
          production: _production,
          allowExecInProd: _allowExec,
          allowPortForwardInProd: _allowForward,
          recordEvents: _recordEvents,
        );
        if (!_current) return;
        if (service is KubeOrganizationService) {
          await (service as KubeOrganizationService).saveKubeClusterLabels(
            connection.id,
            tags: _tags.text.split(',').map((s) => s.trim()).where((s) => s.isNotEmpty).toSet().toList(),
            color: _color,
          );
        }
        if (mounted && _current) {
          ref.invalidate(kubeConnectionsProvider);
          closeDialog(context, true);
        }
      } else {
        final yaml = SecretText(
          jsonEncode({
            'apiVersion': 'v1',
            'kind': 'Config',
            'clusters': [
              {
                'name': _name.text.trim(),
                'cluster': {
                  'server': _server.text,
                  if (_ca.text.trim().isNotEmpty) 'certificate-authority-data': base64Encode(utf8.encode(_ca.text)),
                },
              },
            ],
            'users': [
              {
                'name': _name.text.trim(),
                'user': _auth == 'token'
                    ? {'token': _token.text}
                    : {
                        'client-certificate-data': base64Encode(utf8.encode(_certificate.text)),
                        'client-key-data': base64Encode(utf8.encode(_privateKey.text)),
                      },
              },
            ],
            'contexts': [
              {
                'name': _name.text.trim(),
                'context': {'cluster': _name.text.trim(), 'user': _name.text.trim(), 'namespace': _namespace.text},
              },
            ],
            'current-context': _name.text.trim(),
          }),
        );
        _token.clear();
        _privateKey.clear();
        try {
          final saved = await showAppDialog<bool>(
            context,
            secure: true,
            barrierDismissible: false,
            builder: (_) => KubeImportDialog(initialYaml: yaml),
          );
          if (mounted && _current && saved == true) closeDialog(context, true);
        } finally {
          yaml.wipe();
        }
      }
    } catch (_) {
      if (_current) setState(() => _failed = true);
    } finally {
      if (_current) setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    ref.listen(kubeScopeProvider, (_, next) {
      if (identical(_scope, next)) return;
      _token.clear();
      _privateKey.clear();
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (!mounted) return;
        final route = ModalRoute.of(context);
        if (route?.isActive == true) Navigator.of(context).removeRoute(route!);
      });
    });
    final l = context.l10n;
    final tokens = GlassTokens.of(context);
    Widget section(String title) => Padding(
      padding: const EdgeInsets.only(top: GlassSpacing.s16, bottom: GlassSpacing.s8),
      child: Text(
        title.toUpperCase(),
        style: tokens.typography.caption.copyWith(
          color: tokens.secondaryLabel,
          fontWeight: FontWeight.w600,
          letterSpacing: 0.6,
        ),
      ),
    );
    Widget gap(Widget child) => Padding(
      padding: const EdgeInsets.only(bottom: GlassSpacing.s8),
      child: child,
    );
    Widget toggle(String title, bool value, ValueChanged<bool> onChanged, {String? subtitle, String? key}) =>
        SwitchListTile.adaptive(
          key: key == null ? null : ValueKey(key),
          contentPadding: EdgeInsets.zero,
          dense: true,
          activeTrackColor: tokens.palette.accent,
          title: Text(title, style: tokens.typography.body.copyWith(color: tokens.palette.label)),
          subtitle: subtitle == null
              ? null
              : Text(subtitle, style: tokens.typography.callout.copyWith(color: tokens.secondaryLabel)),
          value: value,
          onChanged: _busy ? null : onChanged,
        );
    Widget field(String key, TextEditingController controller, String label, {bool secret = false, int lines = 1}) =>
        gap(
          TextField(
            key: ValueKey(key),
            controller: controller,
            enabled: !_busy,
            obscureText: secret && lines == 1,
            minLines: lines,
            maxLines: lines == 1 ? 1 : 5,
            autocorrect: false,
            enableSuggestions: false,
            enableIMEPersonalizedLearning: false,
            decoration: InputDecoration(labelText: label),
            onChanged: (_) => setState(() {}),
          ),
        );
    return GlassDialog(
      title: widget.connection == null ? l.kubeNewConnection : l.kubeConnectionSettings,
      width: 750,
      content: ConstrainedBox(
        constraints: const BoxConstraints(maxHeight: 560),
        child: !_current
            ? const SizedBox.shrink()
            : SingleChildScrollView(
                child: Column(
                  mainAxisSize: MainAxisSize.min,
                  crossAxisAlignment: CrossAxisAlignment.stretch,
                  children: [
                    section(l.kubeEditorConnection),
                    field('kube-editor-name', _name, l.kubeName),
                    field('kube-editor-namespace', _namespace, l.kubeNamespace),
                    if (widget.connection == null) ...[
                      field('kube-editor-server', _server, l.kubeServerUrl),
                      field('kube-editor-ca', _ca, l.kubeCertificateAuthority, lines: 3),
                      DropdownButtonFormField<String>(
                        initialValue: _auth,
                        decoration: InputDecoration(labelText: l.kubeAuthentication),
                        items: [
                          DropdownMenuItem(value: 'token', child: Text(l.kubeAuthToken)),
                          DropdownMenuItem(value: 'certificate', child: Text(l.kubeAuthCertificate)),
                        ],
                        onChanged: _busy ? null : (value) => setState(() => _auth = value!),
                      ),
                      if (_auth == 'token')
                        field('kube-editor-token', _token, l.kubeAuthToken, secret: true)
                      else ...[
                        field('kube-editor-certificate', _certificate, l.kubeClientCertificate, lines: 3),
                        field('kube-editor-private-key', _privateKey, l.kubePrivateKey, secret: true, lines: 3),
                      ],
                      Text(l.kubeManualReviewHelp),
                    ] else ...[
                      if (ref.read(kubeServiceProvider) is KubeOrganizationService) ...[
                        section(l.kubeEditorOrganization),
                        field('kube-editor-tags', _tags, l.kubeTags),
                        Text(l.kubeTagsHelp, style: tokens.typography.callout.copyWith(color: tokens.secondaryLabel)),
                        const SizedBox(height: GlassSpacing.s12),
                        Row(
                          key: const ValueKey('kube-editor-color'),
                          children: [
                            Text(l.kubeColor, style: tokens.typography.body.copyWith(color: tokens.palette.label)),
                            const SizedBox(width: GlassSpacing.s12),
                            for (final value in <String?>[
                              null,
                              ...{...kubeClusterColors, ?_color},
                            ])
                              Padding(
                                padding: const EdgeInsets.only(right: 6),
                                child: Tooltip(
                                  message: value ?? l.kubeNoColor,
                                  child: GlassInteractive(
                                    key: ValueKey('kube-editor-color-${value ?? 'none'}'),
                                    semanticLabel: value ?? l.kubeNoColor,
                                    selected: _color == value,
                                    onPressed: _busy ? null : () => setState(() => _color = value),
                                    builder: (context, state) => Container(
                                      width: 26,
                                      height: 26,
                                      decoration: BoxDecoration(
                                        shape: BoxShape.circle,
                                        color: value == null
                                            ? Colors.transparent
                                            : Color(
                                                int.tryParse('FF${value.replaceFirst('#', '')}', radix: 16) ??
                                                    0xFF4285F4,
                                              ),
                                        border: Border.all(
                                          color: _color == value ? tokens.palette.label : tokens.surfaces.separator,
                                          width: _color == value ? 2.5 : 1,
                                        ),
                                      ),
                                      child: value == null
                                          ? Icon(Icons.block_rounded, size: 14, color: tokens.secondaryLabel)
                                          : null,
                                    ),
                                  ),
                                ),
                              ),
                          ],
                        ),
                      ],
                      section(l.kubeEditorSafety),
                      toggle(l.kubeProductionContext, _production, (v) => setState(() => _production = v)),
                      if (_production) ...[
                        toggle(l.kubeAllowExecInProd, _allowExec, (v) => setState(() => _allowExec = v)),
                        toggle(l.kubeAllowForwardInProd, _allowForward, (v) => setState(() => _allowForward = v)),
                      ],
                      toggle(
                        l.kubeRecordEvents,
                        _recordEvents,
                        (v) => setState(() => _recordEvents = v),
                        subtitle: l.kubeRecordEventsHelp,
                      ),
                    ],
                    if (_busy) const LinearProgressIndicator(),
                    if (_failed) Text(l.kubeRequestFailed),
                  ],
                ),
              ),
      ),
      primaryAction: GlassButton.prominent(
        key: const ValueKey('kube-editor-save'),
        label: widget.connection == null ? l.commonContinue : l.commonSave,
        onPressed: !_busy && _valid ? _save : null,
      ),
      secondaryActions: [
        GlassButton(label: l.commonCancel, onPressed: _busy ? null : () => closeDialog(context, false)),
      ],
    );
  }
}
