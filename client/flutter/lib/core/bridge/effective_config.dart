import 'package:consolecrypt/core/models/models.dart';

/// Read-only preview of the Connection Planner's inheritance rules over the
/// decrypted inventory mirror (host editor "effective settings" panel).
///
/// The real plan is always built by app-core (`describe_connection`,
/// terminals, SFTP, tunnels); this only explains *where* a value comes from.
/// Mirrors ssh-core's rules: host value → nearest group with a value →
/// app default; `cc.auth.prompt` stops credential inheritance; an explicit
/// jump chain beats the host's jump profile beats the group's.
// Fallback only: `RustInventoryService.resolveEffective` asks app-core's
// planner (`hosts_plan_preview`, values + provenance + diagnostic codes) and
// uses this resolver while the vault is locked or the core refuses a draft.
// Group defaults (`resolveGroup`) are still computed here.
final class EffectiveConfigResolver {
  const EffectiveConfigResolver({
    required this.hosts,
    required this.groups,
    required this.credentials,
    required this.jumpProfiles,
  });

  final List<Host> hosts;
  final List<Group> groups;
  final List<Credential> credentials;
  final List<JumpProfile> jumpProfiles;

  // Planner diagnostics: the host editor matches these exact texts and shows
  // them localized (`_diagnostic` in hosts/host_editor_screen.dart).
  static const _jumpHostDeleted = 'A jump host in the chain was deleted'; // l10n-ignore: planner diagnostic
  static const _selfJump = 'The host cannot jump through itself'; // l10n-ignore: planner diagnostic
  static const _noCredential =
      'No credential: you will be asked for a password when connecting'; // l10n-ignore: planner diagnostic
  static const _credentialMissing = 'The selected credential no longer exists'; // l10n-ignore: planner diagnostic
  static const _noUsername = 'No username: set one on the host or on a group'; // l10n-ignore: planner diagnostic
  static const _credentialPrompt = 'Password — asked when connecting'; // l10n-ignore: planner diagnostic

  List<Group> _chain(ObjectId? groupId) {
    final chain = <Group>[];
    var cursor = groupId;
    final seen = <ObjectId>{};
    while (cursor != null && seen.add(cursor)) {
      final g = groups.where((x) => x.id == cursor).firstOrNull;
      if (g == null) break;
      chain.add(g);
      cursor = g.parentId;
    }
    return chain;
  }

  (Resolved<int>, Resolved<String>, Resolved<ObjectId>) _basic(Host host) {
    final chain = _chain(host.groupId);
    Resolved<int> port = host.port != null
        ? Resolved(host.port, ValueSource.host)
        : const Resolved(defaultSshPort, ValueSource.appDefault);
    if (host.port == null) {
      for (final g in chain) {
        if (g.inheritedPort != null) {
          port = Resolved(g.inheritedPort, ValueSource.group, sourceName: g.name);
          break;
        }
      }
    }
    Resolved<ObjectId> credential = host.credentialId != null
        ? Resolved(host.credentialId, ValueSource.host)
        : const Resolved.unset();
    if (host.credentialId == null && !host.promptsForPassword) {
      for (final g in chain) {
        if (g.inheritedCredentialId != null) {
          credential = Resolved(g.inheritedCredentialId, ValueSource.group, sourceName: g.name);
          break;
        }
      }
    }
    Resolved<String> username = host.username != null
        ? Resolved(host.username, ValueSource.host)
        : const Resolved.unset();
    if (host.username == null) {
      for (final g in chain) {
        if (g.inheritedUsername != null) {
          username = Resolved(g.inheritedUsername, ValueSource.group, sourceName: g.name);
          break;
        }
      }
    }
    if (username.value == null && credential.value != null) {
      final c = credentials.where((c) => c.id == credential.value).firstOrNull;
      if (c?.username != null) {
        username = Resolved(c!.username, ValueSource.host, sourceName: c.name);
      }
    }
    return (port, username, credential);
  }

  String _label(Host h) {
    final (port, user, _) = _basic(h);
    final p = port.value ?? defaultSshPort;
    return '${h.name} (${user.value == null ? '' : '${user.value}@'}${h.address}${p == defaultSshPort ? '' : ':$p'})';
  }

  EffectiveHostConfig resolve(Host host) {
    final (port, username, resolvedCredential) = _basic(host);
    var credential = resolvedCredential;
    final chain = _chain(host.groupId);
    var hops = const <ObjectId>[];
    Resolved<String> routeSource = const Resolved(null, ValueSource.unset);
    if (host.jumpChain.isNotEmpty) {
      hops = host.jumpChain;
      routeSource = const Resolved(null, ValueSource.host);
    } else if (host.jumpProfileId != null) {
      final profile = jumpProfiles.where((p) => p.id == host.jumpProfileId).firstOrNull;
      if (profile != null) {
        hops = profile.chain;
        routeSource = Resolved(profile.name, ValueSource.jumpProfile, sourceName: profile.name);
      }
    } else {
      for (final g in chain) {
        if (g.inheritedJumpProfileId != null) {
          final profile = jumpProfiles.where((p) => p.id == g.inheritedJumpProfileId).firstOrNull;
          if (profile != null) {
            hops = profile.chain;
            routeSource = Resolved(profile.name, ValueSource.group, sourceName: g.name);
          }
          break;
        }
      }
    }
    final problems = <String>[];
    final route = <RouteHop>[];
    for (final id in hops) {
      final hop = hosts.where((h) => h.id == id).firstOrNull;
      if (hop == null) {
        problems.add(_jumpHostDeleted);
      } else if (hop.id == host.id) {
        problems.add(_selfJump);
      } else {
        route.add(RouteHop(hostId: hop.id, label: _label(hop)));
      }
    }
    final cred = credentials.where((c) => c.id == credential.value).firstOrNull;
    if (host.promptsForPassword) {
      credential = const Resolved(null, ValueSource.host);
    } else if (credential.value == null) {
      problems.add(_noCredential);
    } else if (cred == null) {
      problems.add(_credentialMissing);
    }
    if (username.value == null) problems.add(_noUsername);
    return EffectiveHostConfig(
      port: port,
      username: username,
      credentialId: credential,
      credentialName: host.promptsForPassword ? _credentialPrompt : cred?.name,
      route: route,
      routeSource: routeSource,
      groupPath: [for (final g in chain.reversed) g.name],
      problems: problems,
    );
  }

  EffectiveGroupDefaults resolveGroup(ObjectId groupId) {
    final chain = _chain(groupId);
    Resolved<T> pick<T>(T? Function(Group) get) {
      for (final g in chain) {
        final v = get(g);
        if (v != null) {
          return Resolved(v, g.id == groupId ? ValueSource.host : ValueSource.group, sourceName: g.name);
        }
      }
      return const Resolved.unset();
    }

    final credential = pick((g) => g.inheritedCredentialId);
    final jump = pick((g) => g.inheritedJumpProfileId);
    return EffectiveGroupDefaults(
      username: pick((g) => g.inheritedUsername),
      port: pick((g) => g.inheritedPort),
      credentialId: credential,
      credentialName: credentials.where((c) => c.id == credential.value).firstOrNull?.name,
      jumpProfileId: jump,
      jumpProfileName: jumpProfiles.where((p) => p.id == jump.value).firstOrNull?.name,
    );
  }
}
