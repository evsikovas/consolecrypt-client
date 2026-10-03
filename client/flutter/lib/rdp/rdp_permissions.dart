import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/rdp/rdp_controller.dart';
import 'package:consolecrypt/rdp/rdp_guard.dart';
import 'package:consolecrypt/rdp/rdp_providers.dart';
import 'package:consolecrypt/rdp/rdp_service.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

String _permissionFailureLabel(AppLocalizations l, String? code) => switch (code) {
  'directory_grant_unavailable' => l.rdpFolderChooseFailed,
  'unsupported_platform' => l.rdpFolderUnsupported,
  'session_limit' => l.rdpFolderChooseLimit,
  _ => l.rdpPermissionsFailed,
};

class RdpFolderStatus extends StatelessWidget {
  const RdpFolderStatus({required this.directory, required this.state, super.key});
  final RdpDirectoryGrant directory;
  final RdpFolderState state;

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    final message = switch (state) {
      RdpFolderState.pending => l.rdpFolderPending,
      RdpFolderState.ready => l.rdpFolderReady,
      RdpFolderState.denied => l.rdpFolderDenied,
      RdpFolderState.disabled || RdpFolderState.unavailable => l.rdpFolderUnavailable,
    };
    return Row(
      key: const ValueKey('rdp-folder-status'),
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Icon(state == RdpFolderState.ready ? Icons.folder_open : Icons.info_outline, size: 18),
        const SizedBox(width: 8),
        Expanded(
          child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [Text(directory.name), Text(message)]),
        ),
      ],
    );
  }
}

class RdpPermissionControls extends ConsumerStatefulWidget {
  const RdpPermissionControls({
    required this.scope,
    required this.permissions,
    required this.directory,
    required this.onChanged,
    this.enabled = true,
    super.key,
  });
  final RdpScope scope;
  final RdpSessionPermissions permissions;
  final RdpDirectoryGrant? directory;
  final void Function(RdpSessionPermissions, RdpDirectoryGrant?) onChanged;
  final bool enabled;
  @override
  ConsumerState<RdpPermissionControls> createState() => _RdpPermissionControlsState();
}

class _RdpPermissionControlsState extends ConsumerState<RdpPermissionControls> {
  RdpCapabilities? _capabilities;
  bool _picking = false;
  bool _failed = false;
  String? _errorCode;
  @override
  void initState() {
    super.initState();
    _load();
  }

  Future<void> _load() async {
    try {
      final capabilities = await ref.read(rdpServiceProvider).capabilities();
      if (mounted && rdpScopeCurrent(ref, widget.scope)) setState(() => _capabilities = capabilities);
    } catch (error) {
      if (mounted && rdpScopeCurrent(ref, widget.scope)) {
        setState(() {
          _failed = true;
          _errorCode = error is RdpFailure ? error.code : null;
        });
      }
    }
  }

  Future<void> _pick() async {
    if (_picking || !widget.enabled || !rdpScopeCurrent(ref, widget.scope)) return;
    final service = ref.read(rdpServiceProvider);
    setState(() {
      _picking = true;
      _failed = false;
      _errorCode = null;
    });
    try {
      final directory = await service.pickDirectory();
      if (directory == null) return;
      if (!mounted || !rdpScopeCurrent(ref, widget.scope)) {
        await service.releaseDirectoryGrant(directory.id);
        return;
      }
      widget.onChanged(
        widget.permissions.copyWith(directoryGrantId: directory.id, directoryWritable: false),
        directory,
      );
    } catch (error) {
      if (mounted && rdpScopeCurrent(ref, widget.scope)) {
        setState(() {
          _failed = true;
          _errorCode = error is RdpFailure ? error.code : null;
        });
      }
    } finally {
      if (mounted) setState(() => _picking = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    final ready = widget.enabled && !_picking && rdpScopeCurrent(ref, widget.scope);
    return Column(
      mainAxisSize: MainAxisSize.min,
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        CheckboxListTile(
          key: const ValueKey('rdp-allow-clipboard'),
          contentPadding: EdgeInsets.zero,
          title: Text(l.rdpAllowClipboard),
          subtitle: Text(l.rdpClipboardHelp),
          value: widget.permissions.clipboardEnabled,
          onChanged: ready && _capabilities?.clipboardSupported == true
              ? (value) =>
                    widget.onChanged(widget.permissions.copyWith(clipboardEnabled: value ?? false), widget.directory)
              : null,
        ),
        const SizedBox(height: 8),
        Text(l.rdpFolderHelp),
        const SizedBox(height: 8),
        Wrap(
          spacing: 8,
          runSpacing: 8,
          crossAxisAlignment: WrapCrossAlignment.center,
          children: [
            GlassButton(
              key: const ValueKey('rdp-pick-folder'),
              label: l.rdpSelectFolder,
              icon: Icons.folder_open,
              busy: _picking,
              onPressed: ready && _capabilities?.folderSupported == true ? _pick : null,
            ),
            if (widget.directory != null) ...[
              Text(widget.directory!.name, key: const ValueKey('rdp-shared-folder-name')),
              GlassButton(
                key: const ValueKey('rdp-remove-folder'),
                label: l.rdpStopFolder,
                onPressed: ready
                    ? () => widget.onChanged(widget.permissions.copyWith(clearDirectory: true), null)
                    : null,
              ),
            ] else
              Text(l.rdpNoFolder),
          ],
        ),
        if (widget.directory != null)
          CheckboxListTile(
            key: const ValueKey('rdp-allow-folder-write'),
            contentPadding: EdgeInsets.zero,
            title: Text(l.rdpAllowFolderWrite),
            subtitle: Text(l.rdpFolderWriteHelp),
            value: widget.permissions.directoryWritable,
            onChanged: ready
                ? (value) =>
                      widget.onChanged(widget.permissions.copyWith(directoryWritable: value ?? false), widget.directory)
                : null,
          ),
        if (widget.directory != null) ...[
          const SizedBox(height: 8),
          Text(l.rdpFolderOpenHelp),
          SelectableText(l.rdpFolderWindowsPath, key: const ValueKey('rdp-folder-windows-path')),
          const SizedBox(height: 8),
          Text(l.rdpFolderLimits),
        ],
        if (_capabilities?.folderSupported == false)
          Padding(padding: const EdgeInsets.only(top: 8), child: Text(l.rdpFolderUnsupported)),
        if (_failed)
          Padding(padding: const EdgeInsets.only(top: 8), child: Text(_permissionFailureLabel(l, _errorCode))),
      ],
    );
  }
}

Future<void> showRdpPermissions(BuildContext context, WidgetRef ref, RdpTab tab) {
  final scope = ref.read(rdpScopeProvider);
  return showAppDialog<void>(
    context,
    secure: true,
    barrierDismissible: false,
    builder: (_) => _PermissionsDialog(scope: scope, tab: tab),
  );
}

class _PermissionsDialog extends ConsumerStatefulWidget {
  const _PermissionsDialog({required this.scope, required this.tab});
  final RdpScope scope;
  final RdpTab tab;
  @override
  ConsumerState<_PermissionsDialog> createState() => _PermissionsDialogState();
}

class _PermissionsDialogState extends ConsumerState<_PermissionsDialog> {
  late RdpSessionPermissions _permissions = widget.tab.permissions;
  late RdpDirectoryGrant? _directory = widget.tab.directory;
  final Set<String> _owned = {};
  late final RdpService _service;
  @override
  void initState() {
    super.initState();
    _service = ref.read(rdpServiceProvider);
  }

  void _releaseUnused() {
    final ids = _owned.toList();
    _owned.clear();
    for (final id in ids) {
      _service.releaseDirectoryGrant(id).ignore();
    }
  }

  bool _busy = false;
  bool _failed = false;
  String? _errorCode;
  Future<void> _apply() async {
    if (_busy || widget.tab.closed || !rdpScopeCurrent(ref, widget.scope)) return;
    final controller = ref.read(rdpWorkspaceProvider);
    setState(() {
      _busy = true;
      _failed = false;
      _errorCode = null;
    });
    try {
      await controller.setPermissions(widget.tab, _permissions, directory: _directory);
      if (_directory != null) _owned.remove(_directory!.id);
      if (mounted && rdpScopeCurrent(ref, widget.scope)) closeDialog<void>(context);
    } catch (error) {
      if (mounted && rdpScopeCurrent(ref, widget.scope)) {
        setState(() {
          _failed = true;
          _errorCode = error is RdpFailure ? error.code : null;
        });
      }
    } finally {
      if (!mounted) _releaseUnused();
      if (mounted) setState(() => _busy = false);
    }
  }

  @override
  void dispose() {
    if (!_busy) _releaseUnused();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) => RdpScopeGuard(
    scope: widget.scope,
    child: PopScope(
      canPop: !_busy,
      child: GlassDialog(
        title: context.l10n.rdpPermissions,
        content: SizedBox(
          width: 500,
          child: SingleChildScrollView(
            child: Column(
              mainAxisSize: MainAxisSize.min,
              children: [
                RdpPermissionControls(
                  scope: widget.scope,
                  permissions: _permissions,
                  directory: _directory,
                  enabled: !_busy,
                  onChanged: (permissions, directory) => setState(() {
                    final old = _directory;
                    if (old != null && old.id != directory?.id && _owned.remove(old.id)) {
                      _service.releaseDirectoryGrant(old.id).ignore();
                    }
                    if (directory != null && directory.id != widget.tab.directory?.id) _owned.add(directory.id);
                    _permissions = permissions;
                    _directory = directory;
                  }),
                ),
                if (_directory != null &&
                    _permissions.directoryGrantId == widget.tab.permissions.directoryGrantId &&
                    _permissions.directoryWritable == widget.tab.permissions.directoryWritable) ...[
                  const SizedBox(height: 12),
                  AnimatedBuilder(
                    animation: widget.tab,
                    builder: (context, _) => RdpFolderStatus(directory: _directory!, state: widget.tab.folderState),
                  ),
                ],
                if (_failed) Text(_permissionFailureLabel(context.l10n, _errorCode)),
              ],
            ),
          ),
        ),
        secondaryActions: [
          GlassButton(
            key: const ValueKey('rdp-permissions-cancel'),
            label: context.l10n.commonCancel,
            onPressed: _busy ? null : () => closeDialog<void>(context),
          ),
        ],
        primaryAction: GlassButton.prominent(
          key: const ValueKey('rdp-permissions-apply'),
          label: context.l10n.commonSave,
          busy: _busy,
          onPressed: _busy ? null : _apply,
        ),
      ),
    ),
  );
}
