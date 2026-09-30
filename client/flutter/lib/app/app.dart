import 'dart:async';

import 'package:consolecrypt/app/about.dart';
import 'package:consolecrypt/app/app_info.dart';
import 'package:consolecrypt/app/commands.dart';
import 'package:consolecrypt/app/design_gallery_screen.dart';
import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/platform.dart';
import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/app/theme.dart';
import 'package:consolecrypt/app/theme/glass_tokens.dart';
import 'package:consolecrypt/core/glass/accessibility_bridge.dart';
import 'package:consolecrypt/core/glass/glass_scope.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/sharing/enrollment_automation.dart';
import 'package:consolecrypt/updates/update_settings.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

class ConsoleCryptApp extends ConsumerWidget {
  const ConsoleCryptApp({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final router = ref.watch(routerProvider);
    final preferences = ref.watch(localSettingsProvider).value ?? const LocalSettings();
    final themeMode = preferences.themeMode;
    final appLocale = ref.watch(localSettingsProvider.select((s) => s.value?.appLocale ?? AppLocale.system));
    // macOS Increase Contrast arrives over the native accessibility bridge;
    // the Windows high-contrast theme via MediaQuery (highContrast* themes).
    final increaseContrast = ref.watch(osIncreaseContrastProvider);
    final localProfile = ref.watch(activeProfileProvider.select((p) => p?.isLocal ?? false));
    return MaterialApp.router(
      title: kAppName,
      debugShowCheckedModeBanner: false,
      // Localization (ADR-0101): app strings + material_ui's Material/Cupertino/Widgets delegates.
      locale: localeFor(appLocale),
      supportedLocales: AppLocalizations.supportedLocales,
      localizationsDelegates: const [AppLocalizations.delegate, ...GlobalMaterialLocalizations.delegates],
      localeListResolutionCallback: (preferred, supported) => resolveSystemLocale(preferred, supported),
      theme: AppTheme.light(highContrast: increaseContrast, preferences: preferences),
      darkTheme: AppTheme.dark(highContrast: increaseContrast, preferences: preferences),
      highContrastTheme: AppTheme.light(highContrast: true, preferences: preferences),
      highContrastDarkTheme: AppTheme.dark(highContrast: true, preferences: preferences),
      themeMode: AppTheme.mode(themeMode),
      routerConfig: router,
      // Liquid Glass root: Glass setting × OS accessibility → GlassScope, the
      // static ambient backdrop under every route (transparent scaffolds) and,
      // on macOS, the unified transparent title bar — the shell and the gate
      // screens reserve the traffic-light area and route toolbar clicks
      // through `MacosToolbarPassthrough` (ADR-0101 §11).
      builder: (context, child) => GlassAppScope(
        ambient: !AppPlatform.isMobile,
        unifiedTitlebar: AppPlatform.isMacOS,
        localProfileCue: localProfile,
        child: ColoredBox(
          color: AppPlatform.isMobile ? GlassTokens.of(context).ambient.base : Colors.transparent,
          child: UpdateNoticeScope(
            child: EnrollmentAutomationScope(
              child: DesignGalleryShortcut(child: AppCommandsScope(child: child ?? const SizedBox.shrink())),
            ),
          ),
        ),
      ),
    );
  }
}

/// Global keyboard shortcuts (Cmd on macOS / Ctrl on Windows) and, on
/// macOS, the native menu bar exposing the same commands.
class AppCommandsScope extends ConsumerWidget {
  const AppCommandsScope({required this.child, super.key});

  final Widget child;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final dispatcher = ref.watch(appCommandDispatcherProvider);
    final unlocked = ref.watch(appStageProvider) == AppStage.unlocked;
    Widget result = Shortcuts(
      shortcuts: {for (final c in appCommands) c.activator: AppCommandIntent(c.id)},
      child: Actions(
        actions: {
          AppCommandIntent: CallbackAction<AppCommandIntent>(
            onInvoke: (intent) {
              dispatcher.invoke(intent.id);
              return null;
            },
          ),
        },
        child: child,
      ),
    );
    if (AppPlatform.isMacOS) {
      final l10n = context.l10n;
      PlatformMenuItem item(AppCommandId id) {
        final c = commandFor(id);
        return PlatformMenuItem(
          label: c.label(l10n),
          shortcut: c.activator,
          onSelected: unlocked ? () => dispatcher.invoke(id) : null,
        );
      }

      result = PlatformMenuBar(
        menus: [
          PlatformMenu(
            label: kAppName,
            menus: [
              PlatformMenuItemGroup(
                members: [
                  PlatformMenuItem(
                    label: l10n.aboutTitle(kAppName),
                    onSelected: () {
                      final context = rootNavigatorKey.currentContext;
                      if (context != null) unawaited(showAboutConsoleCrypt(context));
                    },
                  ),
                ],
              ),
              PlatformMenuItemGroup(members: [item(AppCommandId.openSettings), item(AppCommandId.lockVault)]),
              const PlatformMenuItemGroup(
                members: [
                  PlatformProvidedMenuItem(type: PlatformProvidedMenuItemType.hide),
                  PlatformProvidedMenuItem(type: PlatformProvidedMenuItemType.quit),
                ],
              ),
            ],
          ),
          PlatformMenu(
            label: l10n.menuFile,
            menus: [item(AppCommandId.newHost), item(AppCommandId.newTerminalTab), item(AppCommandId.closeTerminalTab)],
          ),
          PlatformMenu(label: l10n.menuView, menus: [item(AppCommandId.commandPalette), item(AppCommandId.snippets)]),
          PlatformMenu(
            label: l10n.menuWindow,
            menus: const [
              PlatformProvidedMenuItem(type: PlatformProvidedMenuItemType.minimizeWindow),
              PlatformProvidedMenuItem(type: PlatformProvidedMenuItemType.zoomWindow),
              PlatformProvidedMenuItem(type: PlatformProvidedMenuItemType.toggleFullScreen),
            ],
          ),
        ],
        child: result,
      );
    }
    return result;
  }
}
