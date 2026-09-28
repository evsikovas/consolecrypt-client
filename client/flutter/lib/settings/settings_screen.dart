import 'package:consolecrypt/account/profile_switcher.dart';
import 'package:consolecrypt/app/about.dart';
import 'package:consolecrypt/app/app_info.dart';
import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/l10n/labels.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/core/widgets/language_picker.dart';
import 'package:consolecrypt/settings/ai_provider_dialog.dart';
import 'package:consolecrypt/settings/appearance_controls.dart';
import 'package:consolecrypt/settings/device_unlock_settings.dart';
import 'package:consolecrypt/settings/screen_capture_settings.dart';
import 'package:consolecrypt/settings/settings_dialogs.dart';
import 'package:consolecrypt/settings/sftp_settings.dart';
import 'package:consolecrypt/sync/enable_sync_dialog.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import 'package:material_ui/material_ui.dart';

class SettingsScreen extends ConsumerWidget {
  const SettingsScreen({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    return PageScaffold(
      title: context.l10n.settingsTitle,
      scrollable: true,
      maxWidth: 900,
      body: const Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          _ProfilesSection(),
          SizedBox(height: GlassSpacing.s16),
          _AccountSyncSection(),
          SizedBox(height: GlassSpacing.s16),
          _VaultSection(),
          SizedBox(height: GlassSpacing.s16),
          _AppearanceSection(),
          SizedBox(height: GlassSpacing.s16),
          _SecuritySection(),
          SizedBox(height: GlassSpacing.s16),
          _AiSection(),
          SizedBox(height: GlassSpacing.s16),
          _TerminalSection(),
          SizedBox(height: GlassSpacing.s16),
          SftpSettingsSection(),
          SizedBox(height: GlassSpacing.s16),
          _AboutSection(),
        ],
      ),
    );
  }
}

class _ProfilesSection extends ConsumerWidget {
  const _ProfilesSection();

  Future<void> _rename(BuildContext context, WidgetRef ref, Profile p) async {
    final name = await showTextInputDialog(context, title: context.l10n.settingsRenameProfileTitle, initial: p.name);
    if (name != null && context.mounted) {
      await runWithFeedback(context, () => ref.read(profileServiceProvider).rename(p.id, name));
    }
  }

  Future<void> _delete(BuildContext context, WidgetRef ref, Profile p) => confirmAndDeleteProfile(context, ref, p);

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final l10n = context.l10n;
    final state = ref.watch(profilesProvider).value ?? ProfilesState.empty;
    return SectionCard(
      title: l10n.settingsProfilesTitle,
      icon: Icons.manage_accounts_rounded,
      subtitle: l10n.settingsProfilesSubtitle,
      trailing: GlassButton.plain(
        onPressed: () => context.go(AppRoutes.welcome),
        icon: Icons.add_rounded,
        label: l10n.settingsAddProfile,
      ),
      child: Column(
        children: [
          for (final p in state.profiles)
            ListTile(
              contentPadding: EdgeInsets.zero,
              leading: Icon(profileIcon(p)),
              title: Text(p.id == state.activeId ? l10n.settingsProfileActive(p.name) : p.name),
              subtitle: Text(p.localizedSubtitle(l10n)),
              trailing: Wrap(
                spacing: GlassSpacing.s2,
                crossAxisAlignment: WrapCrossAlignment.center,
                children: [
                  if (p.id != state.activeId)
                    GlassButton.plain(
                      size: GlassControlSize.sm,
                      onPressed: () => switchProfile(context, ref, p),
                      label: l10n.settingsProfileSwitch,
                    ),
                  GlassIconButton(
                    tooltip: l10n.commonRename,
                    icon: Icons.edit_rounded,
                    style: GlassIconButtonStyle.plain,
                    onPressed: () => _rename(context, ref, p),
                  ),
                  GlassIconButton(
                    tooltip: l10n.commonRemove,
                    icon: Icons.delete_rounded,
                    style: GlassIconButtonStyle.plain,
                    onPressed: () => _delete(context, ref, p),
                  ),
                ],
              ),
            ),
        ],
      ),
    );
  }
}

class _AccountSyncSection extends ConsumerWidget {
  const _AccountSyncSection();

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final l10n = context.l10n;
    final profile = ref.watch(activeProfileProvider);
    final session = ref.watch(authStateProvider).value?.session;
    if (profile == null) return const SizedBox.shrink();
    if (profile.isLocal) {
      return SectionCard(
        title: l10n.settingsSyncTitle,
        icon: Icons.cloud_off_rounded,
        subtitle: l10n.settingsSyncLocalSubtitle,
        child: Row(
          children: [
            Expanded(child: Text(l10n.settingsSyncLocalBody)),
            const SizedBox(width: GlassSpacing.s12),
            GlassButton(
              key: const ValueKey('settings-enable-sync'),
              onPressed: () => showEnableSyncWizard(context),
              icon: Icons.cloud_upload_rounded,
              label: l10n.settingsEnableSync,
            ),
          ],
        ),
      );
    }
    return SectionCard(
      title: l10n.settingsAccountSyncTitle,
      icon: Icons.cloud_rounded,
      child: Column(
        children: [
          LabeledValue(label: l10n.settingsServerLabel, value: SelectableText(profile.serverUrl?.toString() ?? '—')),
          LabeledValue(label: l10n.settingsAccountLabel, value: Text(profile.accountEmail ?? '—')),
          LabeledValue(label: l10n.settingsThisDeviceLabel, value: Text(session?.deviceName ?? '—')),
          const SizedBox(height: GlassSpacing.s8),
          Wrap(
            spacing: GlassSpacing.s8,
            runSpacing: GlassSpacing.s8,
            children: [
              GlassButton(
                onPressed: () => showChangeAccountPasswordDialog(context),
                label: l10n.settingsChangeAccountPassword,
              ),
              GlassButton(
                key: const ValueKey('settings-disconnect'),
                onPressed: () => showDisconnectDialog(context, ref),
                label: l10n.settingsDisconnect,
              ),
              GlassButton.plain(
                onPressed: () async {
                  await ref.read(authServiceProvider).logout();
                  if (context.mounted) context.go(AppRoutes.hosts);
                },
                label: l10n.settingsSignOut,
              ),
            ],
          ),
          const SizedBox(height: GlassSpacing.s8),
          Text(l10n.settingsDifferentServerHint, style: GlassTokens.of(context).typography.callout),
        ],
      ),
    );
  }
}

class _VaultSection extends ConsumerWidget {
  const _VaultSection();

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final l10n = context.l10n;
    final vault = ref.watch(vaultSettingsProvider).value;
    final status = ref.watch(vaultStatusProvider).value;
    final local = ref.watch(localSettingsProvider).value ?? const LocalSettings();
    final settings = ref.read(settingsServiceProvider);
    return SectionCard(
      title: l10n.settingsVaultTitle,
      icon: Icons.shield_rounded,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          LabeledValue(label: l10n.settingsVaultNameLabel, value: Text(vault?.vaultName ?? status?.vaultName ?? '—')),
          if (status?.vaultId != null)
            LabeledValue(
              label: l10n.settingsVaultIdLabel,
              value: SelectableText(
                status!.vaultId!.value,
                style: GlassTokens.of(context).typography.mono.copyWith(fontSize: 12),
              ),
            ),
          LabeledValue(
            label: l10n.settingsAutoLockLabel,
            value: Align(
              alignment: Alignment.centerLeft,
              child: GlassSelect<int>(
                key: const ValueKey('auto-lock'),
                value: const [0, 5, 15, 60].contains(local.autoLockMinutes) ? local.autoLockMinutes : 15,
                items: [
                  GlassSelectItem(value: 5, label: l10n.settingsAutoLockAfterMinutes(5)),
                  GlassSelectItem(value: 15, label: l10n.settingsAutoLockAfterMinutes(15)),
                  GlassSelectItem(value: 60, label: l10n.settingsAutoLockAfterHours(1)),
                  GlassSelectItem(value: 0, label: l10n.settingsAutoLockNever),
                ],
                // TODO(client/ui): inactivity timer lives in app-core (it also locks while the
                // window is hidden); next: wire the setting through at M4.
                onChanged: (v) => settings.updateLocal(local.copyWith(autoLockMinutes: v)),
              ),
            ),
          ),
          const SizedBox(height: GlassSpacing.s8),
          const DeviceUnlockSettings(),
          const SizedBox(height: GlassSpacing.s12),
          Wrap(
            spacing: GlassSpacing.s8,
            runSpacing: GlassSpacing.s8,
            children: [
              GlassButton(
                icon: Icons.key_rounded,
                onPressed: () => showChangePassphraseDialog(context),
                label: l10n.settingsChangePassphrase,
              ),
              GlassButton(
                icon: Icons.health_and_safety_rounded,
                onPressed: () => showRegenerateKitDialog(context),
                label: l10n.settingsNewRecoveryKit,
              ),
              GlassButton(
                icon: Icons.save_alt_rounded,
                onPressed: () => context.go(AppRoutes.backups),
                label: l10n.settingsBackups,
              ),
              GlassButton(
                icon: Icons.lock_rounded,
                onPressed: () => ref.read(vaultServiceProvider).lock(),
                label: l10n.settingsLockNow,
              ),
            ],
          ),
        ],
      ),
    );
  }
}

class _AiSection extends ConsumerWidget {
  const _AiSection();

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final l10n = context.l10n;
    final providers = ref.watch(aiProvidersProvider).value ?? const <AiProviderConfig>[];
    final vault = ref.watch(vaultSettingsProvider).value;
    final settings = ref.read(settingsServiceProvider);
    return SectionCard(
      title: l10n.settingsAiProvidersTitle,
      icon: Icons.auto_awesome_rounded,
      subtitle: l10n.settingsAiProvidersSubtitle,
      trailing: GlassButton.plain(
        key: const ValueKey('add-provider'),
        onPressed: () => showAiProviderDialog(context),
        icon: Icons.add_rounded,
        label: l10n.commonAdd,
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          for (final p in providers)
            ListTile(
              key: ValueKey('provider-${p.name}'),
              contentPadding: EdgeInsets.zero,
              leading: Icon(p.isRemote ? Icons.public_rounded : Icons.computer_rounded),
              title: Text(p.isDefault ? l10n.settingsAiProviderDefault(p.name) : p.name),
              subtitle: Text(
                [
                  p.provider.localized(l10n),
                  p.baseUrl,
                  p.chatModel,
                  p.privacyProfile.localized(l10n),
                  if (p.hasApiKey) l10n.settingsAiProviderApiKeyStored,
                ].join(' · '),
              ),
              trailing: const Icon(Icons.chevron_right_rounded),
              onTap: () => showAiProviderDialog(context, provider: p),
            ),
          if (vault != null) ...[
            const Divider(height: GlassSpacing.s24),
            LabeledValue(
              label: l10n.settingsDefaultPrivacyLabel,
              value: Align(
                alignment: AlignmentDirectional.centerStart,
                child: GlassSegmented<PrivacyProfile>(
                  key: const ValueKey('default-privacy'),
                  inChrome: false,
                  segments: [for (final p in PrivacyProfile.values) GlassSegment(value: p, label: p.localized(l10n))],
                  selected: vault.defaultPrivacyProfile,
                  onChanged: (p) => settings.updateVault(vault.copyWith(defaultPrivacyProfile: p)),
                ),
              ),
            ),
            SwitchListTile.adaptive(
              contentPadding: EdgeInsets.zero,
              value: vault.syncAiConversations,
              onChanged: (v) => settings.updateVault(vault.copyWith(syncAiConversations: v)),
              title: Text(l10n.settingsKeepAiConversations),
              subtitle: Text(l10n.settingsKeepAiConversationsHelp),
            ),
          ],
        ],
      ),
    );
  }
}

class _TerminalSection extends ConsumerWidget {
  const _TerminalSection();

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final l10n = context.l10n;
    final vault = ref.watch(vaultSettingsProvider).value;
    final local = ref.watch(localSettingsProvider).value ?? const LocalSettings();
    final settings = ref.read(settingsServiceProvider);
    final synced = ref.watch(activeProfileProvider)?.isSynced ?? false;
    return SectionCard(
      key: const ValueKey('settings-terminal-section'),
      title: l10n.settingsTerminalTitle,
      icon: Icons.terminal_rounded,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          if (vault != null) ...[
            Text(
              l10n.settingsCommandHistoryLabel,
              style: GlassTokens.of(context).typography.bodyEmph.copyWith(color: GlassTokens.of(context).palette.label),
            ),
            const SizedBox(height: GlassSpacing.s6),
            GlassSegmented<TerminalHistoryMode>(
              key: const ValueKey('history-mode'),
              inChrome: false,
              expand: true,
              segments: [for (final m in TerminalHistoryMode.values) GlassSegment(value: m, label: m.localized(l10n))],
              selected: vault.terminalHistoryMode,
              onChanged: (m) => settings.updateVault(vault.copyWith(terminalHistoryMode: m)),
            ),
            const SizedBox(height: GlassSpacing.s4),
            Text(
              !synced && vault.terminalHistoryMode == TerminalHistoryMode.encryptedSync
                  ? l10n.settingsHistoryPendingSync(vault.terminalHistoryMode.localizedDescription(l10n))
                  : vault.terminalHistoryMode.localizedDescription(l10n),
              style: GlassTokens.of(context).typography.callout.copyWith(color: GlassTokens.of(context).secondaryLabel),
            ),
            const SizedBox(height: GlassSpacing.s12),
          ],
          LabeledValue(
            label: l10n.settingsFontSizeLabel,
            value: Slider(
              key: const ValueKey('terminal-font-size'),
              value: local.terminalFontSize.clamp(10, 22),
              min: 10,
              max: 22,
              divisions: 12,
              label: local.terminalFontSize.toStringAsFixed(0),
              onChanged: (v) => settings.updateLocal(local.copyWith(terminalFontSize: v)),
            ),
          ),
          const TerminalAppearanceControls(),
          LabeledValue(
            label: l10n.settingsScrollbackLabel,
            value: Align(
              alignment: Alignment.centerLeft,
              child: GlassSelect<int>(
                key: const ValueKey('scrollback'),
                value: const [1000, 5000, 10000, 50000].contains(local.terminalScrollback)
                    ? local.terminalScrollback
                    : 5000,
                items: [
                  for (final n in const [1000, 5000, 10000, 50000])
                    GlassSelectItem(value: n, label: l10n.settingsScrollbackLines(n)),
                ],
                onChanged: (v) => settings.updateLocal(local.copyWith(terminalScrollback: v)),
              ),
            ),
          ),
          Text(
            l10n.settingsTerminalBuffersNote,
            style: GlassTokens.of(context).typography.callout.copyWith(color: GlassTokens.of(context).secondaryLabel),
          ),
        ],
      ),
    );
  }
}

/// Theme, Glass (Clear · Default · Tinted · Solid, LIQUID_GLASS_SPEC §2.2),
/// sidebar style (§2.5) and language — all device-local.
class _AppearanceSection extends ConsumerWidget {
  const _AppearanceSection();

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final local = ref.watch(localSettingsProvider).value ?? const LocalSettings();
    final settings = ref.read(settingsServiceProvider);
    final l10n = context.l10n;
    final tokens = GlassTokens.of(context);
    final strings = GlassStrings.of(context);
    final glass = GlassScope.of(context);
    final solidReason = glass.solidReason;
    final help = tokens.typography.callout.copyWith(color: tokens.secondaryLabel);
    // Why glass renders solid although another option is picked (OS setting,
    // remote session, battery saver, frame guard); nothing when it is the
    // user's own choice.
    final solidNote = switch (solidReason) {
      GlassSolidReason.reduceTransparency => l10n.settingsGlassSolidReduceTransparency,
      GlassSolidReason.remoteSession => l10n.settingsGlassSolidRemote,
      GlassSolidReason.batterySaver => l10n.settingsGlassSolidBattery,
      GlassSolidReason.performance => l10n.settingsGlassSolidPerformance,
      GlassSolidReason.setting || null => null,
    };
    return SectionCard(
      key: const ValueKey('settings-appearance-section'),
      title: l10n.settingsAppearanceTitle,
      icon: Icons.palette_rounded,
      child: Column(
        children: [
          LabeledValue(
            label: l10n.settingsThemeLabel,
            value: Align(
              alignment: AlignmentDirectional.centerStart,
              child: GlassSegmented<AppThemeMode>(
                key: const ValueKey('theme-mode'),
                inChrome: false,
                segments: [
                  GlassSegment(value: AppThemeMode.system, label: l10n.settingsThemeSystem),
                  GlassSegment(value: AppThemeMode.light, label: l10n.settingsThemeLight),
                  GlassSegment(value: AppThemeMode.dark, label: l10n.settingsThemeDark),
                ],
                selected: local.themeMode,
                onChanged: (m) => settings.updateLocal(local.copyWith(themeMode: m)),
              ),
            ),
          ),
          LabeledValue(
            label: strings.glassSetting,
            value: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                GlassSegmented<GlassMode>(
                  key: const ValueKey('glass-mode'),
                  inChrome: false,
                  segments: [
                    for (final m in GlassMode.values)
                      GlassSegment(key: ValueKey('glass-mode-${m.wireName}'), value: m, label: strings.glassMode(m)),
                  ],
                  selected: local.glassMode,
                  onChanged: (m) {
                    // Re-selecting the checked segment is also an explicit
                    // retry after automatic performance reduction.
                    glass.retryEffects?.call();
                    runWithFeedback(context, () => settings.updateLocal(settings.currentLocal.copyWith(glassMode: m)));
                  },
                ),
                const SizedBox(height: GlassSpacing.s4),
                Text(l10n.settingsGlassHelp, style: help),
                if (solidNote != null)
                  Padding(
                    padding: const EdgeInsets.only(top: GlassSpacing.s4),
                    child: Row(
                      children: [
                        Icon(Icons.info_rounded, size: 14, color: tokens.palette.info),
                        const SizedBox(width: GlassSpacing.s4),
                        Expanded(
                          child: Text(
                            solidNote,
                            key: const ValueKey('glass-solid-reason'),
                            style: help.copyWith(color: tokens.palette.info),
                          ),
                        ),
                      ],
                    ),
                  ),
                if (solidReason == GlassSolidReason.performance && glass.retryEffects != null)
                  GlassButton(
                    key: const ValueKey('glass-retry-effects'),
                    label: l10n.settingsGlassRetryEffects,
                    icon: Icons.refresh_rounded,
                    onPressed: glass.retryEffects,
                  ),
              ],
            ),
          ),
          const InterfaceAppearanceControls(),
          LabeledValue(
            label: l10n.settingsSidebarLabel,
            value: Align(
              alignment: AlignmentDirectional.centerStart,
              child: GlassSegmented<SidebarStyle>(
                key: const ValueKey('sidebar-style'),
                inChrome: false,
                segments: [
                  GlassSegment(value: SidebarStyle.floating, label: l10n.settingsSidebarFloating),
                  GlassSegment(value: SidebarStyle.edgeToEdge, label: l10n.settingsSidebarEdgeToEdge),
                ],
                selected: local.sidebarStyle,
                onChanged: (v) => settings.updateLocal(local.copyWith(sidebarStyle: v)),
              ),
            ),
          ),
          LabeledValue(
            label: l10n.settingsWorkspacePanel,
            value: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                GlassSegmented<WorkspacePanelStyle>(
                  key: const ValueKey('workspace-panel-style'),
                  inChrome: false,
                  segments: [
                    GlassSegment(
                      key: const ValueKey('workspace-panel-floating'),
                      value: WorkspacePanelStyle.floating,
                      label: l10n.settingsWorkspaceFloating,
                    ),
                    GlassSegment(
                      key: const ValueKey('workspace-panel-expanded'),
                      value: WorkspacePanelStyle.expanded,
                      label: l10n.settingsWorkspaceExpanded,
                    ),
                  ],
                  selected: local.workspacePanelStyle,
                  onChanged: (value) => runWithFeedback(
                    context,
                    () => settings.updateLocal(settings.currentLocal.copyWith(workspacePanelStyle: value)),
                  ),
                ),
                const SizedBox(height: GlassSpacing.s4),
                Text(
                  local.workspacePanelStyle == WorkspacePanelStyle.floating
                      ? l10n.settingsWorkspaceFloatingHelp
                      : l10n.settingsWorkspaceExpandedHelp,
                  style: help,
                ),
              ],
            ),
          ),
          LabeledValue(
            label: l10n.languageLabel,
            value: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                const LanguageDropdown(),
                const SizedBox(height: GlassSpacing.s4),
                Text(l10n.languageSettingsHelp, style: help),
              ],
            ),
          ),
        ],
      ),
    );
  }
}

class _SecuritySection extends ConsumerWidget {
  const _SecuritySection();

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final local = ref.watch(localSettingsProvider).value ?? const LocalSettings();
    final settings = ref.read(settingsServiceProvider);
    final l10n = context.l10n;
    return SectionCard(
      title: l10n.settingsSecurityTitle,
      icon: Icons.security_rounded,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          LabeledValue(
            label: l10n.settingsClearClipboardLabel,
            value: Align(
              alignment: AlignmentDirectional.centerStart,
              child: GlassSelect<int>(
                key: const ValueKey('clipboard-clear'),
                value: const [15, 30, 60, 120].contains(local.clipboardClearSeconds) ? local.clipboardClearSeconds : 30,
                items: [
                  for (final s in const [15, 30, 60, 120])
                    GlassSelectItem(value: s, label: l10n.settingsClearClipboardAfter(s)),
                ],
                onChanged: (v) => settings.updateLocal(local.copyWith(clipboardClearSeconds: v)),
              ),
            ),
          ),
          const ScreenCaptureSettings(),
        ],
      ),
    );
  }
}

/// Product, version, author and licences; opens the About dialog.
class _AboutSection extends StatelessWidget {
  const _AboutSection();

  @override
  Widget build(BuildContext context) {
    final l10n = context.l10n;
    return SectionCard(
      key: const ValueKey('settings-about'),
      title: l10n.settingsAboutTitle,
      icon: Icons.info_rounded,
      trailing: GlassButton.plain(
        key: const ValueKey('settings-about-open'),
        onPressed: () => showAboutConsoleCrypt(context),
        label: l10n.aboutTitle(kAppName),
      ),
      child: const AboutConsoleCryptContent(compact: true),
    );
  }
}
