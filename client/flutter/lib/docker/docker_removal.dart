enum DockerRemovalKind { volume, network, bind }

enum DockerRemovalStatus { removed, skipped, failed }

final class DockerRemovalResource {
  const DockerRemovalResource({
    required this.kind,
    required this.id,
    required this.name,
    required this.eligible,
    required this.reason,
  });
  final DockerRemovalKind kind;
  final String id, name, reason;
  final bool eligible;

  factory DockerRemovalResource.fromJson(Map<String, dynamic> json) => DockerRemovalResource(
    kind: DockerRemovalKind.values.byName(json['kind'] as String),
    id: json['id'] as String,
    name: json['name'] as String,
    eligible: json['eligible'] as bool,
    reason: json['reason'] as String,
  );
}

final class DockerRemovalPlan {
  const DockerRemovalPlan({
    required this.planId,
    required this.containerId,
    required this.name,
    required this.running,
    required this.resources,
  });
  final String planId, containerId, name;
  final bool running;
  final List<DockerRemovalResource> resources;

  factory DockerRemovalPlan.fromJson(Map<String, dynamic> json) => DockerRemovalPlan(
    planId: json['plan_id'] as String,
    containerId: json['container_id'] as String,
    name: json['name'] as String,
    running: json['running'] as bool,
    resources: [
      for (final resource in (json['resources'] as List).cast<Map<String, dynamic>>())
        DockerRemovalResource.fromJson(resource),
    ],
  );
}

final class DockerRemovalSelection {
  const DockerRemovalSelection({
    required this.planId,
    required this.confirmName,
    this.volumes = const [],
    this.networks = const [],
    this.stopRunning = false,
  });
  final String planId, confirmName;
  final List<String> volumes, networks;
  final bool stopRunning;

  Map<String, dynamic> toJson() => {
    'plan_id': planId,
    'volumes': volumes,
    'networks': networks,
    'stop_running': stopRunning,
    'confirm_name': confirmName,
  };
}

final class DockerRemovalOutcome {
  const DockerRemovalOutcome({
    required this.kind,
    required this.id,
    required this.name,
    required this.status,
    required this.reason,
  });
  final DockerRemovalKind kind;
  final DockerRemovalStatus status;
  final String id, name, reason;

  factory DockerRemovalOutcome.fromJson(Map<String, dynamic> json) => DockerRemovalOutcome(
    kind: DockerRemovalKind.values.byName(json['kind'] as String),
    id: json['id'] as String,
    name: json['name'] as String,
    status: DockerRemovalStatus.values.byName(json['status'] as String),
    reason: json['reason'] as String,
  );
}

final class DockerRemovalResult {
  const DockerRemovalResult({
    required this.containerRemoved,
    required this.containerStopped,
    this.resources = const [],
    this.error,
  });
  final bool containerRemoved, containerStopped;
  final List<DockerRemovalOutcome> resources;
  final String? error;

  factory DockerRemovalResult.fromJson(Map<String, dynamic> json) => DockerRemovalResult(
    containerRemoved: json['container_removed'] as bool,
    containerStopped: json['container_stopped'] as bool,
    resources: [
      for (final resource in (json['resources'] as List).cast<Map<String, dynamic>>())
        DockerRemovalOutcome.fromJson(resource),
    ],
    error: json['error'] as String?,
  );
}
