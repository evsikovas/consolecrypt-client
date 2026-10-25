import 'package:consolecrypt/core/providers.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

/// Whether the desktop sidebar is folded to icons (windows below the compact
/// breakpoint are always compact, see the shell).
///
/// The user's choice is remembered on this device. Next to the terminal tab
/// column the sidebar folds by default and keeps its own remembered choice
/// there; an open Kubernetes cluster folds it while the workspace is open.
/// A manual toggle always wins over the automatic folding.
final sidebarCollapsedProvider = NotifierProvider<SidebarCollapsed, bool>(SidebarCollapsed.new);

class SidebarCollapsed extends Notifier<bool> {
  bool _autoCollapsed = false;
  bool _besideTabs = false;

  @override
  bool build() {
    final remembered = ref.watch(localSettingsProvider.select((s) => s.value?.sidebarCollapsed)) ?? false;
    final besideTabs = ref.watch(localSettingsProvider.select((s) => s.value?.sidebarCollapsedBesideTabs));
    return _resolve(remembered, besideTabs);
  }

  bool _resolve(bool remembered, bool? besideTabs) {
    if (_autoCollapsed) return true;
    if (_besideTabs) return besideTabs ?? true;
    return remembered;
  }

  void _update() {
    final local = ref.read(localSettingsProvider).value;
    state = _resolve(local?.sidebarCollapsed ?? false, local?.sidebarCollapsedBesideTabs);
  }

  /// A manual toggle wins over a workspace's automatic collapse and is
  /// remembered (next to the terminal tab column: for that layout).
  void toggle() {
    final next = !state;
    _autoCollapsed = false;
    state = next;
    final settings = ref.read(settingsServiceProvider);
    final local = settings.currentLocal;
    settings
        .updateLocal(
          _besideTabs ? local.copyWith(sidebarCollapsedBesideTabs: next) : local.copyWith(sidebarCollapsed: next),
        )
        .ignore();
  }

  /// Wide workspaces (an open Kubernetes cluster) collapse the sidebar to
  /// icons; [restore] returns the user's choice.
  void autoCollapse() {
    if (!ref.mounted) return;
    _autoCollapsed = true;
    state = true;
  }

  void restore() {
    if (!ref.mounted || !_autoCollapsed) return;
    _autoCollapsed = false;
    _update();
  }

  /// The terminal with its tab column at the left edge is on screen.
  void setBesideTabs(bool value) {
    if (!ref.mounted || value == _besideTabs) return;
    _besideTabs = value;
    _update();
  }
}
