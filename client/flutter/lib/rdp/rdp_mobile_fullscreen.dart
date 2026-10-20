import 'dart:async';
import 'dart:convert';

import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/models/rdp_mobile.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/hosts/connection_picker.dart';
import 'package:consolecrypt/rdp/rdp_clipboard.dart';
import 'package:consolecrypt/rdp/rdp_controller.dart';
import 'package:consolecrypt/rdp/rdp_guard.dart';
import 'package:consolecrypt/rdp/rdp_mobile_overlay.dart';
import 'package:consolecrypt/rdp/rdp_permissions.dart';
import 'package:consolecrypt/rdp/rdp_providers.dart';
import 'package:consolecrypt/rdp/rdp_service.dart';
import 'package:consolecrypt/rdp/rdp_view.dart';
import 'package:consolecrypt/rdp/rdp_viewport.dart';
import 'package:crypto/crypto.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

String rdpMobileHostKey(RdpScope scope, RdpTab tab) => sha256
    .convert(
      utf8.encode(
        jsonEncode([
          scope.profileId.toString(),
          tab.hostId ?? [tab.options.endpoint, tab.options.username, tab.options.domain],
        ]),
      ),
    )
    .toString();

class RdpMobileFullscreen extends ConsumerStatefulWidget {
  const RdpMobileFullscreen({super.key, required this.controller, required this.scope, required this.onLeave});
  final RdpWorkspaceController controller;
  final RdpScope scope;
  final VoidCallback onLeave;
  @override
  ConsumerState<RdpMobileFullscreen> createState() => _RdpMobileFullscreenState();
}

class _RdpMobileFullscreenState extends ConsumerState<RdpMobileFullscreen> {
  final _views = <String, GlobalKey<RdpViewState>>{};
  final _resolutions = <String, (int, int, int)>{};
  final _resizeUnsupported = <String>{};
  Timer? _resize;
  (String, int, int, int)? _pending;
  DateTime? _back;
  RdpMobileOrientation? _orientation;
  bool _introduced = false;
  bool get _current => mounted && rdpScopeCurrent(ref, widget.scope);
  Widget _guard(Widget child) => RdpScopeGuard(scope: widget.scope, child: child);

  GlobalKey<RdpViewState> _key(RdpTab tab) => _views.putIfAbsent(tab.info.id, () => GlobalKey<RdpViewState>());
  RdpMobileSettings get _settings => ref.read(settingsServiceProvider).currentLocal.rdpMobile;

  void _save(RdpMobileSettings settings) {
    final service = ref.read(settingsServiceProvider);
    service.updateLocal(service.currentLocal.copyWith(rdpMobile: settings)).ignore();
  }

  void _setOrientation(RdpMobileOrientation orientation) {
    if (_orientation == orientation) return;
    _orientation = orientation;
    SystemChrome.setPreferredOrientations(switch (orientation) {
      RdpMobileOrientation.automatic => [],
      RdpMobileOrientation.landscape => [DeviceOrientation.landscapeLeft, DeviceOrientation.landscapeRight],
      RdpMobileOrientation.portrait => [DeviceOrientation.portraitUp, DeviceOrientation.portraitDown],
    }).ignore();
  }

  void _scheduleResolution(RdpTab tab, Size size, {bool force = false}) {
    if (tab.noticeCode == 'resize_unavailable') _resizeUnsupported.add(tab.info.id);
    if ((!_settings.autoResize && !force) ||
        _resizeUnsupported.contains(tab.info.id) ||
        tab.status.phase != RdpPhase.connected) {
      return;
    }
    final resolution = rdpPhoneResolution(size, clarity: _settings.clarity);
    final scale = _settings.windowsScale;
    final request = (resolution.$1, resolution.$2, scale);
    if (!force && _resolutions[tab.info.id] == request) return;
    final next = (tab.info.id, resolution.$1, resolution.$2, scale);
    if (_pending == next && !force) return;
    _pending = next;
    _resize?.cancel();
    _resize = Timer(const Duration(milliseconds: 500), () {
      _pending = null;
      if (!_current ||
          (!force && !_settings.autoResize) ||
          tab.closed ||
          !identical(widget.controller.active, tab) ||
          tab.status.phase != RdpPhase.connected ||
          tab.noticeCode == 'resize_unavailable') {
        return;
      }
      _resolutions[tab.info.id] = request;
      widget.controller.send(tab, [RdpResizeInput(resolution.$1, resolution.$2, desktopScale: scale)]);
    });
  }

  void _onBack() {
    if (!mounted) return;
    final tab = widget.controller.active;
    if (tab == null) return widget.onLeave();
    final view = _key(tab).currentState;
    if (view?.keyboardShown == true) {
      view?.hideKeyboard();
      return;
    }
    final now = DateTime.now();
    if (_back != null && now.difference(_back!) < const Duration(seconds: 2)) {
      widget.onLeave();
      return;
    }
    _back = now;
    view?.showControls(!(view.menuOpen));
    ScaffoldMessenger.of(context)
        .showSnackBar(SnackBar(content: Text(context.l10n.rdpMobileBackAgain), duration: const Duration(seconds: 2)));
  }

  Future<void> _sessions() async {
    final l = context.l10n;
    final tab = await showModalBottomSheet<RdpTab>(
      context: context,
      builder: (context) => _guard(
        SafeArea(
          child: ListView(
            shrinkWrap: true,
            children: [
              for (final tab in widget.controller.tabs)
                ListTile(
                  title: Text(tab.title),
                  selected: identical(tab, widget.controller.active),
                  onTap: () => Navigator.pop(context, tab),
                ),
              ListTile(
                leading: const Icon(Icons.add),
                title: Text(l.hostPickerDefaultTitle),
                enabled: widget.controller.canConnect,
                onTap: () {
                  if (!_current) return;
                  Navigator.pop(context);
                  showConnectionPicker(this.context, ref, protocol: HostProtocol.rdp).ignore();
                },
              ),
            ],
          ),
        ),
      ),
    );
    if (_current && tab != null && !tab.closed) widget.controller.activate(tab);
  }

  Future<void> _orientationPicker() async {
    final l = context.l10n;
    final selected = await showModalBottomSheet<RdpMobileOrientation>(
      context: context,
      builder: (context) => _guard(
        SafeArea(
          child: Column(
            mainAxisSize: MainAxisSize.min,
            children: [
              for (final entry in [
                (RdpMobileOrientation.automatic, l.rdpMobileAutomatic),
                (RdpMobileOrientation.landscape, l.rdpMobileLandscape),
                (RdpMobileOrientation.portrait, l.rdpMobilePortrait),
              ])
                ListTile(
                  title: Text(entry.$2),
                  selected: _orientation == entry.$1,
                  onTap: () => Navigator.pop(context, entry.$1),
                ),
            ],
          ),
        ),
      ),
    );
    if (!_current || selected == null) return;
    _setOrientation(selected);
    _save(_settings.copyWith(orientation: selected));
  }

  Future<void> _disconnect(RdpTab tab) async {
    final l = context.l10n;
    final approved = await showDialog<bool>(
      context: context,
      builder: (context) => _guard(
        AlertDialog(
          title: Text(l.rdpMobileDisconnectConfirm),
          actions: [
            TextButton(onPressed: () => Navigator.pop(context, false), child: Text(l.commonCancel)),
            TextButton(onPressed: () => Navigator.pop(context, true), child: Text(l.rdpDisconnect)),
          ],
        ),
      ),
    );
    if (_current && approved == true) await widget.controller.close(tab);
  }

  void _introduce() {
    if (_introduced || _settings.hideIntroduction || MediaQuery.sizeOf(context).shortestSide >= 600) return;
    _introduced = true;
    WidgetsBinding.instance.addPostFrameCallback((_) async {
      if (!_current) return;
      final l = context.l10n;
      var step = 0;
      final done = await showDialog<bool>(
        context: context,
        builder: (context) => _guard(
          StatefulBuilder(
            builder: (context, update) => AlertDialog(
              title: Text(l.rdpMobileHelpTitle),
              content: Text([l.rdpMobileIntroMove, l.rdpMobileIntroZoom, l.rdpMobileIntroMenu][step]),
              actions: [
                TextButton(onPressed: () => Navigator.pop(context, true), child: Text(l.rdpMobileDontShow)),
                TextButton(
                  onPressed: () => step < 2 ? update(() => step++) : Navigator.pop(context, true),
                  child: Text(step < 2 ? l.commonNext : l.commonDone),
                ),
              ],
            ),
          ),
        ),
      );
      if (_current && done == true) _save(_settings.copyWith(hideIntroduction: true));
    });
  }

  @override
  void dispose() {
    _resize?.cancel();
    SystemChrome.setPreferredOrientations([]).ignore();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final settings = ref.watch(localSettingsProvider).value?.rdpMobile ?? _settings;
    _setOrientation(settings.orientation);
    _introduce();
    return PopScope(
      canPop: false,
      onPopInvokedWithResult: (didPop, _) {
        if (!didPop) _onBack();
      },
      child: AnimatedBuilder(
        animation: widget.controller,
        builder: (context, _) {
          final tab = widget.controller.active;
          if (tab == null || tab.closed) {
            WidgetsBinding.instance.addPostFrameCallback((_) {
              if (mounted) widget.onLeave();
            });
            return const ColoredBox(color: Colors.black);
          }
          return AnimatedBuilder(
            animation: tab,
            builder: (context, _) => Scaffold(
              key: const ValueKey('rdp-mobile-fullscreen'),
              backgroundColor: Colors.black,
              resizeToAvoidBottomInset: false,
              body: SafeArea(
                maintainBottomViewPadding: true,
                child: LayoutBuilder(
                  builder: (context, constraints) {
                    _scheduleResolution(tab, constraints.biggest);
                    final l = context.l10n;
                    final connected = tab.status.phase == RdpPhase.connected;
                    void action(VoidCallback invoke) {
                      widget.controller.cancelInteraction(tab);
                      _key(tab).currentState?.showControls(false);
                      invoke();
                    }

                    return RdpView(
                      key: _key(tab),
                      scope: widget.scope,
                      mobileStatus: switch (tab.status.phase) {
                        RdpPhase.connected => null,
                        RdpPhase.connecting => l.rdpConnecting,
                        RdpPhase.disconnected => l.rdpDisconnected,
                        RdpPhase.failed => l.rdpConnectionFailed,
                      },
                      frame: tab.frame,
                      width: tab.info.width,
                      height: tab.info.height,
                      enabled: connected,
                      onInput: (inputs) => widget.controller.send(tab, inputs),
                      onInteractionCancelled: () => widget.controller.cancelInteraction(tab),
                      localClipboardEnabled: tab.permissions.clipboardEnabled,
                      onPaste: (current) => pasteLocalRdpClipboard(context, ref, tab, current),
                      mobileSettings: settings,
                      onMobileSettingsChanged: _save,
                      hostKey: rdpMobileHostKey(widget.scope, tab),
                      mobileActions: [
                        RdpMobileAction(
                          l.rdpSecureAttention,
                          Icons.security,
                          !connected
                              ? null
                              : () => action(() {
                                  widget.controller.send(tab, const [
                                    RdpScancodeInput(0x1d, down: true),
                                    RdpScancodeInput(0x38, down: true),
                                    RdpScancodeInput(0x53, extended: true, down: true),
                                    RdpScancodeInput(0x53, extended: true, down: false),
                                    RdpScancodeInput(0x38, down: false),
                                    RdpScancodeInput(0x1d, down: false),
                                  ]);
                                }),
                        ),
                        RdpMobileAction(
                          l.rdpMobileFitResolution,
                          Icons.aspect_ratio,
                          !connected || _resizeUnsupported.contains(tab.info.id)
                              ? null
                              : () => action(() => _scheduleResolution(tab, constraints.biggest, force: true)),
                        ),
                        RdpMobileAction(
                          l.rdpMobileOrientation,
                          Icons.screen_rotation,
                          () => action(() => _orientationPicker().ignore()),
                        ),
                        RdpMobileAction(
                          l.rdpMobileClipboard,
                          Icons.content_paste,
                          !connected
                              ? null
                              : () => action(() {
                                  showModalBottomSheet<void>(
                                    context: context,
                                    builder: (context) => _guard(
                                      SafeArea(
                                        child: Padding(
                                          padding: const EdgeInsets.all(16),
                                          child: RdpClipboardActions(tab: tab),
                                        ),
                                      ),
                                    ),
                                  );
                                }),
                        ),
                        RdpMobileAction(
                          l.rdpPermissions,
                          Icons.admin_panel_settings_outlined,
                          !connected ? null : () => action(() => showRdpPermissions(context, ref, tab).ignore()),
                        ),
                        RdpMobileAction(l.rdpConnections, Icons.tab, () => action(() => _sessions().ignore())),
                        RdpMobileAction(l.rdpMobileMinimize, Icons.fullscreen_exit, () => action(widget.onLeave)),
                        RdpMobileAction(l.rdpDisconnect, Icons.close, () => action(() => _disconnect(tab).ignore())),
                      ],
                    );
                  },
                ),
              ),
            ),
          );
        },
      ),
    );
  }
}
