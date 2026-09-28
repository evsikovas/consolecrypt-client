import 'dart:async';

import 'package:consolecrypt/app/platform.dart';
import 'package:consolecrypt/core/l10n/error_messages.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/services/app_services.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/hosts/host_picker.dart';
import 'package:consolecrypt/sftp/sftp_actions.dart';
import 'package:consolecrypt/sftp/sftp_controller.dart';
import 'package:consolecrypt/sftp/sftp_providers.dart';
import 'package:consolecrypt/sftp/widgets/sftp_activity.dart';
import 'package:consolecrypt/sftp/widgets/sftp_file_list.dart';
import 'package:consolecrypt/sftp/widgets/sftp_local_pane.dart';
import 'package:consolecrypt/sftp/widgets/sftp_mobile_list.dart';
import 'package:consolecrypt/sftp/widgets/sftp_path_bar.dart';
import 'package:consolecrypt/sftp/widgets/sftp_status_bar.dart';
import 'package:consolecrypt/sftp/widgets/sftp_toolbar.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

/// Transmit-style SFTP browser (docs/design/SFTP_BROWSER_SPEC.md): toolbar,
/// breadcrumb path bar, dense file list (content surface), activity drawer
/// (transfers / edit sessions), status bar; optional local pane.
class SftpScreen extends ConsumerStatefulWidget {
  const SftpScreen({super.key});

  @override
  ConsumerState<SftpScreen> createState() => _SftpScreenState();
}

class _SftpScreenState extends ConsumerState<SftpScreen> {
  final FocusNode _listFocus = FocusNode(debugLabel: 'sftp-list');
  final FocusNode _searchFocus = FocusNode(debugLabel: 'sftp-search');
  final TextEditingController _search = TextEditingController();
  final GlobalKey<SftpPathBarState> _pathBar = GlobalKey();

  @override
  void dispose() {
    _listFocus.dispose();
    _searchFocus.dispose();
    _search.dispose();
    super.dispose();
  }

  Future<void> _connect() async {
    final host = await showHostPicker(context, title: context.l10n.sftpOpenPickerTitle);
    if (host == null || !mounted) return;
    if (!await _confirmStopEdits()) return;
    await ref.read(sftpControllerProvider.notifier).connect(host);
  }

  Future<void> _disconnect() async {
    if (!await _confirmStopEdits()) return;
    await ref.read(sftpControllerProvider.notifier).disconnect();
  }

  /// Disconnecting ends the edit sessions of this connection (the core
  /// uploads pending changes first) — ask when there are any.
  Future<bool> _confirmStopEdits() async {
    final session = ref.read(sftpControllerProvider).session;
    if (session == null) return true;
    final edits = (ref.read(editSessionsProvider).value ?? const <EditSessionInfo>[])
        .where((e) => e.sftpSession == session)
        .length;
    if (edits == 0) return true;
    final l10n = context.l10n;
    return showConfirmDialog(
      context,
      title: l10n.sftpDisconnectEditsTitle,
      message: l10n.sftpDisconnectEditsMessage(edits),
      confirmLabel: l10n.sftpDisconnect,
    );
  }

  KeyEventResult _onKey(FocusNode node, KeyEvent event) {
    if (event is! KeyDownEvent) return KeyEventResult.ignored;
    final keys = HardwareKeyboard.instance;
    final primary = AppPlatform.usesMeta ? keys.isMetaPressed : keys.isControlPressed;
    if (!primary) return KeyEventResult.ignored;
    // Cmd/Ctrl+L (and Finder's Cmd+Shift+G): type a path. While the browser
    // has focus this takes precedence over the app-wide Lock shortcut.
    if (event.logicalKey == LogicalKeyboardKey.keyL ||
        (event.logicalKey == LogicalKeyboardKey.keyG && keys.isShiftPressed)) {
      _pathBar.currentState?.startEditing();
      return KeyEventResult.handled;
    }
    if (event.logicalKey == LogicalKeyboardKey.keyF) {
      _searchFocus.requestFocus();
      _search.selection = TextSelection(baseOffset: 0, extentOffset: _search.text.length);
      return KeyEventResult.handled;
    }
    return KeyEventResult.ignored;
  }

  void _onSearchEscape() {
    _search.clear();
    ref.read(sftpControllerProvider.notifier).setFilter('');
    _listFocus.requestFocus();
  }

  @override
  Widget build(BuildContext context) {
    final l10n = context.l10n;
    final theme = Theme.of(context);
    final state = ref.watch(sftpControllerProvider);
    final controller = ref.read(sftpControllerProvider.notifier);

    // Keep the search field in sync when navigation clears the filter.
    ref.listen(sftpControllerProvider.select((s) => s.filter), (_, next) {
      if (_search.text != next) _search.text = next;
    });
    // Open the connection with the list focused.
    ref.listen(sftpControllerProvider.select((s) => s.session), (prev, next) {
      if (next != null && prev != next) {
        WidgetsBinding.instance.addPostFrameCallback((_) {
          if (mounted) _listFocus.requestFocus();
        });
      }
    });
    // A session that runs into a conflict / error surfaces in the drawer.
    ref.listen(editSessionsProvider, (prev, next) {
      final before = {for (final s in prev?.value ?? const <EditSessionInfo>[]) s.id: s.status.needsAttention};
      final attention = (next.value ?? const <EditSessionInfo>[]).any(
        (s) => s.status.needsAttention && before[s.id] != true,
      );
      if (attention) ref.read(sftpActivityProvider.notifier).show(SftpActivityTab.editing);
    });

    if (!state.isConnected) {
      return PageScaffold(
        title: 'SFTP',
        subtitle: l10n.sftpSubtitle,
        actions: [
          FilledButton.icon(
            key: const ValueKey('sftp-connect'),
            onPressed: state.connecting ? null : _connect,
            icon: const Icon(Icons.link),
            label: Text(l10n.commonConnect),
          ),
        ],
        body: state.connecting
            ? Center(
                child: Column(
                  mainAxisSize: MainAxisSize.min,
                  children: [
                    const CircularProgressIndicator(),
                    const SizedBox(height: 12),
                    Text(l10n.sftpConnecting(state.host?.name ?? '')),
                  ],
                ),
              )
            : EmptyState(
                icon: Icons.folder_copy_outlined,
                title: l10n.sftpNotConnectedTitle,
                message: state.error == null ? l10n.sftpNotConnectedMessage : errorMessage(l10n, state.error!),
              ),
      );
    }

    final actions = SftpActions(context, ref);
    final selection = state.selectedEntries;
    return Material(
      color: theme.colorScheme.surface,
      child: Focus(
        onKeyEvent: _onKey,
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            SftpToolbar(
              searchController: _search,
              searchFocus: _searchFocus,
              onQuickLook: SftpActions.isEnabled(SftpAction.quickLook, selection)
                  ? () => unawaited(actions.run(SftpAction.quickLook, selection))
                  : null,
              actionMenuEntries: actions.glassMenuEntries(selection),
              onAction: (action) => actions.runChoice(action, selection),
              onSwitchHost: () => unawaited(_connect()),
              onDisconnect: () => unawaited(_disconnect()),
              onSearchEscape: _onSearchEscape,
            ),
            const Divider(height: 1),
            if (state.error != null)
              Padding(
                padding: const EdgeInsets.fromLTRB(12, 8, 12, 0),
                child: InfoBanner(
                  tone: BannerTone.danger,
                  message: errorMessage(l10n, state.error!),
                  action: TextButton(onPressed: controller.clearError, child: Text(l10n.commonClose)),
                ),
              ),
            Expanded(
              child: LayoutBuilder(
                builder: (context, constraints) {
                  final remote = Column(
                    crossAxisAlignment: CrossAxisAlignment.stretch,
                    children: [
                      SftpPathBar(key: _pathBar, onEditingDone: _listFocus.requestFocus),
                      const Divider(height: 1),
                      Expanded(
                        child: AppPlatform.isMobile ? const SftpMobileList() : SftpFileList(focusNode: _listFocus),
                      ),
                    ],
                  );
                  if (AppPlatform.isMobile || !state.prefs.showLocalPane) return remote;
                  final localWidth = (constraints.maxWidth * 0.38).clamp(240.0, 420.0);
                  return Row(
                    crossAxisAlignment: CrossAxisAlignment.stretch,
                    children: [
                      SizedBox(width: localWidth, child: const SftpLocalPane()),
                      const VerticalDivider(width: 1),
                      Expanded(child: remote),
                    ],
                  );
                },
              ),
            ),
            const SftpActivityDrawer(),
            const Divider(height: 1),
            const SftpStatusBar(),
          ],
        ),
      ),
    );
  }
}
