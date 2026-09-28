import 'dart:async';

import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/app/theme/app_theme.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

// Debug-only developer tool: its strings are intentionally not localised.

/// Standalone entry point: `flutter run -d macos -t lib/app/design_gallery_screen.dart`.
void main() {
  WidgetsFlutterBinding.ensureInitialized();
  runApp(const ProviderScope(child: DesignGalleryApp()));
}

/// Opens the gallery on the root navigator (debug builds only).
void openDesignGallery() {
  if (!kDebugMode) return;
  final navigator = rootNavigatorKey.currentState;
  if (navigator == null) return;
  unawaited(navigator.push(MaterialPageRoute<void>(builder: (_) => const DesignGalleryScreen())));
}

/// Debug builds: ⌘⇧D (macOS) / Ctrl+Shift+D (Windows) opens the gallery.
class DesignGalleryShortcut extends StatelessWidget {
  const DesignGalleryShortcut({required this.child, super.key});

  final Widget child;

  static SingleActivator get activator => SingleActivator(
    LogicalKeyboardKey.keyD,
    shift: true,
    meta: defaultTargetPlatform == TargetPlatform.macOS,
    control: defaultTargetPlatform != TargetPlatform.macOS,
  );

  @override
  Widget build(BuildContext context) {
    if (!kDebugMode) return child;
    return CallbackShortcuts(bindings: {activator: openDesignGallery}, child: child);
  }
}

/// App wrapper for the standalone gallery (mock backend, real settings).
class DesignGalleryApp extends ConsumerWidget {
  const DesignGalleryApp({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final contrast = ref.watch(osIncreaseContrastProvider);
    return MaterialApp(
      title: 'ConsoleCrypt design gallery',
      debugShowCheckedModeBanner: false,
      theme: AppTheme.light(highContrast: contrast),
      darkTheme: AppTheme.dark(highContrast: contrast),
      highContrastTheme: AppTheme.light(highContrast: true),
      highContrastDarkTheme: AppTheme.dark(highContrast: true),
      builder: (context, child) => GlassAppScope(child: child ?? const SizedBox.shrink()),
      home: const DesignGalleryScreen(),
    );
  }
}

enum _Brightness { system, light, dark }

/// Every kit component in all variants, with light/dark/contrast, glass
/// mode, platform, motion and transparency toggles, sample content under
/// glass (including an opaque fake terminal) and the performance overlay.
class DesignGalleryScreen extends ConsumerStatefulWidget {
  const DesignGalleryScreen({super.key});

  @override
  ConsumerState<DesignGalleryScreen> createState() => _DesignGalleryScreenState();
}

class _DesignGalleryScreenState extends ConsumerState<DesignGalleryScreen> {
  _Brightness _brightness = _Brightness.system;
  bool _contrast = false;
  bool _reduceMotion = false;
  bool _reduceTransparency = false;
  TargetPlatform _platform = defaultTargetPlatform == TargetPlatform.windows
      ? TargetPlatform.windows
      : TargetPlatform.macOS;
  GlassSidebarStyle _sidebar = GlassSidebarStyle.floating;
  bool _overlay = false;
  bool _liveToolbar = false;
  bool _unifiedTitlebar = false;
  int _nav = 0;

  @override
  void dispose() {
    if (_unifiedTitlebar) unawaited(GlassWindowChrome.setUnifiedTitlebar(enabled: false));
    super.dispose();
  }

  Future<void> _setMode(GlassMode mode) async {
    final settings = ref.read(settingsServiceProvider);
    await settings.updateLocal(settings.currentLocal.copyWith(glassMode: mode));
  }

  Future<void> _setTitlebar(bool unified) async {
    final ok = await GlassWindowChrome.setUnifiedTitlebar(enabled: unified);
    if (mounted && ok) setState(() => _unifiedTitlebar = unified);
  }

  @override
  Widget build(BuildContext context) {
    final platformBrightness = MediaQuery.platformBrightnessOf(context);
    final brightness = switch (_brightness) {
      _Brightness.system => platformBrightness,
      _Brightness.light => Brightness.light,
      _Brightness.dark => Brightness.dark,
    };
    final base = GlassScope.of(context);
    final mode = ref.watch(glassModeProvider);
    final appearance = base.appearance.copyWith(
      mode: mode,
      increaseContrast: _contrast || base.appearance.increaseContrast,
      reduceMotion: _reduceMotion || base.appearance.reduceMotion,
      reduceTransparency: _reduceTransparency || base.appearance.reduceTransparency,
      tier: _reduceTransparency ? GlassTier.solid : null,
    );
    final theme = AppTheme.build(
      brightness,
      highContrast: appearance.increaseContrast || Theme.of(context).extension<GlassTokens>()?.highContrast == true,
      platform: _platform,
    );
    final scope = base.copyWith(
      appearance: appearance,
      solidReason: _reduceTransparency ? GlassSolidReason.reduceTransparency : null,
      unifiedTitlebar: _unifiedTitlebar,
    );

    final controls = _GalleryControls(
      brightness: _brightness,
      onBrightness: (v) => setState(() => _brightness = v),
      contrast: _contrast,
      onContrast: (v) => setState(() => _contrast = v),
      mode: mode,
      onMode: (m) => unawaited(_setMode(m)),
      platform: _platform,
      onPlatform: (p) => setState(() => _platform = p),
      reduceMotion: _reduceMotion,
      onReduceMotion: (v) => setState(() => _reduceMotion = v),
      reduceTransparency: _reduceTransparency,
      onReduceTransparency: (v) => setState(() => _reduceTransparency = v),
      sidebar: _sidebar,
      onSidebar: (v) => setState(() => _sidebar = v),
      overlay: _overlay,
      onOverlay: (v) => setState(() => _overlay = v),
      liveToolbar: _liveToolbar,
      onLiveToolbar: (v) => setState(() => _liveToolbar = v),
      unifiedTitlebar: _unifiedTitlebar,
      onUnifiedTitlebar: defaultTargetPlatform == TargetPlatform.macOS ? (v) => unawaited(_setTitlebar(v)) : null,
    );

    return Theme(
      data: theme,
      child: GlassScope(
        data: scope,
        child: Scaffold(
          backgroundColor: theme.extension<GlassTokens>()!.ambient.base,
          body: _GalleryShell(
            nav: _nav,
            onNav: (i) => setState(() => _nav = i),
            sidebarStyle: _sidebar,
            liveToolbar: _liveToolbar,
            overlay: _overlay,
            onClose: Navigator.of(context).canPop() ? () => Navigator.of(context).maybePop() : null,
            controls: controls,
          ),
        ),
      ),
    );
  }
}

const _navItems = [
  (Icons.dns_rounded, 'Hosts'),
  (Icons.terminal_rounded, 'Terminal'),
  (Icons.folder_copy_rounded, 'SFTP'),
  (Icons.code_rounded, 'Snippets'),
  (Icons.devices_rounded, 'Devices'),
  (Icons.settings_rounded, 'Settings'),
];

class _GalleryShell extends StatelessWidget {
  const _GalleryShell({
    required this.nav,
    required this.onNav,
    required this.sidebarStyle,
    required this.liveToolbar,
    required this.overlay,
    required this.onClose,
    required this.controls,
  });

  final int nav;
  final ValueChanged<int> onNav;
  final GlassSidebarStyle sidebarStyle;
  final bool liveToolbar;
  final bool overlay;
  final VoidCallback? onClose;
  final Widget controls;

  @override
  Widget build(BuildContext context) {
    final tokens = GlassTokens.of(context);
    final compact = MediaQuery.sizeOf(context).width < GlassSizes.compactBreakpoint;
    final inset = sidebarStyle == GlassSidebarStyle.floating ? tokens.radii.shellInset : 0.0;
    return Stack(
      children: [
        const Positioned.fill(child: AmbientBackdrop()),
        Row(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            GlassSidebar(
              style: sidebarStyle,
              compact: compact,
              header: compact
                  ? null
                  : const GlassCapsule(
                      height: 30,
                      child: Row(
                        mainAxisSize: MainAxisSize.min,
                        children: [
                          Icon(Icons.cloud_done_rounded, size: 16),
                          SizedBox(width: 6),
                          Flexible(child: Text('Personal', overflow: TextOverflow.ellipsis)),
                        ],
                      ),
                    ),
              footer: Align(
                alignment: compact ? Alignment.center : Alignment.centerLeft,
                child: GlassIconButton(
                  icon: Icons.lock_rounded,
                  tooltip: 'Lock vault',
                  style: GlassIconButtonStyle.plain,
                  onPressed: () {},
                ),
              ),
              children: [
                for (final (i, (icon, label)) in _navItems.indexed)
                  GlassSidebarItem(
                    key: ValueKey('gallery-nav-$i'),
                    icon: icon,
                    label: label,
                    compact: compact,
                    selected: nav == i,
                    badge: switch (i) {
                      1 => '3',
                      4 => '1',
                      _ => null,
                    },
                    badgeTone: i == 4 ? GlassTone.warning : GlassTone.neutral,
                    onPressed: () => onNav(i),
                  ),
              ],
            ),
            Expanded(
              child: Padding(
                padding: EdgeInsetsDirectional.only(end: inset),
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.stretch,
                  children: [
                    _GalleryToolbar(live: liveToolbar, onClose: onClose),
                    Expanded(
                      child: ScrollEdgeEffect(
                        child: SingleChildScrollView(
                          key: const ValueKey('gallery-scroll'),
                          padding: const EdgeInsets.fromLTRB(
                            GlassSpacing.pageCompact,
                            GlassSpacing.s8,
                            GlassSpacing.pageCompact,
                            GlassSpacing.s48,
                          ),
                          child: Column(
                            crossAxisAlignment: CrossAxisAlignment.stretch,
                            children: [
                              controls,
                              const SizedBox(height: GlassSpacing.section),
                              const GlassKitShowcase(),
                            ],
                          ),
                        ),
                      ),
                    ),
                  ],
                ),
              ),
            ),
          ],
        ),
        if (overlay) const Positioned(top: 60, right: 16, child: GlassPerfOverlay()),
      ],
    );
  }
}

class _GalleryToolbar extends StatelessWidget {
  const _GalleryToolbar({required this.live, required this.onClose});

  final bool live;
  final VoidCallback? onClose;

  @override
  Widget build(BuildContext context) {
    final tokens = GlassTokens.of(context);
    final backdrop = live ? BackdropMode.live : BackdropMode.static;
    return GlassToolbar(
      leading: [
        GlassToolbarGroup(
          backdrop: backdrop,
          children: [
            GlassIconButton(
              icon: onClose == null ? Icons.view_sidebar_rounded : Icons.arrow_back_rounded,
              tooltip: onClose == null ? 'Toggle sidebar' : 'Close gallery',
              style: GlassIconButtonStyle.plain,
              onPressed: onClose ?? () {},
            ),
            const GlassToolbarTitle('Design gallery'),
          ],
        ),
      ],
      center: ConstrainedBox(
        constraints: const BoxConstraints(maxWidth: 420),
        child: GlassCapsule(
          interactive: true,
          child: LayoutBuilder(
            builder: (context, constraints) => Row(
              children: [
                Icon(Icons.search_rounded, size: 18, color: tokens.secondaryLabel),
                const SizedBox(width: GlassSpacing.s6),
                Expanded(
                  child: Text(
                    'Search or run command',
                    overflow: TextOverflow.ellipsis,
                    style: tokens.typography.body.copyWith(color: tokens.secondaryLabel),
                  ),
                ),
                if (constraints.maxWidth > 200)
                  DecoratedBox(
                    decoration: ShapeDecoration(color: tokens.surfaces.inset, shape: GlassRadii.shape(tokens.radii.xs)),
                    child: Padding(
                      padding: const EdgeInsets.symmetric(horizontal: 6, vertical: 1),
                      child: Text(
                        tokens.platform == TargetPlatform.macOS ? '⌘K' : 'Ctrl+K',
                        style: tokens.typography.caption.copyWith(color: tokens.secondaryLabel),
                      ),
                    ),
                  ),
              ],
            ),
          ),
        ),
      ),
      trailing: [
        GlassButton.prominent(
          label: 'New connection',
          icon: Icons.add_rounded,
          size: GlassControlSize.lg,
          onPressed: () {},
        ),
        const GlassStatusPill(label: 'Synced', tone: GlassTone.success),
        GlassMenuButton<String>(
          entries: const [
            GlassMenuItem(value: 'settings', label: 'Settings', icon: Icons.settings_rounded, shortcut: '⌘,'),
            GlassMenuItem(value: 'lock', label: 'Lock vault', icon: Icons.lock_rounded, shortcut: '⌘L'),
          ],
          onSelected: (_) {},
          builder: (context, open) => GlassIconButton(
            icon: Icons.more_horiz_rounded,
            tooltip: GlassStrings.of(context).moreActions,
            onPressed: open,
          ),
        ),
      ],
    );
  }
}

class _GalleryControls extends StatelessWidget {
  const _GalleryControls({
    required this.brightness,
    required this.onBrightness,
    required this.contrast,
    required this.onContrast,
    required this.mode,
    required this.onMode,
    required this.platform,
    required this.onPlatform,
    required this.reduceMotion,
    required this.onReduceMotion,
    required this.reduceTransparency,
    required this.onReduceTransparency,
    required this.sidebar,
    required this.onSidebar,
    required this.overlay,
    required this.onOverlay,
    required this.liveToolbar,
    required this.onLiveToolbar,
    required this.unifiedTitlebar,
    required this.onUnifiedTitlebar,
  });

  final _Brightness brightness;
  final ValueChanged<_Brightness> onBrightness;
  final bool contrast;
  final ValueChanged<bool> onContrast;
  final GlassMode mode;
  final ValueChanged<GlassMode> onMode;
  final TargetPlatform platform;
  final ValueChanged<TargetPlatform> onPlatform;
  final bool reduceMotion;
  final ValueChanged<bool> onReduceMotion;
  final bool reduceTransparency;
  final ValueChanged<bool> onReduceTransparency;
  final GlassSidebarStyle sidebar;
  final ValueChanged<GlassSidebarStyle> onSidebar;
  final bool overlay;
  final ValueChanged<bool> onOverlay;
  final bool liveToolbar;
  final ValueChanged<bool> onLiveToolbar;
  final bool unifiedTitlebar;
  final ValueChanged<bool>? onUnifiedTitlebar;

  @override
  Widget build(BuildContext context) {
    Widget toggle(String key, String label, bool value, ValueChanged<bool>? onChanged, (String, String) names) =>
        _Labeled(
          label: label,
          child: GlassSegmented<bool>(
            key: ValueKey('gallery-$key'),
            inChrome: false,
            selected: value,
            onChanged: onChanged,
            segments: [
              GlassSegment(value: false, label: names.$1),
              GlassSegment(value: true, label: names.$2, key: ValueKey('gallery-$key-on')),
            ],
          ),
        );
    return _Section(
      title: 'Preview controls',
      subtitle: 'Glass mode writes the real device setting (SettingsService); the rest only affects this preview.',
      child: Wrap(
        spacing: GlassSpacing.s24,
        runSpacing: GlassSpacing.s12,
        children: [
          _Labeled(
            label: 'Appearance',
            child: GlassSegmented<_Brightness>(
              inChrome: false,
              selected: brightness,
              onChanged: onBrightness,
              segments: const [
                GlassSegment(value: _Brightness.system, label: 'System'),
                GlassSegment(value: _Brightness.light, label: 'Light', key: ValueKey('gallery-light')),
                GlassSegment(value: _Brightness.dark, label: 'Dark', key: ValueKey('gallery-dark')),
              ],
            ),
          ),
          _Labeled(
            label: GlassStrings.of(context).glassSetting,
            child: GlassSegmented<GlassMode>(
              inChrome: false,
              selected: mode,
              onChanged: onMode,
              segments: [
                for (final m in GlassMode.values)
                  GlassSegment(
                    value: m,
                    label: GlassStrings.of(context).glassMode(m),
                    key: ValueKey('gallery-mode-${m.name}'),
                  ),
              ],
            ),
          ),
          toggle('contrast', 'Contrast', contrast, onContrast, ('Standard', 'Increased')),
          _Labeled(
            label: 'Platform tokens',
            child: GlassSegmented<TargetPlatform>(
              inChrome: false,
              selected: platform,
              onChanged: onPlatform,
              segments: const [
                GlassSegment(value: TargetPlatform.macOS, label: 'macOS'),
                GlassSegment(value: TargetPlatform.windows, label: 'Windows', key: ValueKey('gallery-windows')),
              ],
            ),
          ),
          toggle('motion', 'Motion', reduceMotion, onReduceMotion, ('Full', 'Reduced')),
          toggle('transparency', 'Transparency', reduceTransparency, onReduceTransparency, ('System', 'Reduced')),
          _Labeled(
            label: 'Sidebar',
            child: GlassSegmented<GlassSidebarStyle>(
              inChrome: false,
              selected: sidebar,
              onChanged: onSidebar,
              segments: const [
                GlassSegment(value: GlassSidebarStyle.floating, label: 'Floating'),
                GlassSegment(value: GlassSidebarStyle.edgeToEdge, label: 'Edge', key: ValueKey('gallery-edge')),
              ],
            ),
          ),
          toggle('live-toolbar', 'Toolbar group', liveToolbar, onLiveToolbar, ('Static', 'Live')),
          toggle('overlay', 'Blur budget overlay', overlay, onOverlay, ('Off', 'On')),
          toggle('titlebar', 'macOS title bar', unifiedTitlebar, onUnifiedTitlebar, ('Standard', 'Unified')),
        ],
      ),
    );
  }
}

class _Labeled extends StatelessWidget {
  const _Labeled({required this.label, required this.child});

  final String label;
  final Widget child;

  @override
  Widget build(BuildContext context) {
    final tokens = GlassTokens.of(context);
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      mainAxisSize: MainAxisSize.min,
      children: [
        Text(label, style: tokens.typography.caption.copyWith(color: tokens.secondaryLabel)),
        const SizedBox(height: GlassSpacing.s4),
        child,
      ],
    );
  }
}

class _Section extends StatelessWidget {
  const _Section({required this.title, required this.child, this.subtitle, this.onContent = true});

  final String title;
  final String? subtitle;
  final Widget child;

  /// `false`: children sit directly on the ambient backdrop (glass demos).
  final bool onContent;

  @override
  Widget build(BuildContext context) {
    final tokens = GlassTokens.of(context);
    final header = Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text(title, style: tokens.typography.title2.copyWith(color: tokens.palette.label)),
        if (subtitle != null) ...[
          const SizedBox(height: GlassSpacing.s4),
          Text(subtitle!, style: tokens.typography.callout.copyWith(color: tokens.secondaryLabel)),
        ],
        const SizedBox(height: GlassSpacing.s12),
      ],
    );
    return Padding(
      padding: const EdgeInsets.only(bottom: GlassSpacing.section),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          header,
          if (onContent) ContentSurface(child: child) else child,
        ],
      ),
    );
  }
}

const _codeGroups = ['48213', '90377', '15642', '73019', '26485', '61930'];

/// All kit components in their variants (no Riverpod needed; used by the
/// gallery and by the widget tests).
class GlassKitShowcase extends StatelessWidget {
  const GlassKitShowcase({super.key});

  @override
  Widget build(BuildContext context) {
    final tokens = GlassTokens.of(context);
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        const _Section(
          title: 'Materials',
          subtitle: 'Static glass over vivid content, live (budgeted) glass, and the secure material.',
          onContent: false,
          child: _MaterialsDemo(),
        ),
        const _Section(title: 'Buttons', onContent: false, child: _ButtonsDemo()),
        const _Section(title: 'Buttons on a content card', child: _ButtonsDemo(compact: true)),
        const _Section(
          title: 'Segmented controls',
          onContent: false,
          child: Wrap(
            spacing: GlassSpacing.s24,
            runSpacing: GlassSpacing.s12,
            children: [_StatefulSegmented(inChrome: true), _StatefulSegmented(inChrome: false)],
          ),
        ),
        const _Section(title: 'Fields', child: _FieldsDemo()),
        const _Section(title: 'Menus, popovers, dialogs, sheets', onContent: false, child: _OverlaysDemo()),
        const _Section(title: 'Terminal tab strip', onContent: false, child: _TabsDemo()),
        const _Section(title: 'Toasts', onContent: false, child: _ToastsDemo()),
        const _Section(title: 'Badges and status', child: _BadgesDemo()),
        _Section(
          title: 'Secure surface',
          subtitle: 'Opaque, no blur, no refraction, no animation: approvals, secrets, risky confirmations.',
          onContent: false,
          child: SecureSurface(
            key: const ValueKey('gallery-secure'),
            radius: tokens.radii.dialog,
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text('Verification code', style: tokens.typography.title3.copyWith(color: tokens.palette.label)),
                const SizedBox(height: GlassSpacing.s12),
                const GlassVerificationCode(groups: _codeGroups),
                const SizedBox(height: GlassSpacing.s16),
                Text('Host key fingerprint', style: tokens.typography.caption.copyWith(color: tokens.secondaryLabel)),
                const SizedBox(height: GlassSpacing.s4),
                ContentSurface(
                  kind: ContentSurfaceKind.inset,
                  padding: const EdgeInsets.all(GlassSpacing.s8),
                  child: SelectableText(
                    'SHA256:nThbg6kXUpJWGl7E1IGOCspRomTxdCARLviKw6E5SY8',
                    style: tokens.typography.mono.copyWith(color: tokens.palette.label),
                  ),
                ),
              ],
            ),
          ),
        ),
        const _Section(title: 'Content surfaces', onContent: false, child: _SurfacesDemo()),
        const _Section(
          title: 'Terminal (always opaque)',
          subtitle: 'No persistent glass may overlap the terminal viewport.',
          onContent: false,
          child: _FakeTerminal(),
        ),
        const _Section(title: 'Scroll edges', child: _ScrollEdgesDemo()),
        const _Section(title: 'Typography', child: _TypographyDemo()),
        const _Section(title: 'Palette', child: _PaletteDemo()),
      ],
    );
  }
}

class _VividContent extends StatelessWidget {
  const _VividContent();

  @override
  Widget build(BuildContext context) {
    final tokens = GlassTokens.of(context);
    return DecoratedBox(
      decoration: ShapeDecoration(
        shape: GlassRadii.shape(tokens.radii.card),
        gradient: const LinearGradient(
          colors: [Color(0xFFFF6B6B), Color(0xFFFFD166), Color(0xFF06D6A0), Color(0xFF118AB2), Color(0xFF8338EC)],
        ),
      ),
      child: Padding(
        padding: const EdgeInsets.all(GlassSpacing.s12),
        child: Text(
          List.filled(40, 'content under glass').join(' · '),
          style: tokens.typography.bodyEmph.copyWith(color: const Color(0xFF111111)),
        ),
      ),
    );
  }
}

class _MaterialsDemo extends StatefulWidget {
  const _MaterialsDemo();

  @override
  State<_MaterialsDemo> createState() => _MaterialsDemoState();
}

class _MaterialsDemoState extends State<_MaterialsDemo> {
  /// Gallery-only budget so every material can be shown live side by side;
  /// it still obeys the app budget's suppression (device approval).
  GlassBackdropBudget? _budget;

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    final parent = GlassScope.of(context).budget;
    if (_budget?.parent != parent) {
      final old = _budget;
      // Children still hold the old budget until they rebuild this frame.
      if (old != null) WidgetsBinding.instance.addPostFrameCallback((_) => old.dispose());
      _budget = GlassBackdropBudget(maxChrome: 8, parent: parent);
    }
  }

  @override
  void dispose() {
    _budget?.dispose();
    super.dispose();
  }

  static const _variants = [GlassVariant.clear, GlassVariant.thin, GlassVariant.regular, GlassVariant.thick];

  @override
  Widget build(BuildContext context) {
    final tokens = GlassTokens.of(context);
    Widget swatch(Widget surface) => SizedBox(
      width: 168,
      child: ConstrainedBox(constraints: const BoxConstraints(minHeight: 96), child: surface),
    );
    Widget label(String title, String note) => Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      mainAxisSize: MainAxisSize.min,
      children: [
        Text(title, style: tokens.typography.bodyEmph.copyWith(color: tokens.palette.label)),
        Text(note, style: tokens.typography.callout.copyWith(color: tokens.secondaryLabel)),
      ],
    );
    Widget stage(Widget background, List<Widget> children) => ClipRSuperellipse(
      borderRadius: BorderRadius.circular(tokens.radii.card),
      child: Stack(
        children: [
          Positioned.fill(child: background),
          Padding(
            padding: const EdgeInsets.all(GlassSpacing.s16),
            child: Wrap(spacing: GlassSpacing.s16, runSpacing: GlassSpacing.s16, children: children),
          ),
        ],
      ),
    );
    final scope = GlassScope.of(context);
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        // How persistent chrome is drawn: static glass over the ambient backdrop.
        stage(const AmbientBackdrop(), [
          for (final v in _variants)
            swatch(
              GlassPanel(
                key: ValueKey('gallery-material-${v.name}'),
                variant: v,
                radius: tokens.radii.card,
                child: label('glass.${v.name}', 'static · over ambient'),
              ),
            ),
        ]),
        const SizedBox(height: GlassSpacing.s16),
        // Live (blurred; refractive on macOS) over busy content. The app budget
        // allows one live chrome surface; this demo gets its own budget.
        GlassScope(
          data: scope.copyWith(budget: _budget),
          child: stage(const _VividContent(), [
            for (final v in _variants)
              swatch(
                GlassPanel(
                  key: ValueKey('gallery-material-live-${v.name}'),
                  variant: v,
                  radius: tokens.radii.card,
                  backdrop: BackdropMode.live,
                  child: label('glass.${v.name}', 'live · over content'),
                ),
              ),
            swatch(
              SecureSurface(
                radius: tokens.radii.card,
                padding: const EdgeInsets.all(GlassSpacing.card),
                child: label('glass.secure', 'opaque · no blur'),
              ),
            ),
          ]),
        ),
      ],
    );
  }
}

class _ButtonsDemo extends StatelessWidget {
  const _ButtonsDemo({this.compact = false});

  final bool compact;

  @override
  Widget build(BuildContext context) {
    final sizes = compact ? const [GlassControlSize.md] : GlassControlSize.values;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        for (final size in sizes) ...[
          Wrap(
            spacing: GlassSpacing.s12,
            runSpacing: GlassSpacing.s12,
            crossAxisAlignment: WrapCrossAlignment.center,
            children: [
              for (final style in GlassButtonStyle.values)
                GlassButton(
                  key: ValueKey('gallery-button-${style.name}-${size.name}${compact ? '-card' : ''}'),
                  label: style.name,
                  icon: style == GlassButtonStyle.destructiveQuiet ? Icons.delete_rounded : null,
                  style: style,
                  size: size,
                  onPressed: () {},
                ),
              GlassButton(label: 'Disabled', size: size, onPressed: null),
              GlassButton.prominent(label: 'Save', busyLabel: 'Saving…', busy: true, size: size, onPressed: () {}),
            ],
          ),
          const SizedBox(height: GlassSpacing.s12),
        ],
        Wrap(
          spacing: GlassSpacing.s12,
          runSpacing: GlassSpacing.s12,
          children: [
            GlassIconButton(icon: Icons.add_rounded, tooltip: 'Add', onPressed: () {}),
            GlassIconButton(icon: Icons.star_rounded, tooltip: 'Selected', selected: true, onPressed: () {}),
            const GlassIconButton(icon: Icons.refresh_rounded, tooltip: 'Disabled', onPressed: null),
            GlassIconButton(
              icon: Icons.more_horiz_rounded,
              tooltip: 'Plain',
              style: GlassIconButtonStyle.plain,
              onPressed: () {},
            ),
          ],
        ),
      ],
    );
  }
}

class _StatefulSegmented extends StatefulWidget {
  const _StatefulSegmented({required this.inChrome});

  final bool inChrome;

  @override
  State<_StatefulSegmented> createState() => _StatefulSegmentedState();
}

class _StatefulSegmentedState extends State<_StatefulSegmented> {
  String _value = 'list';

  @override
  Widget build(BuildContext context) => GlassSegmented<String>(
    key: ValueKey('gallery-segmented-${widget.inChrome ? 'chrome' : 'content'}'),
    inChrome: widget.inChrome,
    selected: _value,
    onChanged: (v) => setState(() => _value = v),
    segments: const [
      GlassSegment(value: 'list', label: 'List', icon: Icons.view_list_rounded),
      GlassSegment(value: 'grid', label: 'Grid', icon: Icons.grid_view_rounded),
      GlassSegment(value: 'tree', label: 'Tree', icon: Icons.account_tree_rounded),
    ],
  );
}

class _FieldsDemo extends StatelessWidget {
  const _FieldsDemo();

  @override
  Widget build(BuildContext context) {
    Widget box(Widget child) => SizedBox(width: 280, child: child);
    return Wrap(
      spacing: GlassSpacing.s16,
      runSpacing: GlassSpacing.s16,
      children: [
        box(const GlassField(placeholder: 'Hostname (md)', fieldKey: ValueKey('gallery-field-md'))),
        box(const GlassField(placeholder: 'Large field', size: GlassFieldSize.lg)),
        box(const GlassField(placeholder: 'Search hosts', search: true, leadingIcon: Icons.search_rounded)),
        box(const GlassField(placeholder: 'Port', errorText: 'Port must be between 1 and 65535')),
        box(const GlassField(placeholder: 'Passphrase', secret: true, fieldKey: ValueKey('gallery-field-secret'))),
        box(const GlassField(placeholder: '/var/log', mono: true)),
        box(const GlassField(placeholder: 'Disabled', enabled: false)),
        box(const GlassField(placeholder: 'Notes (multi-line)', maxLines: 3)),
      ],
    );
  }
}

class _OverlaysDemo extends StatelessWidget {
  const _OverlaysDemo();

  @override
  Widget build(BuildContext context) {
    return Wrap(
      spacing: GlassSpacing.s12,
      runSpacing: GlassSpacing.s12,
      children: [
        GlassMenuButton<String>(
          entries: const [
            GlassMenuItem(value: 'connect', label: 'Connect', icon: Icons.play_arrow_rounded, shortcut: '↩'),
            GlassMenuItem(value: 'sftp', label: 'Open SFTP', icon: Icons.folder_copy_rounded),
            GlassMenuItem(value: 'copy', label: 'Copy address', icon: Icons.copy_rounded, shortcut: '⌘C'),
            GlassMenuItem(value: 'disabled', label: 'Disabled item', enabled: false),
            GlassMenuDivider(),
            GlassMenuItem(
              value: 'delete',
              label: 'Delete…',
              icon: Icons.delete_rounded,
              destructive: true,
              key: ValueKey('gallery-menu-delete'),
            ),
          ],
          onSelected: (v) => GlassToast.show(context, GlassToastRequest(message: 'Selected "$v"')),
          builder: (context, open) => GlassButton(
            key: const ValueKey('gallery-open-menu'),
            label: 'Menu',
            icon: Icons.more_horiz_rounded,
            onPressed: open,
          ),
        ),
        Builder(
          builder: (context) => GlassButton(
            key: const ValueKey('gallery-open-popover'),
            label: 'Popover',
            icon: Icons.info_outline_rounded,
            onPressed: () => unawaited(
              showGlassPopover<void>(
                context: context,
                anchor: glassAnchorRect(context),
                width: 260,
                builder: (context) {
                  final t = GlassTokens.of(context);
                  return Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    mainAxisSize: MainAxisSize.min,
                    children: [
                      Text('Synced 2 min ago', style: t.typography.bodyEmph.copyWith(color: t.palette.label)),
                      Text(
                        'sync.example.org · 0 pending',
                        style: t.typography.callout.copyWith(color: t.secondaryLabel),
                      ),
                      const SizedBox(height: GlassSpacing.s12),
                      GlassButton(label: 'Sync now', icon: Icons.sync_rounded, onPressed: () {}),
                    ],
                  );
                },
              ),
            ),
          ),
        ),
        GlassButton(
          key: const ValueKey('gallery-open-dialog'),
          label: 'Dialog',
          onPressed: () => unawaited(
            showGlassDialog<void>(
              context,
              builder: (context) => GlassDialog(
                title: 'Rename host',
                content: const GlassField(placeholder: 'Display name', autofocus: true),
                secondaryActions: [GlassButton.plain(label: 'Cancel', onPressed: () => Navigator.of(context).pop())],
                primaryAction: GlassButton.prominent(label: 'Save', onPressed: () => Navigator.of(context).pop()),
                onSubmit: () => Navigator.of(context).pop(),
              ),
            ),
          ),
        ),
        GlassButton(
          key: const ValueKey('gallery-open-secure'),
          label: 'Risky command',
          icon: Icons.dangerous_rounded,
          style: GlassButtonStyle.destructiveQuiet,
          onPressed: () => unawaited(
            showGlassDialog<void>(context, variant: GlassVariant.secure, builder: (_) => const _RiskyConfirm()),
          ),
        ),
        GlassButton(
          key: const ValueKey('gallery-open-approval'),
          label: 'Device approval',
          icon: Icons.verified_user_rounded,
          onPressed: () => unawaited(
            showGlassDialog<void>(
              context,
              variant: GlassVariant.secure,
              opaque: true,
              barrierDismissible: false,
              builder: (_) => const _ApprovalDemo(),
            ),
          ),
        ),
        GlassButton(
          key: const ValueKey('gallery-open-sheet'),
          label: 'Sheet',
          onPressed: () => unawaited(
            showGlassDialog<void>(
              context,
              sheet: true,
              builder: (context) => GlassDialog(
                title: 'Import hosts',
                content: const Text('Sheets attach under the toolbar on macOS and are centred dialogs on Windows.'),
                primaryAction: GlassButton.prominent(label: 'Done', onPressed: () => Navigator.of(context).pop()),
              ),
            ),
          ),
        ),
      ],
    );
  }
}

class _AckRow extends StatelessWidget {
  const _AckRow({required this.value, required this.onChanged, required this.label});

  final bool value;
  final ValueChanged<bool> onChanged;
  final String label;

  @override
  Widget build(BuildContext context) {
    final tokens = GlassTokens.of(context);
    return InkWell(
      onTap: () => onChanged(!value),
      child: Row(
        children: [
          Checkbox(value: value, onChanged: (v) => onChanged(v ?? false)),
          const SizedBox(width: GlassSpacing.s4),
          Expanded(
            child: Text(label, style: tokens.typography.body.copyWith(color: tokens.palette.label)),
          ),
        ],
      ),
    );
  }
}

class _RiskyConfirm extends StatefulWidget {
  const _RiskyConfirm();

  @override
  State<_RiskyConfirm> createState() => _RiskyConfirmState();
}

class _RiskyConfirmState extends State<_RiskyConfirm> {
  bool _ack = false;

  @override
  Widget build(BuildContext context) {
    final tokens = GlassTokens.of(context);
    return GlassDialog(
      width: 560,
      icon: Icons.dangerous_rounded,
      iconTone: GlassTone.danger,
      title: 'Run on prod-db-1?',
      content: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        mainAxisSize: MainAxisSize.min,
        children: [
          Wrap(
            spacing: GlassSpacing.s8,
            crossAxisAlignment: WrapCrossAlignment.center,
            children: [
              const GlassRiskBadge(risk: RiskLevel.destructive, label: 'Destructive'),
              Text('deploy@prod-db-1', style: tokens.typography.bodyEmph.copyWith(color: tokens.palette.label)),
            ],
          ),
          const SizedBox(height: GlassSpacing.s12),
          ContentSurface(
            kind: ContentSurfaceKind.inset,
            padding: const EdgeInsets.all(GlassSpacing.s12),
            child: Text(
              'sudo systemctl restart postgresql',
              style: tokens.typography.mono.copyWith(color: tokens.palette.label),
            ),
          ),
          const SizedBox(height: GlassSpacing.s12),
          _AckRow(
            value: _ack,
            onChanged: (v) => setState(() => _ack = v),
            label: 'I understand this restarts the database on prod-db-1',
          ),
        ],
      ),
      secondaryActions: [
        GlassButton.plain(label: 'Cancel', autofocus: true, onPressed: () => Navigator.of(context).pop()),
      ],
      primaryAction: GlassButton.destructive(label: 'Run', onPressed: _ack ? () => Navigator.of(context).pop() : null),
    );
  }
}

class _ApprovalDemo extends StatefulWidget {
  const _ApprovalDemo();

  @override
  State<_ApprovalDemo> createState() => _ApprovalDemoState();
}

class _ApprovalDemoState extends State<_ApprovalDemo> {
  bool _match = false;

  @override
  Widget build(BuildContext context) {
    final tokens = GlassTokens.of(context);
    return GlassDialog(
      width: 600,
      icon: Icons.laptop_mac_rounded,
      title: 'Approve "Work MacBook"?',
      content: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        mainAxisSize: MainAxisSize.min,
        children: [
          Text(
            'alice@example.org · requested 2 minutes ago',
            style: tokens.typography.callout.copyWith(color: tokens.secondaryLabel),
          ),
          const SizedBox(height: GlassSpacing.s16),
          const GlassVerificationCode(key: ValueKey('gallery-approval-code'), groups: _codeGroups),
          const SizedBox(height: GlassSpacing.s16),
          const Text('1. Look at the code on Work MacBook'),
          const Text('2. Compare every group'),
          const SizedBox(height: GlassSpacing.s8),
          _AckRow(
            value: _match,
            onChanged: (v) => setState(() => _match = v),
            label: 'All 6 groups match the code shown on Work MacBook',
          ),
        ],
      ),
      leadingAction: GlassButton(
        label: "Codes don't match",
        style: GlassButtonStyle.destructiveQuiet,
        onPressed: () => Navigator.of(context).pop(),
      ),
      secondaryActions: [
        GlassButton.plain(label: 'Cancel', autofocus: true, onPressed: () => Navigator.of(context).pop()),
      ],
      primaryAction: GlassButton.prominent(
        key: const ValueKey('gallery-approve'),
        label: 'Approve',
        onPressed: _match ? () => Navigator.of(context).pop() : null,
      ),
    );
  }
}

class _TabsDemo extends StatefulWidget {
  const _TabsDemo();

  @override
  State<_TabsDemo> createState() => _TabsDemoState();
}

class _TabsDemoState extends State<_TabsDemo> {
  final List<GlassTab> _tabs = [
    const GlassTab(title: 'bastion-a', subtitle: 'deploy', state: GlassTabState.connected, tooltip: 'Connected'),
    const GlassTab(title: 'prod-db-1', subtitle: 'root', state: GlassTabState.reconnecting, tooltip: 'Reconnecting…'),
    const GlassTab(title: 'staging-web', subtitle: 'ci', state: GlassTabState.disconnected, tooltip: 'Disconnected'),
    const GlassTab(title: 'nas.local', hostColor: Color(0xFF8338EC)),
  ];
  int _active = 0;

  @override
  Widget build(BuildContext context) => GlassTabStrip(
    tabs: _tabs,
    activeIndex: _active,
    onSelect: (i) => setState(() => _active = i),
    onClose: (i) => setState(() {
      _tabs.removeAt(i);
      if (_active >= _tabs.length) _active = _tabs.length - 1;
    }),
    onAdd: () => setState(() => _tabs.add(GlassTab(title: 'host-${_tabs.length + 1}'))),
    trailing: GlassToolbarGroup(
      children: [
        GlassIconButton(
          icon: Icons.folder_copy_rounded,
          tooltip: 'SFTP',
          style: GlassIconButtonStyle.plain,
          size: 28,
          iconSize: 18,
          onPressed: () {},
        ),
        GlassIconButton(
          icon: Icons.vertical_split_rounded,
          tooltip: 'Split',
          style: GlassIconButtonStyle.plain,
          size: 28,
          iconSize: 18,
          onPressed: () {},
        ),
      ],
    ),
  );
}

class _ToastsDemo extends StatelessWidget {
  const _ToastsDemo();

  @override
  Widget build(BuildContext context) {
    void show(GlassToastRequest r) => GlassToast.show(context, r);
    return Wrap(
      spacing: GlassSpacing.s12,
      runSpacing: GlassSpacing.s12,
      children: [
        GlassButton(
          key: const ValueKey('gallery-toast-info'),
          label: 'Info',
          onPressed: () => show(const GlassToastRequest(message: 'Host saved', icon: Icons.info_rounded)),
        ),
        GlassButton(
          label: 'Success + action',
          onPressed: () => show(
            GlassToastRequest(
              message: 'Snippet deleted',
              tone: GlassTone.success,
              icon: Icons.check_circle_rounded,
              actionLabel: 'Undo',
              onAction: () {},
            ),
          ),
        ),
        GlassButton(
          key: const ValueKey('gallery-toast-error'),
          label: 'Error (stays)',
          onPressed: () => show(
            const GlassToastRequest(
              message: 'Sync failed: server unreachable',
              tone: GlassTone.danger,
              icon: Icons.error_rounded,
            ),
          ),
        ),
        GlassButton(
          label: 'Clipboard',
          onPressed: () => show(
            const GlassToastRequest(
              message: 'Password copied',
              tone: GlassTone.accent,
              countdown: Duration(seconds: 30),
            ),
          ),
        ),
      ],
    );
  }
}

class _BadgesDemo extends StatelessWidget {
  const _BadgesDemo();

  @override
  Widget build(BuildContext context) {
    const labels = {
      RiskLevel.readOnly: 'Read-only',
      RiskLevel.unknown: 'Unknown',
      RiskLevel.modifying: 'Modifying',
      RiskLevel.destructive: 'Destructive',
    };
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Wrap(
          spacing: GlassSpacing.s8,
          runSpacing: GlassSpacing.s8,
          children: [
            for (final e in labels.entries) GlassRiskBadge(risk: e.key, label: e.value),
            for (final e in labels.entries) GlassRiskBadge(risk: e.key, label: e.value, dense: true),
          ],
        ),
        const SizedBox(height: GlassSpacing.s12),
        Wrap(
          spacing: GlassSpacing.s8,
          runSpacing: GlassSpacing.s8,
          children: [for (final t in GlassTone.values) GlassBadge(label: t.name, tone: t)],
        ),
        const SizedBox(height: GlassSpacing.s12),
        const Wrap(
          spacing: GlassSpacing.s8,
          runSpacing: GlassSpacing.s8,
          children: [
            GlassStatusPill(label: 'Synced', tone: GlassTone.success),
            GlassStatusPill(label: 'Syncing…', tone: GlassTone.accent, pulsing: true),
            GlassStatusPill(label: 'Offline', tone: GlassTone.warning, icon: Icons.cloud_off_rounded),
            GlassStatusPill(label: 'Sync error', tone: GlassTone.danger),
            GlassStatusPill(label: 'Local only', tone: GlassTone.neutral, icon: Icons.lock_rounded),
          ],
        ),
      ],
    );
  }
}

class _SurfacesDemo extends StatelessWidget {
  const _SurfacesDemo();

  @override
  Widget build(BuildContext context) {
    final tokens = GlassTokens.of(context);
    return Wrap(
      spacing: GlassSpacing.s16,
      runSpacing: GlassSpacing.s16,
      children: [
        for (final kind in ContentSurfaceKind.values)
          SizedBox(
            width: 200,
            child: ContentSurface(
              kind: kind,
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text('surface.${kind.name}', style: tokens.typography.bodyEmph.copyWith(color: tokens.palette.label)),
                  Text('secondary text', style: tokens.typography.callout.copyWith(color: tokens.secondaryLabel)),
                  Text('tertiary text', style: tokens.typography.callout.copyWith(color: tokens.palette.tertiary)),
                ],
              ),
            ),
          ),
      ],
    );
  }
}

class _FakeTerminal extends StatelessWidget {
  const _FakeTerminal();

  static const _lines = [
    r'deploy@bastion-a:~$ journalctl -u nginx --since "5 min ago"',
    'Sep 26 10:41:02 bastion-a nginx[812]: 10.0.4.17 - - "GET /healthz HTTP/1.1" 200 2',
    'Sep 26 10:41:07 bastion-a nginx[812]: 10.0.4.22 - - "POST /api/v1/sync HTTP/1.1" 204 0',
    'Sep 26 10:41:12 bastion-a nginx[812]: upstream response time 0.018 s',
    r'deploy@bastion-a:~$ _',
  ];

  @override
  Widget build(BuildContext context) {
    final tokens = GlassTokens.of(context);
    return ContentSurface(
      key: const ValueKey('gallery-terminal'),
      kind: ContentSurfaceKind.solid,
      color: tokens.surfaces.terminalBackground,
      padding: const EdgeInsets.all(10),
      child: RepaintBoundary(
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            for (final line in _lines)
              Text(
                line,
                maxLines: 1,
                overflow: TextOverflow.clip,
                softWrap: false,
                style: tokens.typography.mono.copyWith(color: tokens.surfaces.terminalForeground),
              ),
          ],
        ),
      ),
    );
  }
}

class _ScrollEdgesDemo extends StatelessWidget {
  const _ScrollEdgesDemo();

  @override
  Widget build(BuildContext context) {
    final tokens = GlassTokens.of(context);
    Widget list() => ListView(
      children: [
        for (var i = 0; i < 20; i++)
          SizedBox(
            height: GlassSizes.listRowDense,
            child: Align(
              alignment: Alignment.centerLeft,
              child: Text('row $i', style: tokens.typography.body.copyWith(color: tokens.palette.label)),
            ),
          ),
      ],
    );
    return SizedBox(
      height: 160,
      child: Row(
        children: [
          Expanded(child: ScrollEdgeEffect(bottom: true, child: list())),
          const SizedBox(width: GlassSpacing.s16),
          Expanded(
            child: ScrollEdgeEffect.hard(
              header: Padding(
                padding: const EdgeInsets.all(GlassSpacing.s6),
                child: Text(
                  'Name · Size · Modified',
                  style: tokens.typography.caption.copyWith(color: tokens.secondaryLabel),
                ),
              ),
              child: list(),
            ),
          ),
        ],
      ),
    );
  }
}

class _TypographyDemo extends StatelessWidget {
  const _TypographyDemo();

  @override
  Widget build(BuildContext context) {
    final tokens = GlassTokens.of(context);
    final t = tokens.typography;
    final styles = {
      'largeTitle': t.largeTitle,
      'title1': t.title1,
      'title2': t.title2,
      'title3': t.title3,
      'body': t.body,
      'bodyEmph': t.bodyEmph,
      'callout': t.callout,
      'caption': t.caption,
      'button': t.button,
      'mono': t.mono,
      'code': t.code,
    };
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        for (final e in styles.entries)
          Text(
            '${e.key} ${e.value.fontSize?.toStringAsFixed(0)}',
            maxLines: 1,
            overflow: TextOverflow.ellipsis,
            style: e.value.copyWith(color: tokens.palette.label),
          ),
      ],
    );
  }
}

class _PaletteDemo extends StatelessWidget {
  const _PaletteDemo();

  @override
  Widget build(BuildContext context) {
    final tokens = GlassTokens.of(context);
    final p = tokens.palette;
    final roles = {
      'label': p.label,
      'secondary': p.secondary,
      'tertiary': p.tertiary,
      'accent': p.accent,
      'accentFill': p.accentFill,
      'info': p.info,
      'warning': p.warning,
      'danger': p.danger,
      'unknown': p.unknown,
      'success': p.success,
    };
    return Wrap(
      spacing: GlassSpacing.s8,
      runSpacing: GlassSpacing.s8,
      children: [
        for (final e in roles.entries)
          Row(
            mainAxisSize: MainAxisSize.min,
            children: [
              SizedBox.square(
                dimension: 16,
                child: DecoratedBox(
                  decoration: BoxDecoration(color: e.value, shape: BoxShape.circle),
                ),
              ),
              const SizedBox(width: GlassSpacing.s4),
              Text(e.key, style: tokens.typography.callout.copyWith(color: e.value)),
            ],
          ),
      ],
    );
  }
}
