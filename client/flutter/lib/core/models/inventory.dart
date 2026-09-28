import 'package:consolecrypt/core/models/credential.dart';
import 'package:consolecrypt/core/models/ids.dart';
import 'package:consolecrypt/core/security/secret_text.dart';

/// Where an effective connection value comes from.
enum ValueSource { host, group, jumpProfile, appDefault, unset }

/// A value resolved by the Connection Planner with its provenance, so the
/// host editor can show "port 2222 — inherited from group Production".
final class Resolved<T> {
  const Resolved(this.value, this.source, {this.sourceName});

  const Resolved.unset() : value = null, source = ValueSource.unset, sourceName = null;

  final T? value;
  final ValueSource source;

  /// Group / profile name for `group` and `jumpProfile` sources.
  final String? sourceName;

  bool get isInherited => source == ValueSource.group || source == ValueSource.jumpProfile;
}

/// One hop in a resolved route.
final class RouteHop {
  const RouteHop({required this.hostId, required this.label});

  final ObjectId hostId;

  /// `name (user@address:port)`.
  final String label;
}

/// Effective settings for a host after group inheritance — a preview of
/// app-core's `ConnectionPlan` (the UI never assembles SSH parameters).
final class EffectiveHostConfig {
  const EffectiveHostConfig({
    required this.port,
    required this.username,
    required this.credentialId,
    required this.credentialName,
    required this.route,
    required this.routeSource,
    required this.groupPath,
    this.problems = const [],
    this.diagnostics = const [],
  });

  final Resolved<int> port;
  final Resolved<String> username;
  final Resolved<ObjectId> credentialId;
  final String? credentialName;

  /// Jump hops, first = closest to the client. Empty = direct.
  final List<RouteHop> route;
  final Resolved<String> routeSource;

  /// Group names from root to the host's group.
  final List<String> groupPath;

  /// e.g. "No credential: connection will ask for a password".
  /// Legacy English texts; prefer [diagnostics].
  final List<String> problems;

  /// Planner findings with stable codes + args (app-core `plan_preview`;
  /// empty from the Dart-only preview). Localize from `code`.
  final List<PlanDiagnostic> diagnostics;
}

/// Severity of a [PlanDiagnostic].
enum PlanDiagnosticSeverity {
  /// No connection can be made until fixed.
  error('error'),

  /// Connects, but probably not as intended.
  warning('warning'),

  /// Explanation (e.g. "password asked when connecting").
  info('info');

  const PlanDiagnosticSeverity(this.wireName);

  final String wireName;

  static PlanDiagnosticSeverity fromWire(String? name) =>
      values.firstWhere((v) => v.wireName == name, orElse: () => warning);
}

/// A connection-planner finding (app-core `PlanDiagnosticDto`).
///
/// Codes — errors: `missing_host`, `jump_host_deleted` (`host_id`),
/// `missing_group` (`group_id`), `group_cycle` (`group_id`),
/// `group_too_deep`, `credential_missing` (`credential_id`),
/// `missing_jump_profile` (`jump_profile_id`), `missing_proxy` (`proxy_id`),
/// `jump_cycle`, `self_jump`, `too_many_hops`, `no_username` (`host_id`,
/// `name`), `invalid_host` (`field`, `rule`); warnings: `no_credential`,
/// `tunnel_public_bind` (`name`, `bind_host`), `tunnel_skipped` (`name`,
/// `rule`); info: `credential_prompt`.
final class PlanDiagnostic {
  const PlanDiagnostic({required this.code, required this.severity, this.args = const {}, this.message = ''});

  final String code;
  final PlanDiagnosticSeverity severity;

  /// Non-secret parameters (ids, names, field/rule).
  final Map<String, String> args;

  /// English diagnostic (never the primary UI text).
  final String message;
}

/// Effective defaults of a group (own values + ancestors').
final class EffectiveGroupDefaults {
  const EffectiveGroupDefaults({
    required this.username,
    required this.port,
    required this.credentialId,
    required this.credentialName,
    required this.jumpProfileId,
    required this.jumpProfileName,
  });

  final Resolved<String> username;
  final Resolved<int> port;
  final Resolved<ObjectId> credentialId;
  final String? credentialName;
  final Resolved<ObjectId> jumpProfileId;
  final String? jumpProfileName;
}

/// How a host authenticates — the full desired state sent with
/// `InventoryService.saveHostWithAuth` (Termius-style inline auth). The core
/// creates / updates / deletes the host's inline Password credential and
/// its Secret atomically; shared credentials are never deleted.
sealed class HostAuth {
  const HostAuth();
}

/// Inherit username/credential from the group chain.
final class HostAuthInherit extends HostAuth {
  const HostAuthInherit();
}

/// Link an existing (possibly shared) credential of any kind.
final class HostAuthCredential extends HostAuth {
  const HostAuthCredential(this.credentialId);

  final ObjectId credentialId;
}

/// Password saved in the vault as this host's inline credential.
/// `password == null` keeps the already saved secret unchanged.
final class HostAuthInlinePassword extends HostAuth {
  const HostAuthInlinePassword({this.password});

  final SecretText? password;

  @override
  String toString() => 'HostAuthInlinePassword(<redacted>)';
}

/// Password asked at connect time; nothing is persisted.
final class HostAuthPasswordPrompt extends HostAuth {
  const HostAuthPasswordPrompt();
}

/// OS SSH agent or an external agent socket / pipe (an agent credential is
/// reused if one with the same settings exists, else created).
final class HostAuthAgent extends HostAuth {
  const HostAuthAgent({required this.kind, this.agentPath});

  /// [CredentialKind.osSshAgent] or [CredentialKind.externalAgent].
  final CredentialKind kind;
  final String? agentPath;
}
