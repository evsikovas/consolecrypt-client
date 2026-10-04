import 'dart:typed_data';

import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/core/widgets/secret_field.dart';
import 'package:consolecrypt/hosts/connection_picker.dart';
import 'package:consolecrypt/rdp/rdp_clipboard.dart';
import 'package:consolecrypt/rdp/rdp_controller.dart';
import 'package:consolecrypt/rdp/rdp_fullscreen.dart';
import 'package:consolecrypt/rdp/rdp_guard.dart';
import 'package:consolecrypt/rdp/rdp_permissions.dart';
import 'package:consolecrypt/rdp/rdp_providers.dart';
import 'package:consolecrypt/rdp/rdp_service.dart';
import 'package:consolecrypt/rdp/rdp_view.dart';
import 'package:consolecrypt/rdp/rdp_window.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

String _failureLabel(AppLocalizations l, String? code) => switch (code) {
  'certificate_mismatch' => l.rdpCertificateChanged,
  'authentication' => l.rdpAuthenticationFailed,
  'resize_unavailable' => l.rdpResizeUnavailable,
  _ => l.rdpConnectionFailed,
};

class RdpScreen extends ConsumerStatefulWidget {
  const RdpScreen({super.key});
  @override
  ConsumerState<RdpScreen> createState() => _RdpScreenState();
}

class _RdpScreenState extends ConsumerState<RdpScreen> {
  bool _fullscreen = false;
  RdpWorkspaceController? _controller;

  Future<void> _newConnection() => showConnectionPicker(context, ref, protocol: HostProtocol.rdp);

  Future<void> _openFullscreen() async {
    if (_fullscreen) return;
    final scope = ref.read(rdpScopeProvider);
    final controller = ref.read(rdpWorkspaceProvider);
    final tab = controller.active;
    if (!scope.unlocked || tab == null || tab.closed) return;
    final window = ref.read(rdpWindowProvider);
    final lease = window.acquire();
    if (lease == null) {
      showSnack(context, context.l10n.rdpFullscreenFailed, error: true);
      return;
    }
    controller.cancelInteraction(tab);
    setState(() => _fullscreen = true);
    var minimize = false;
    try {
      await lease.enter();
      if (!mounted || !rdpScopeCurrent(ref, scope) || controller.active == null) return;
      minimize =
          await Navigator.of(context, rootNavigator: true).push<bool>(
            PageRouteBuilder<bool>(
              settings: const RouteSettings(name: 'rdp-fullscreen'),
              transitionDuration: Duration.zero,
              reverseTransitionDuration: Duration.zero,
              pageBuilder: (_, _, _) => RdpFullscreen(scope: scope, controller: controller, window: window),
            ),
          ) ==
          true;
    } catch (_) {
      if (mounted && rdpScopeCurrent(ref, scope)) showSnack(context, context.l10n.rdpFullscreenFailed, error: true);
    } finally {
      // Includes lock/profile disposal and a native entry that finishes late.
      // The native lease restores the original placement, including maximized.
      try {
        await lease.restore(minimize: minimize);
      } catch (_) {
        if (mounted && rdpScopeCurrent(ref, scope)) showSnack(context, context.l10n.rdpFullscreenFailed, error: true);
      }
      if (mounted) setState(() => _fullscreen = false);
    }
  }

  @override
  void dispose() {
    // Indexed-shell navigation retains this screen; actual route/profile
    // teardown must not retain a remote desktop in another workspace.
    _controller?.closeAll().ignore();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final scope = ref.watch(rdpScopeProvider);
    final controller = ref.watch(rdpWorkspaceProvider);
    final visible = TickerMode.valuesOf(context).enabled;
    controller.setVisible(visible || _fullscreen);
    _controller = controller;
    final l = context.l10n;
    if (!scope.unlocked) return Center(child: Text(l.rdpLocked));
    if (_fullscreen) return const SizedBox.shrink();
    return AnimatedBuilder(
      animation: controller,
      builder: (context, _) {
        final active = controller.active;
        final body = Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            if (controller.tabs.isNotEmpty) ...[
              GlassTabStrip(
                tabs: [
                  for (final tab in controller.tabs)
                    GlassTab(
                      key: ValueKey('rdp-tab-${tab.info.id}'),
                      title: tab.title,
                      subtitle: tab.options.username,
                      tooltip: tab.options.endpoint,
                      state: switch (tab.status.phase) {
                        RdpPhase.connected => GlassTabState.connected,
                        RdpPhase.connecting => GlassTabState.reconnecting,
                        _ => GlassTabState.disconnected,
                      },
                    ),
                ],
                activeIndex: active == null ? null : controller.tabs.indexOf(active),
                onSelect: (index) => controller.activate(controller.tabs[index]),
                onClose: (index) => controller.close(controller.tabs[index]),
                onAdd: controller.canConnect ? _newConnection : null,
              ),
              const SizedBox(height: 8),
            ],
            if (active == null)
              Expanded(
                child: EmptyState(
                  icon: Icons.desktop_windows_outlined,
                  title: l.rdpEmptyTitle,
                  message: l.rdpEmptyHelp,
                  action: GlassButton.prominent(
                    key: const ValueKey('rdp-empty-connect'),
                    label: l.hostPickerDefaultTitle,
                    icon: Icons.add,
                    onPressed: controller.canConnect ? _newConnection : null,
                  ),
                ),
              )
            else
              Expanded(
                child: AnimatedBuilder(
                  animation: active,
                  builder: (context, _) => Column(
                    children: [
                      Wrap(
                        spacing: 8,
                        runSpacing: 8,
                        crossAxisAlignment: WrapCrossAlignment.center,
                        children: [
                          Text(switch (active.status.phase) {
                            RdpPhase.connecting => l.rdpConnecting,
                            RdpPhase.connected => l.rdpConnected,
                            RdpPhase.disconnected => l.rdpDisconnected,
                            RdpPhase.failed => _failureLabel(l, active.status.errorCode),
                          }),
                          if (active.noticeCode != null) Text(l.rdpResizeUnavailable),
                          GlassIconButton(
                            key: const ValueKey('rdp-secure-attention'),
                            icon: Icons.keyboard,
                            tooltip: l.rdpSecureAttention,
                            onPressed: active.status.phase == RdpPhase.connected
                                ? () => controller.send(active, const [
                                    RdpScancodeInput(0x1d, down: true),
                                    RdpScancodeInput(0x38, down: true),
                                    RdpScancodeInput(0x53, extended: true, down: true),
                                    RdpScancodeInput(0x53, extended: true, down: false),
                                    RdpScancodeInput(0x38, down: false),
                                    RdpScancodeInput(0x1d, down: false),
                                  ])
                                : null,
                          ),
                          GlassIconButton(
                            key: const ValueKey('rdp-resize'),
                            icon: Icons.aspect_ratio,
                            tooltip: l.rdpResize,
                            onPressed: active.status.phase == RdpPhase.connected
                                ? () => controller.send(active, const [RdpResizeInput(1920, 1080)])
                                : null,
                          ),
                          GlassIconButton(
                            key: const ValueKey('rdp-permissions'),
                            icon: Icons.admin_panel_settings_outlined,
                            tooltip: l.rdpPermissions,
                            onPressed: active.status.phase == RdpPhase.connected
                                ? () => showRdpPermissions(context, ref, active)
                                : null,
                          ),
                          RdpClipboardActions(key: ValueKey('rdp-clipboard-${active.info.id}'), tab: active),
                          GlassIconButton(
                            key: const ValueKey('rdp-expand'),
                            icon: Icons.fullscreen,
                            tooltip: l.rdpExpand,
                            onPressed: _openFullscreen,
                          ),
                        ],
                      ),
                      const SizedBox(height: 8),
                      if (active.directory != null &&
                          (active.status.phase == RdpPhase.connected ||
                              active.status.phase == RdpPhase.connecting)) ...[
                        RdpFolderStatus(directory: active.directory!, state: active.folderState),
                        const SizedBox(height: 8),
                      ],
                      Expanded(
                        child: RdpView(
                          key: ValueKey('rdp-view-${active.info.id}'),
                          frame: active.frame,
                          width: active.info.width,
                          height: active.info.height,
                          enabled: visible && active.status.phase == RdpPhase.connected,
                          onInput: (inputs) => controller.send(active, inputs),
                          onInteractionCancelled: () => controller.cancelInteraction(active),
                          localClipboardEnabled: active.permissions.clipboardEnabled,
                          onPaste: (isInputCurrent) => pasteLocalRdpClipboard(context, ref, active, isInputCurrent),
                        ),
                      ),
                    ],
                  ),
                ),
              ),
          ],
        );
        return GlassBlurSuppressor(
          budget: GlassScope.of(context).budget,
          child: PageScaffold(title: l.rdpTitle, subtitle: l.rdpWorkspaceHelp, body: body),
        );
      },
    );
  }
}

/// The saved-host probe supplies an immutable fresh configuration and opaque
/// native snapshot. Password credentials remain native for saved connections.
class RdpConnectionDialog extends ConsumerStatefulWidget {
  const RdpConnectionDialog({required this.scope, this.savedTicket, super.key});
  final RdpScope scope;
  final RdpSavedHostTicket? savedTicket;
  @override
  ConsumerState<RdpConnectionDialog> createState() => _ConnectionDialogState();
}

class _ConnectionDialogState extends ConsumerState<RdpConnectionDialog> {
  final _address = TextEditingController(), _port = TextEditingController(text: '3389');
  final _username = TextEditingController(), _domain = TextEditingController(), _password = TextEditingController();
  late final RdpService _service;
  RdpSessionPermissions _permissions = const RdpSessionPermissions();
  RdpDirectoryGrant? _directory;
  final Set<String> _owned = {};
  bool _busy = false;
  String? _error;
  Uint8List? _pendingPassword;
  bool _authInFlight = false;
  bool _cancelled = false;
  bool get _operationCurrent =>
      mounted && !_cancelled && rdpScopeCurrent(ref, widget.scope) && ModalRoute.of(context)?.isActive == true;

  @override
  void initState() {
    super.initState();
    _service = ref.read(rdpServiceProvider);
    final ticket = widget.savedTicket;
    if (ticket != null) {
      _address.text = ticket.options.address;
      _port.text = '${ticket.options.port}';
      _username.text = ticket.options.username;
      _domain.text = ticket.options.domain;
    }
  }

  void _permissionsChanged(RdpSessionPermissions permissions, RdpDirectoryGrant? directory) {
    final previous = _directory;
    if (previous != null && previous.id != directory?.id && _owned.remove(previous.id)) {
      _service.releaseDirectoryGrant(previous.id).ignore();
    }
    if (directory != null) _owned.add(directory.id);
    setState(() {
      _permissions = permissions;
      _directory = directory;
    });
  }

  void _releaseUnusedDirectories() {
    final ids = _owned.toList();
    _owned.clear();
    for (final id in ids) {
      _service.releaseDirectoryGrant(id).ignore();
    }
  }

  Future<void> _connect() async {
    if (_busy || !_operationCurrent) return;
    final saved = widget.savedTicket;
    RdpConnectionOptions options;
    try {
      options =
          saved?.options ??
          RdpConnectionOptions(
            address: _address.text.trim(),
            port: int.parse(_port.text.trim()),
            username: _username.text.trim(),
            domain: _domain.text.trim(),
          );
      if (saved?.hasSavedPassword != true && _password.text.isEmpty) throw const FormatException('empty_password');
    } catch (_) {
      setState(() => _error = context.l10n.rdpInvalidForm);
      return;
    }
    Uint8List? bytes;
    if (saved?.hasSavedPassword != true) {
      final secret = SecretText(_password.text);
      _password.clear(); // Immutable platform strings are best effort, never retained by metadata.
      bytes = secret.exposeBytes();
      secret.wipe();
      _pendingPassword = bytes;
      if (bytes.length > 4096) {
        bytes.fillRange(0, bytes.length, 0);
        _pendingPassword = null;
        setState(() => _error = context.l10n.rdpInvalidForm);
        return;
      }
    }
    final controller = ref.read(rdpWorkspaceProvider);
    setState(() {
      _busy = true;
      _error = null;
    });
    try {
      final certificate = saved?.certificate ?? await _service.probeCertificate(options.address, options.port);
      if (!mounted || !_operationCurrent) return;
      if (certificate.address != options.address || certificate.port != options.port) {
        throw const RdpFailure('certificate_mismatch');
      }
      final approved = await showAppDialog<bool>(
        context,
        secure: true,
        builder: (_) => _CertificateDialog(scope: widget.scope, endpoint: options.endpoint, certificate: certificate),
      );
      if (!_operationCurrent || approved != true) return;
      _authInFlight = true;
      final tab = saved == null
          ? await controller.connect(
              options,
              bytes!,
              certificate.sha256,
              isCurrent: () => _operationCurrent,
              permissions: _permissions,
              directory: _directory,
            )
          : await controller.connectSavedHost(
              saved,
              passwordBytes: bytes,
              isCurrent: () => _operationCurrent,
              permissions: _permissions,
              directory: _directory,
            );
      if (tab != null && _directory != null) _owned.remove(_directory!.id);
      if (mounted && _operationCurrent && tab != null) closeDialog<RdpTab>(context, tab);
    } catch (error) {
      if (mounted && _operationCurrent) {
        setState(() => _error = _failureLabel(context.l10n, error is RdpFailure ? error.code : null));
      }
    } finally {
      bytes?.fillRange(0, bytes.length, 0);
      if (identical(_pendingPassword, bytes)) _pendingPassword = null;
      _authInFlight = false;
      if (!mounted) _releaseUnusedDirectories();
      if (mounted) setState(() => _busy = false);
    }
  }

  @override
  void dispose() {
    _password.clear();
    _cancelled = true;
    // A native borrower retains these buffers/capabilities until connect
    // settles. Late sessions are disconnected before unused grants release.
    if (!_authInFlight) {
      _pendingPassword?.fillRange(0, _pendingPassword!.length, 0);
      _releaseUnusedDirectories();
    }
    for (final c in [_address, _port, _username, _domain, _password]) {
      c.dispose();
    }
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    final saved = widget.savedTicket;
    final editable = !_busy && saved == null;
    return RdpScopeGuard(
      scope: widget.scope,
      child: GlassDialog(
        title: saved?.name ?? l.rdpNewConnection,
        content: SizedBox(
          width: 500,
          child: SingleChildScrollView(
            child: Column(
              mainAxisSize: MainAxisSize.min,
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                Text(saved?.hasSavedPassword == true ? l.rdpSavedPasswordHelp : l.rdpCredentialsHelp),
                const SizedBox(height: 12),
                TextField(
                  key: const ValueKey('rdp-address'),
                  controller: _address,
                  readOnly: saved != null,
                  enabled: !_busy,
                  autocorrect: false,
                  enableSuggestions: false,
                  decoration: InputDecoration(labelText: l.rdpAddress, hintText: 'example.test'),
                ),
                const SizedBox(height: 8),
                TextField(
                  key: const ValueKey('rdp-port'),
                  controller: _port,
                  readOnly: !editable,
                  enabled: !_busy,
                  keyboardType: TextInputType.number,
                  decoration: InputDecoration(labelText: l.rdpPort),
                ),
                const SizedBox(height: 8),
                TextField(
                  key: const ValueKey('rdp-username'),
                  controller: _username,
                  readOnly: !editable,
                  enabled: !_busy,
                  autocorrect: false,
                  enableSuggestions: false,
                  decoration: InputDecoration(labelText: l.rdpUsername),
                ),
                const SizedBox(height: 8),
                TextField(
                  key: const ValueKey('rdp-domain'),
                  controller: _domain,
                  readOnly: !editable,
                  enabled: !_busy,
                  autocorrect: false,
                  enableSuggestions: false,
                  decoration: InputDecoration(labelText: l.rdpDomain),
                ),
                if (saved?.hasSavedPassword != true) ...[
                  const SizedBox(height: 8),
                  SecretField(
                    key: const ValueKey('rdp-password'),
                    controller: _password,
                    label: l.rdpPassword,
                    enabled: !_busy,
                  ),
                ],
                const SizedBox(height: 12),
                RdpPermissionControls(
                  scope: widget.scope,
                  permissions: _permissions,
                  directory: _directory,
                  enabled: !_busy,
                  onChanged: _permissionsChanged,
                ),
                if (_error != null) Padding(padding: const EdgeInsets.only(top: 12), child: Text(_error!)),
                if (_busy) Padding(padding: const EdgeInsets.only(top: 12), child: Text(l.rdpConnecting)),
              ],
            ),
          ),
        ),
        secondaryActions: [
          GlassButton(
            key: const ValueKey('rdp-connect-cancel'),
            label: l.commonCancel,
            onPressed: () {
              _cancelled = true;
              closeDialog<RdpTab>(context);
            },
          ),
        ],
        primaryAction: GlassButton.prominent(
          key: const ValueKey('rdp-connect-submit'),
          label: l.commonConnect,
          busy: _busy,
          onPressed: _busy ? null : _connect,
        ),
      ),
    );
  }
}

class _CertificateDialog extends ConsumerStatefulWidget {
  const _CertificateDialog({required this.scope, required this.endpoint, required this.certificate});
  final RdpScope scope;
  final String endpoint;
  final RdpCertificate certificate;
  @override
  ConsumerState<_CertificateDialog> createState() => _CertificateDialogState();
}

class _CertificateDialogState extends ConsumerState<_CertificateDialog> {
  bool _confirmed = false;
  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    return RdpScopeGuard(
      scope: widget.scope,
      child: GlassDialog(
        title: l.rdpCertificateTitle,
        content: SizedBox(
          width: 500,
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Text(l.rdpCertificateHelp),
              const SizedBox(height: 12),
              SelectableText(widget.endpoint),
              const SizedBox(height: 12),
              Text(l.rdpFingerprint),
              SelectableText(
                widget.certificate.displaySha256,
                key: const ValueKey('rdp-certificate-fingerprint'),
                style: const TextStyle(fontFamily: 'monospace'),
              ),
              const SizedBox(height: 12),
              CheckboxListTile(
                key: const ValueKey('rdp-certificate-checked'),
                value: _confirmed,
                contentPadding: EdgeInsets.zero,
                title: Text(l.rdpCertificateConfirm),
                onChanged: (value) => setState(() => _confirmed = value ?? false),
              ),
            ],
          ),
        ),
        secondaryActions: [
          GlassButton(
            key: const ValueKey('rdp-certificate-cancel'),
            label: l.commonCancel,
            autofocus: true,
            onPressed: () => closeDialog(context, false),
          ),
        ],
        primaryAction: GlassButton.prominent(
          key: const ValueKey('rdp-certificate-accept'),
          label: l.commonConnect,
          onPressed: _confirmed && rdpScopeCurrent(ref, widget.scope) ? () => closeDialog(context, true) : null,
        ),
      ),
    );
  }
}
