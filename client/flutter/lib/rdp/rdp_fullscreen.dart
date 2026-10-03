import 'dart:async';

import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/rdp/rdp_clipboard.dart';
import 'package:consolecrypt/rdp/rdp_controller.dart';
import 'package:consolecrypt/rdp/rdp_guard.dart';
import 'package:consolecrypt/rdp/rdp_permissions.dart';
import 'package:consolecrypt/rdp/rdp_providers.dart';
import 'package:consolecrypt/rdp/rdp_service.dart';
import 'package:consolecrypt/rdp/rdp_view.dart';
import 'package:consolecrypt/rdp/rdp_window.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

/// An opaque root route: no shell, title bar, sidebar or second input surface.
/// Closing this route restores the owning window without ending the session.
class RdpFullscreen extends ConsumerStatefulWidget {
  const RdpFullscreen({required this.scope, required this.controller, required this.window, super.key});
  final RdpScope scope;
  final RdpWorkspaceController controller;
  final RdpWindow window;
  @override
  ConsumerState<RdpFullscreen> createState() => _RdpFullscreenState();
}

class _RdpFullscreenState extends ConsumerState<RdpFullscreen> {
  Timer? _hideTimer, _windowTimer;
  final _pinFocus = FocusNode(debugLabel: 'rdp-connection-bar');
  bool _shown = true, _pinned = false, _hovering = false, _focused = false, _menuOpen = false;
  bool _closing = false, _checking = false;

  @override
  void initState() {
    super.initState();
    _scheduleHide();
    // Also honour the OS fullscreen command (green traffic light, WM action).
    if (widget.window.desktop) {
      _windowTimer = Timer.periodic(const Duration(milliseconds: 500), (_) => _checkWindow());
    }
  }

  Future<void> _checkWindow() async {
    if (_checking || _closing) return;
    _checking = true;
    try {
      if (!await widget.window.isFullscreen() && mounted) _leave();
    } catch (_) {
      if (mounted) _leave();
    } finally {
      _checking = false;
    }
  }

  void _leave({bool minimize = false}) {
    if (_closing || !mounted) return;
    _closing = true;
    final route = ModalRoute.of(context);
    if (route != null && route.isActive) Navigator.of(context).removeRoute(route, minimize);
  }

  void _scheduleHide() {
    _hideTimer?.cancel();
    if (_pinned || _hovering || _focused || _menuOpen) return;
    _hideTimer = Timer(const Duration(seconds: 3), () {
      if (mounted) setState(() => _shown = false);
    });
  }

  void _reveal({bool keyboard = false}) {
    setState(() => _shown = true);
    if (keyboard) _pinFocus.requestFocus();
    _scheduleHide();
  }

  @override
  void dispose() {
    _hideTimer?.cancel();
    _windowTimer?.cancel();
    _pinFocus.dispose();
    super.dispose();
  }

  Widget _bar(RdpTab tab) {
    final l = context.l10n;
    final controller = widget.controller;
    final connected = tab.status.phase == RdpPhase.connected;
    return Focus(
      onFocusChange: (focused) {
        _focused = focused;
        if (focused) _reveal();
        _scheduleHide();
      },
      child: MouseRegion(
        onEnter: (_) {
          _hovering = true;
          _reveal();
        },
        onExit: (_) {
          _hovering = false;
          _scheduleHide();
        },
        child: RepaintBoundary(
          key: const ValueKey('rdp-fullscreen-bar-surface'),
          child: Material(
            key: const ValueKey('rdp-fullscreen-bar'),
            color: const Color(0xff17243b),
            elevation: 12,
            borderRadius: const BorderRadius.vertical(bottom: Radius.circular(14)),
            clipBehavior: Clip.antiAlias,
            child: IconTheme(
              data: const IconThemeData(color: Colors.white, size: 20),
              child: SizedBox(
                height: 48,
                child: Row(
                  children: [
                    IconButton(
                      key: const ValueKey('rdp-fullscreen-pin'),
                      focusNode: _pinFocus,
                      tooltip: _pinned ? l.rdpUnpinBar : l.rdpPinBar,
                      icon: Icon(_pinned ? Icons.push_pin : Icons.push_pin_outlined),
                      onPressed: () {
                        setState(() => _pinned = !_pinned);
                        _scheduleHide();
                      },
                    ),
                    Expanded(
                      child: PopupMenuButton<RdpTab>(
                        key: const ValueKey('rdp-fullscreen-connections'),
                        tooltip: l.rdpConnections,
                        onOpened: () {
                          _menuOpen = true;
                          _scheduleHide();
                        },
                        onCanceled: () {
                          _menuOpen = false;
                          _scheduleHide();
                        },
                        onSelected: (selected) {
                          _menuOpen = false;
                          controller.activate(selected);
                          _scheduleHide();
                        },
                        itemBuilder: (_) => [
                          for (final item in controller.tabs)
                            CheckedPopupMenuItem(value: item, checked: identical(item, tab), child: Text(item.title)),
                        ],
                        child: Padding(
                          padding: const EdgeInsets.symmetric(horizontal: 8),
                          child: Row(
                            children: [
                              Icon(
                                Icons.desktop_windows_outlined,
                                size: 18,
                                color: connected ? const Color(0xff78d9b6) : Colors.grey,
                              ),
                              const SizedBox(width: 8),
                              Expanded(
                                child: Text(
                                  tab.title,
                                  maxLines: 1,
                                  overflow: TextOverflow.ellipsis,
                                  style: const TextStyle(color: Colors.white, fontWeight: FontWeight.w600),
                                ),
                              ),
                              const Icon(Icons.expand_more, size: 18),
                            ],
                          ),
                        ),
                      ),
                    ),
                    IconButton(
                      key: const ValueKey('rdp-fullscreen-permissions'),
                      tooltip: l.rdpPermissions,
                      icon: const Icon(Icons.admin_panel_settings_outlined),
                      onPressed: connected
                          ? () async {
                              _menuOpen = true;
                              _scheduleHide();
                              await showRdpPermissions(context, ref, tab);
                              if (!mounted) return;
                              _menuOpen = false;
                              _scheduleHide();
                            }
                          : null,
                    ),
                    if (widget.window.desktop)
                      IconButton(
                        key: const ValueKey('rdp-fullscreen-minimize'),
                        tooltip: l.rdpMinimize,
                        icon: const Icon(Icons.minimize),
                        onPressed: () => _leave(minimize: true),
                      ),
                    IconButton(
                      key: const ValueKey('rdp-fullscreen-restore'),
                      tooltip: l.rdpCollapse,
                      icon: const Icon(Icons.fullscreen_exit),
                      onPressed: _leave,
                    ),
                    IconButton(
                      key: const ValueKey('rdp-fullscreen-close'),
                      tooltip: l.rdpDisconnect,
                      icon: const Icon(Icons.close),
                      onPressed: () => controller.close(tab),
                    ),
                  ],
                ),
              ),
            ),
          ),
        ),
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    ref.listen(rdpScopeProvider, (_, next) {
      if (!identical(widget.scope, next)) _leave();
    });
    if (!rdpScopeCurrent(ref, widget.scope)) return const SizedBox.shrink();
    return AnimatedBuilder(
      animation: widget.controller,
      builder: (context, _) {
        final tab = widget.controller.active;
        if (tab == null || tab.closed) {
          WidgetsBinding.instance.addPostFrameCallback((_) => _leave());
          return const ColoredBox(color: Colors.black);
        }
        return AnimatedBuilder(
          animation: tab,
          builder: (context, _) => Scaffold(
            backgroundColor: Colors.black,
            body: Stack(
              children: [
                Positioned.fill(
                  child: RdpView(
                    key: ValueKey('rdp-fullscreen-view-${tab.info.id}'),
                    frame: tab.frame,
                    width: tab.info.width,
                    height: tab.info.height,
                    enabled: !_closing && tab.status.phase == RdpPhase.connected,
                    onInput: (inputs) => widget.controller.send(tab, inputs),
                    onInteractionCancelled: () => widget.controller.cancelInteraction(tab),
                    localClipboardEnabled: tab.permissions.clipboardEnabled,
                    onPaste: (current) => pasteLocalRdpClipboard(context, ref, tab, current),
                    onShowConnectionBar: () => _reveal(keyboard: true),
                  ),
                ),
                Align(
                  alignment: Alignment.topCenter,
                  child: SafeArea(
                    bottom: false,
                    child: ConstrainedBox(
                      constraints: const BoxConstraints(maxWidth: 680),
                      child: Stack(
                        children: [
                          Center(
                            heightFactor: 1,
                            child: MouseRegion(
                              onEnter: (_) => _reveal(),
                              child: Semantics(
                                button: true,
                                label: context.l10n.rdpShowBar,
                                child: GestureDetector(
                                  key: const ValueKey('rdp-fullscreen-reveal'),
                                  behavior: HitTestBehavior.opaque,
                                  onTap: () => _reveal(keyboard: true),
                                  child: SizedBox(
                                    width: 160,
                                    height: 24,
                                    child: Align(
                                      alignment: Alignment.topCenter,
                                      child: Container(
                                        width: 48,
                                        height: 4,
                                        decoration: BoxDecoration(
                                          color: const Color(0xff78a9ff),
                                          borderRadius: BorderRadius.circular(4),
                                        ),
                                      ),
                                    ),
                                  ),
                                ),
                              ),
                            ),
                          ),
                          IgnorePointer(
                            ignoring: !_shown,
                            child: ExcludeSemantics(
                              excluding: !_shown,
                              child: AnimatedSlide(
                                offset: _shown ? Offset.zero : const Offset(0, -1.2),
                                duration: MediaQuery.disableAnimationsOf(context)
                                    ? Duration.zero
                                    : const Duration(milliseconds: 180),
                                child: _bar(tab),
                              ),
                            ),
                          ),
                        ],
                      ),
                    ),
                  ),
                ),
              ],
            ),
          ),
        );
      },
    );
  }
}
