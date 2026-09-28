/// In-app "Glass" setting (LIQUID_GLASS_SPEC §2.2), device-local. Mirrors
/// Apple's 2026 transparency slider: Clear · Default · Tinted · Solid.
///
/// The effective rendering also honours the OS: Reduce Transparency forces
/// [solid] behaviour regardless of this value (`resolveEffectiveGlass`).
enum GlassMode {
  /// Tint −0.12 (floor 0.40) on static chrome; secure surfaces unchanged.
  clear('clear'),

  /// Token values ("Default" in the UI; `default` is a Dart keyword).
  standard('default'),

  /// Tint +0.14 (ceiling 0.94).
  tinted('tinted'),

  /// Opaque tint, no backdrop blur anywhere.
  solid('solid');

  const GlassMode(this.wireName);

  /// Persisted value in `local_settings`.
  final String wireName;

  static GlassMode fromWire(String? name) => values.where((m) => m.wireName == name).firstOrNull ?? standard;
}

/// Sidebar placement (LIQUID_GLASS_SPEC §2.5), device-local: the owner's
/// floating inset glass sidebar, or the macOS 27 edge-to-edge sidebar.
enum SidebarStyle {
  floating('floating'),
  edgeToEdge('edge_to_edge');

  const SidebarStyle(this.wireName);

  /// Persisted value in `local_settings`.
  final String wireName;

  static SidebarStyle fromWire(String? name) => values.where((s) => s.wireName == name).firstOrNull ?? floating;
}
