import 'dart:async';

import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/navigation_registry.dart';
import 'package:consolecrypt/app/platform.dart';
import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/navigation.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:flutter/gestures.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

final navigationPreferencesProvider = Provider<NavigationPreferences?>((ref) {
  final profile = ref.watch(activeProfileProvider)?.id.value;
  return ref.watch(localSettingsProvider).value?.navigationByProfile[profile];
});

/// This controller only changes visibility. It never navigates, disposes a
/// workspace, changes permissions or touches the synced vault settings.
final navigationPreferencesControllerProvider = Provider(NavigationPreferencesController.new);

final class NavigationPreferencesController {
  NavigationPreferencesController(this.ref);
  final Ref ref;

  Future<void> setVisible(String profileId, String id, bool visible) async {
    if (ref.read(appStageProvider) != AppStage.unlocked ||
        ref.read(activeProfileProvider)?.id.value != profileId ||
        !navigationSections.any((item) => item.id == id)) {
      return;
    }
    final settings = ref.read(settingsServiceProvider);
    final current = settings.currentLocal;
    final preferences = current.navigationByProfile[profileId] ?? _defaults();
    if (!preferences.supported) return;
    await settings.updateLocal(
      current.copyWith(
        navigationByProfile: {...current.navigationByProfile, profileId: preferences.withVisibility(id, visible)},
      ),
    );
  }

  Future<void> restore(String profileId) async {
    if (ref.read(appStageProvider) != AppStage.unlocked || ref.read(activeProfileProvider)?.id.value != profileId) {
      return;
    }
    final settings = ref.read(settingsServiceProvider);
    final current = settings.currentLocal;
    if (current.navigationByProfile[profileId]?.supported == false) return;
    // Retain unknown IDs for a later compatible client; they cannot add routes.
    final visibility = {
      ...?current.navigationByProfile[profileId]?.visibility,
      for (final item in navigationSections) item.id: item.defaultPinned,
    };
    await settings.updateLocal(
      current.copyWith(
        navigationByProfile: {
          ...current.navigationByProfile,
          profileId: NavigationPreferences(visibility: visibility),
        },
      ),
    );
  }

  NavigationPreferences _defaults() =>
      NavigationPreferences(visibility: {for (final item in navigationSections) item.id: item.defaultPinned});
}

/// Only wraps the navigation list, preserving profile/footer context actions.
class NavigationContextArea extends StatelessWidget {
  const NavigationContextArea({required this.child, super.key});
  final Widget child;

  Future<void> _menu(BuildContext context, Offset position) async {
    final selected = await showGlassMenu<bool>(
      context: context,
      anchor: Rect.fromLTWH(position.dx, position.dy, 1, 1),
      entries: [GlassMenuItem(value: true, label: context.l10n.navigationCustomize, icon: Icons.tune_rounded)],
    );
    if (selected == true && context.mounted) await showNavigationCustomization(context);
  }

  @override
  Widget build(BuildContext context) => Listener(
    onPointerDown: (event) {
      if (!AppPlatform.isMobile &&
          AppPlatform.usesMeta &&
          event.buttons == kPrimaryButton &&
          HardwareKeyboard.instance.isControlPressed) {
        // macOS Control-click is secondary activation. Cancel the underlying
        // primary gesture so a section is not opened while its menu appears.
        GestureBinding.instance.cancelPointer(event.pointer);
        unawaited(_menu(context, event.position));
      }
    },
    child: GestureDetector(
      behavior: HitTestBehavior.opaque,
      onSecondaryTapDown: AppPlatform.isMobile ? null : (details) => unawaited(_menu(context, details.globalPosition)),
      child: child,
    ),
  );
}

Future<void> showNavigationCustomization(BuildContext context) async {
  final ref = ProviderScope.containerOf(context, listen: false);
  final profile = ref.read(activeProfileProvider)?.id.value;
  if (profile == null || ref.read(appStageProvider) != AppStage.unlocked) return;
  await showGlassDialog<void>(context, builder: (_) => _NavigationCustomization(profileId: profile));
}

enum _NavigationAction { customize }

Future<void> showAllNavigationSections(BuildContext context, {required ValueChanged<ShellBranch> onSelect}) async {
  final container = ProviderScope.containerOf(context, listen: false);
  final profile = container.read(activeProfileProvider);
  if (profile == null || container.read(appStageProvider) != AppStage.unlocked) return;
  final result = await showGlassDialog<Object>(
    context,
    builder: (context) => Consumer(
      builder: (context, ref, _) {
        _closeOnProfileChange(context, ref, profile.id.value);
        final l = context.l10n;
        return GlassDialog(
          key: const ValueKey('all-navigation-sections'),
          title: l.navigationAllSections,
          content: SingleChildScrollView(
            child: Column(
              mainAxisSize: MainAxisSize.min,
              children: [
                for (final item in ref.watch(availableNavigationSectionsProvider))
                  if (!item.syncedOnly || profile.isSynced)
                    ListTile(
                      key: ValueKey('all-nav-${item.id}'),
                      leading: navigationSectionIcon(item),
                      title: Text(shellBranchLabel(item.branch, l)),
                      onTap: () => Navigator.of(context).pop(item.branch),
                    ),
              ],
            ),
          ),
          primaryAction: GlassButton(
            label: l.navigationCustomize,
            icon: Icons.tune_rounded,
            onPressed: () => Navigator.of(context).pop(_NavigationAction.customize),
          ),
        );
      },
    ),
  );
  if (!context.mounted ||
      container.read(appStageProvider) != AppStage.unlocked ||
      container.read(activeProfileProvider)?.id != profile.id) {
    return;
  }
  if (result is ShellBranch) onSelect(result);
  if (result == _NavigationAction.customize) await showNavigationCustomization(context);
}

void _closeOnProfileChange(BuildContext context, WidgetRef ref, String profileId) {
  void close() {
    if (context.mounted && ModalRoute.of(context)?.isCurrent == true) Navigator.of(context).pop();
  }

  ref.listen(activeProfileProvider, (_, next) {
    if (next?.id.value != profileId) close();
  });
  ref.listen(appStageProvider, (_, next) {
    if (next != AppStage.unlocked) close();
  });
}

class _NavigationCustomization extends ConsumerWidget {
  const _NavigationCustomization({required this.profileId});
  final String profileId;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    _closeOnProfileChange(context, ref, profileId);
    final l = context.l10n;
    final preferences = ref.watch(navigationPreferencesProvider);
    final synced = ref.watch(activeProfileProvider)?.isSynced ?? false;
    final editable = preferences?.supported ?? true;
    return GlassDialog(
      key: const ValueKey('navigation-customization'),
      title: l.navigationCustomize,
      content: SingleChildScrollView(
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            Text(editable ? l.navigationCustomizeHint : l.navigationFutureVersion),
            const SizedBox(height: 8),
            for (final item in ref.watch(availableNavigationSectionsProvider))
              if (!item.syncedOnly || synced)
                CheckboxListTile(
                  key: ValueKey('pin-nav-${item.id}'),
                  secondary: navigationSectionIcon(item),
                  title: Text(shellBranchLabel(item.branch, l)),
                  value: preferences?.visible(item.id, defaultVisible: item.defaultPinned) ?? item.defaultPinned,
                  onChanged: !editable
                      ? null
                      : (value) => runWithFeedback(
                          context,
                          () =>
                              ref.read(navigationPreferencesControllerProvider).setVisible(profileId, item.id, value!),
                        ),
                ),
          ],
        ),
      ),
      primaryAction: GlassButton(
        label: MaterialLocalizations.of(context).closeButtonLabel,
        onPressed: () => Navigator.of(context).pop(),
      ),
      secondaryActions: [
        GlassButton(
          key: const ValueKey('navigation-restore-defaults'),
          label: l.navigationRestore,
          icon: Icons.restore_rounded,
          onPressed: !editable
              ? null
              : () => runWithFeedback(
                  context,
                  () => ref.read(navigationPreferencesControllerProvider).restore(profileId),
                ),
        ),
      ],
    );
  }
}
