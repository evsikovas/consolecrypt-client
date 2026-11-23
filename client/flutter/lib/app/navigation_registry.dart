import 'package:consolecrypt/app/features.dart';
import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/app/symbols.dart';
import 'package:consolecrypt/app/tool_icons.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

final class NavigationSection {
  const NavigationSection(this.branch, this.icon, {this.syncedOnly = false, this.defaultPinned = true});

  /// Stable storage ID. Keep these names on route/label refactors.
  String get id => branch.name;
  final ShellBranch branch;
  final IconData icon;

  /// Devices apply to synced profiles only (ADR-0106).
  final bool syncedOnly;

  /// New optional workspaces opt in with false, preserving existing defaults.
  final bool defaultPinned;
}

/// Sidebar / toolbar label of a shell branch.
String shellBranchLabel(ShellBranch branch, AppLocalizations l) => switch (branch) {
  ShellBranch.hosts => l.navHosts,
  ShellBranch.groups => l.navGroups,
  ShellBranch.credentials => l.navCredentials,
  ShellBranch.passwords => l.navPasswords,
  ShellBranch.automation => l.automationTitle,
  ShellBranch.knownHosts => l.settingsKnownHostsTitle,
  ShellBranch.terminal => l.navTerminal,
  ShellBranch.sftp => l.navSftp,
  ShellBranch.tunnels => l.navTunnels,
  ShellBranch.devices => l.navDevices,
  ShellBranch.sync => l.navSync,
  ShellBranch.backups => l.navBackups,
  ShellBranch.settings => l.navSettings,
  ShellBranch.sharing => l.sharingTitle,
  ShellBranch.rdp => l.rdpTitle,
  ShellBranch.kubernetes => l.kubeTitle,
  ShellBranch.databases => l.dbTitle,
  ShellBranch.docker => l.dockerTitle,
};

const navigationSections = [
  NavigationSection(ShellBranch.hosts, Icons.dns_rounded),
  NavigationSection(ShellBranch.credentials, Icons.key_rounded),
  NavigationSection(ShellBranch.passwords, Icons.password_rounded),
  NavigationSection(ShellBranch.automation, ToolIcons.automation, defaultPinned: false),
  NavigationSection(ShellBranch.knownHosts, Icons.verified_user_outlined),
  NavigationSection(ShellBranch.sharing, Icons.people_outline_rounded, syncedOnly: true),
  NavigationSection(ShellBranch.terminal, Icons.terminal_rounded),
  NavigationSection(ShellBranch.rdp, ToolIcons.rdp),
  if (kubernetesEnabled) NavigationSection(ShellBranch.kubernetes, ToolIcons.kubernetes),
  if (databasesEnabled) NavigationSection(ShellBranch.databases, Icons.storage_rounded),
  if (dockerEnabled) NavigationSection(ShellBranch.docker, ToolIcons.docker),
  NavigationSection(ShellBranch.sftp, Icons.folder_copy_rounded),
  NavigationSection(ShellBranch.tunnels, Icons.swap_horiz_rounded),
  NavigationSection(ShellBranch.devices, Icons.devices_rounded, syncedOnly: true),
  NavigationSection(ShellBranch.sync, Icons.sync_rounded),
  NavigationSection(ShellBranch.backups, Icons.save_alt_rounded),
  NavigationSection(ShellBranch.settings, Icons.settings_rounded),
];

enum NavigationArea { access, workspaces, profile }

NavigationArea navigationArea(ShellBranch branch) => switch (branch) {
  ShellBranch.hosts ||
  ShellBranch.groups ||
  ShellBranch.credentials ||
  ShellBranch.passwords ||
  ShellBranch.automation ||
  ShellBranch.knownHosts ||
  ShellBranch.sharing => NavigationArea.access,
  ShellBranch.sync || ShellBranch.backups || ShellBranch.settings => NavigationArea.profile,
  _ => NavigationArea.workspaces,
};

String navigationAreaLabel(NavigationArea area, AppLocalizations l) => switch (area) {
  NavigationArea.access => l.navAreaAccess,
  NavigationArea.workspaces => l.navAreaWorkspaces,
  NavigationArea.profile => l.navAreaProfile,
};

/// Original application glyphs, shared by sidebar and configuration.
Widget navigationSectionIcon(NavigationSection item) =>
    {ShellBranch.rdp, ShellBranch.docker, ShellBranch.kubernetes, ShellBranch.automation}.contains(item.branch)
    ? Icon(item.icon)
    : AppSymbolIcon(AppSymbol.values.byName(item.branch.name));

String navigationBranchRoute(ShellBranch branch) => switch (branch) {
  ShellBranch.hosts => AppRoutes.hosts,
  ShellBranch.groups => AppRoutes.groups,
  ShellBranch.credentials => AppRoutes.credentials,
  ShellBranch.passwords => AppRoutes.passwords,
  ShellBranch.automation => AppRoutes.automation,
  ShellBranch.knownHosts => AppRoutes.knownHosts,
  ShellBranch.sharing => AppRoutes.sharing,
  ShellBranch.terminal => AppRoutes.terminal,
  ShellBranch.rdp => AppRoutes.rdp,
  ShellBranch.kubernetes => AppRoutes.kubernetes,
  ShellBranch.databases => AppRoutes.databases,
  ShellBranch.docker => AppRoutes.docker,
  ShellBranch.sftp => AppRoutes.sftp,
  ShellBranch.tunnels => AppRoutes.tunnels,
  ShellBranch.devices => AppRoutes.devices,
  ShellBranch.sync => AppRoutes.sync,
  ShellBranch.backups => AppRoutes.backups,
  ShellBranch.settings => AppRoutes.settings,
};

final availableNavigationSectionsProvider = Provider<List<NavigationSection>>((ref) => navigationSections);
