// Adapted from WSLc Desk; JSON DTOs only, no separate FRB runtime.
import 'dart:typed_data';

abstract interface class WslcJsonModel {
  Map<String, dynamic> toJson();
}

dynamic wslcEncode(dynamic value) => switch (value) {
  final WslcJsonModel value => value.toJson(),
  final ContainerState value => value.name[0].toUpperCase() + value.name.substring(1),
  final BigInt value => value <= BigInt.from(0x7fffffffffffffff) ? value.toInt() : {'\$u64': value.toString()},
  final List<dynamic> value => value.map(wslcEncode).toList(),
  final Map<dynamic, dynamic> value => value.map((key, item) => MapEntry(key.toString(), wslcEncode(item))),
  _ => value,
};

class AppError implements Exception, WslcJsonModel {
  factory AppError.fromJson(Map<String, dynamic> json) => AppError(
    code: json['code'] as String,
    message: json['message'] as String,
    operation: json['operation'] as String,
    diagnostics: json['diagnostics'] == null ? null : json['diagnostics'] as String,
  );
  @override
  Map<String, dynamic> toJson() => {
    'code': wslcEncode(code),
    'message': wslcEncode(message),
    'operation': wslcEncode(operation),
    'diagnostics': wslcEncode(diagnostics),
  };

  final String code;
  final String message;
  final String operation;
  final String? diagnostics;

  const AppError({required this.code, required this.message, required this.operation, this.diagnostics});

  @override
  int get hashCode => code.hashCode ^ message.hashCode ^ operation.hashCode ^ diagnostics.hashCode;

  @override
  bool operator ==(Object other) =>
      identical(this, other) ||
      other is AppError &&
          runtimeType == other.runtimeType &&
          code == other.code &&
          message == other.message &&
          operation == other.operation &&
          diagnostics == other.diagnostics;
}

/// `wslc build` options from the build dialog.
class BuildSpec implements WslcJsonModel {
  factory BuildSpec.fromJson(Map<String, dynamic> json) => BuildSpec(
    context: json['context'] as String,
    dockerfile: json['dockerfile'] == null ? null : json['dockerfile'] as String,
    tags: (json['tags'] as List).map((item) => item as String).toList(),
    buildArgs: (json['build_args'] as List)
        .map((item) => EnvVar.fromJson((item as Map).cast<String, dynamic>()))
        .toList(),
    target: json['target'] == null ? null : json['target'] as String,
    noCache: json['no_cache'] as bool,
    pull: json['pull'] as bool,
  );
  @override
  Map<String, dynamic> toJson() => {
    'context': wslcEncode(context),
    'dockerfile': wslcEncode(dockerfile),
    'tags': wslcEncode(tags),
    'build_args': wslcEncode(buildArgs),
    'target': wslcEncode(target),
    'no_cache': wslcEncode(noCache),
    'pull': wslcEncode(pull),
  };

  /// Existing Windows folder used as the build context.
  final String context;

  /// Dockerfile path; the context's Dockerfile when absent.
  final String? dockerfile;
  final List<String> tags;
  final List<EnvVar> buildArgs;
  final String? target;
  final bool noCache;
  final bool pull;

  const BuildSpec({
    required this.context,
    this.dockerfile,
    required this.tags,
    required this.buildArgs,
    this.target,
    required this.noCache,
    required this.pull,
  });

  @override
  int get hashCode =>
      context.hashCode ^
      dockerfile.hashCode ^
      tags.hashCode ^
      buildArgs.hashCode ^
      target.hashCode ^
      noCache.hashCode ^
      pull.hashCode;

  @override
  bool operator ==(Object other) =>
      identical(this, other) ||
      other is BuildSpec &&
          runtimeType == other.runtimeType &&
          context == other.context &&
          dockerfile == other.dockerfile &&
          tags == other.tags &&
          buildArgs == other.buildArgs &&
          target == other.target &&
          noCache == other.noCache &&
          pull == other.pull;
}

class ComposeAttach implements WslcJsonModel {
  factory ComposeAttach.fromJson(Map<String, dynamic> json) => ComposeAttach(
    network: json['network'] as String,
    aliases: (json['aliases'] as List).map((item) => item as String).toList(),
    ip: json['ip'] == null ? null : json['ip'] as String,
  );
  @override
  Map<String, dynamic> toJson() => {
    'network': wslcEncode(network),
    'aliases': wslcEncode(aliases),
    'ip': wslcEncode(ip),
  };

  final String network;
  final List<String> aliases;
  final String? ip;

  const ComposeAttach({required this.network, required this.aliases, this.ip});

  @override
  int get hashCode => network.hashCode ^ aliases.hashCode ^ ip.hashCode;

  @override
  bool operator ==(Object other) =>
      identical(this, other) ||
      other is ComposeAttach &&
          runtimeType == other.runtimeType &&
          network == other.network &&
          aliases == other.aliases &&
          ip == other.ip;
}

/// A container that belongs to a Compose project, read from its labels.
class ComposeContainer implements WslcJsonModel {
  factory ComposeContainer.fromJson(Map<String, dynamic> json) => ComposeContainer(
    id: json['id'] as String,
    name: json['name'] as String,
    service: json['service'] == null ? null : json['service'] as String,
    state: ContainerState.values.firstWhere(
      (e) => e.name == (json['state'] as String).toLowerCase(),
      orElse: () => ContainerState.unknown,
    ),
    configHash: json['config_hash'] == null ? null : json['config_hash'] as String,
    health: json['health'] == null ? null : json['health'] as String,
    exitCode: json['exit_code'] == null ? null : (json['exit_code'] as num).toInt(),
  );
  @override
  Map<String, dynamic> toJson() => {
    'id': wslcEncode(id),
    'name': wslcEncode(name),
    'service': wslcEncode(service),
    'state': wslcEncode(state),
    'config_hash': wslcEncode(configHash),
    'health': wslcEncode(health),
    'exit_code': wslcEncode(exitCode),
  };

  final String id;
  final String name;
  final String? service;
  final ContainerState state;
  final String? configHash;

  /// `starting`, `healthy`, `unhealthy` when the container has a health check.
  final String? health;
  final int? exitCode;

  const ComposeContainer({
    required this.id,
    required this.name,
    this.service,
    required this.state,
    this.configHash,
    this.health,
    this.exitCode,
  });

  @override
  int get hashCode =>
      id.hashCode ^
      name.hashCode ^
      service.hashCode ^
      state.hashCode ^
      configHash.hashCode ^
      health.hashCode ^
      exitCode.hashCode;

  @override
  bool operator ==(Object other) =>
      identical(this, other) ||
      other is ComposeContainer &&
          runtimeType == other.runtimeType &&
          id == other.id &&
          name == other.name &&
          service == other.service &&
          state == other.state &&
          configHash == other.configHash &&
          health == other.health &&
          exitCode == other.exitCode;
}

class ComposeDependency implements WslcJsonModel {
  factory ComposeDependency.fromJson(Map<String, dynamic> json) =>
      ComposeDependency(service: json['service'] as String, condition: json['condition'] as String);
  @override
  Map<String, dynamic> toJson() => {'service': wslcEncode(service), 'condition': wslcEncode(condition)};

  final String service;

  /// `started`, `healthy` or `completed`.
  final String condition;

  const ComposeDependency({required this.service, required this.condition});

  @override
  int get hashCode => service.hashCode ^ condition.hashCode;

  @override
  bool operator ==(Object other) =>
      identical(this, other) ||
      other is ComposeDependency &&
          runtimeType == other.runtimeType &&
          service == other.service &&
          condition == other.condition;
}

class ComposeNetwork implements WslcJsonModel {
  factory ComposeNetwork.fromJson(Map<String, dynamic> json) => ComposeNetwork(
    key: json['key'] as String,
    isExternal: json['is_external'] as bool,
    spec: NetworkSpec.fromJson((json['spec'] as Map).cast<String, dynamic>()),
  );
  @override
  Map<String, dynamic> toJson() => {
    'key': wslcEncode(key),
    'is_external': wslcEncode(isExternal),
    'spec': wslcEncode(spec),
  };

  /// Key in the file (`default`, `backend`…).
  final String key;
  final bool isExternal;
  final NetworkSpec spec;

  const ComposeNetwork({required this.key, required this.isExternal, required this.spec});

  @override
  int get hashCode => key.hashCode ^ isExternal.hashCode ^ spec.hashCode;

  @override
  bool operator ==(Object other) =>
      identical(this, other) ||
      other is ComposeNetwork &&
          runtimeType == other.runtimeType &&
          key == other.key &&
          isExternal == other.isExternal &&
          spec == other.spec;
}

/// What `docker compose up` would do, translated to WSLc steps.
class ComposePlan implements WslcJsonModel {
  factory ComposePlan.fromJson(Map<String, dynamic> json) => ComposePlan(
    project: json['project'] as String,
    workingDir: json['working_dir'] == null ? null : json['working_dir'] as String,
    networks: (json['networks'] as List)
        .map((item) => ComposeNetwork.fromJson((item as Map).cast<String, dynamic>()))
        .toList(),
    volumes: (json['volumes'] as List)
        .map((item) => ComposeVolume.fromJson((item as Map).cast<String, dynamic>()))
        .toList(),
    services: (json['services'] as List)
        .map((item) => ComposeService.fromJson((item as Map).cast<String, dynamic>()))
        .toList(),
    hostFolders: (json['host_folders'] as List).map((item) => item as String).toList(),
    commands: (json['commands'] as List).map((item) => item as String).toList(),
    warnings: (json['warnings'] as List).map((item) => item as String).toList(),
  );
  @override
  Map<String, dynamic> toJson() => {
    'project': wslcEncode(project),
    'working_dir': wslcEncode(workingDir),
    'networks': wslcEncode(networks),
    'volumes': wslcEncode(volumes),
    'services': wslcEncode(services),
    'host_folders': wslcEncode(hostFolders),
    'commands': wslcEncode(commands),
    'warnings': wslcEncode(warnings),
  };

  final String project;

  /// Folder relative paths, `.env` and build contexts resolve against.
  final String? workingDir;
  final List<ComposeNetwork> networks;
  final List<ComposeVolume> volumes;

  /// In start order: dependencies first.
  final List<ComposeService> services;

  /// Bind-mount folders created when missing, as Compose does.
  final List<String> hostFolders;

  /// Network and volume commands, for the preview.
  final List<String> commands;

  /// Things WSLc cannot do or that work differently; shown before anything runs.
  final List<String> warnings;

  const ComposePlan({
    required this.project,
    this.workingDir,
    required this.networks,
    required this.volumes,
    required this.services,
    required this.hostFolders,
    required this.commands,
    required this.warnings,
  });

  @override
  int get hashCode =>
      project.hashCode ^
      workingDir.hashCode ^
      networks.hashCode ^
      volumes.hashCode ^
      services.hashCode ^
      hostFolders.hashCode ^
      commands.hashCode ^
      warnings.hashCode;

  @override
  bool operator ==(Object other) =>
      identical(this, other) ||
      other is ComposePlan &&
          runtimeType == other.runtimeType &&
          project == other.project &&
          workingDir == other.workingDir &&
          networks == other.networks &&
          volumes == other.volumes &&
          services == other.services &&
          hostFolders == other.hostFolders &&
          commands == other.commands &&
          warnings == other.warnings;
}

class ComposeService implements WslcJsonModel {
  factory ComposeService.fromJson(Map<String, dynamic> json) => ComposeService(
    service: json['service'] as String,
    containerName: json['container_name'] as String,
    image: json['image'] as String,
    build: json['build'] == null ? null : BuildSpec.fromJson((json['build'] as Map).cast<String, dynamic>()),
    pullPolicy: json['pull_policy'] as String,
    spec: RunSpec.fromJson((json['spec'] as Map).cast<String, dynamic>()),
    extraNetworks: (json['extra_networks'] as List)
        .map((item) => ComposeAttach.fromJson((item as Map).cast<String, dynamic>()))
        .toList(),
    dependsOn: (json['depends_on'] as List)
        .map((item) => ComposeDependency.fromJson((item as Map).cast<String, dynamic>()))
        .toList(),
    configHash: json['config_hash'] as String,
    commands: (json['commands'] as List).map((item) => item as String).toList(),
  );
  @override
  Map<String, dynamic> toJson() => {
    'service': wslcEncode(service),
    'container_name': wslcEncode(containerName),
    'image': wslcEncode(image),
    'build': wslcEncode(build),
    'pull_policy': wslcEncode(pullPolicy),
    'spec': wslcEncode(spec),
    'extra_networks': wslcEncode(extraNetworks),
    'depends_on': wslcEncode(dependsOn),
    'config_hash': wslcEncode(configHash),
    'commands': wslcEncode(commands),
  };

  final String service;
  final String containerName;
  final String image;

  /// Built before the container is created; the image name is its tag.
  final BuildSpec? build;

  /// `missing`, `always`, `never` or `build`.
  final String pullPolicy;

  /// Always `create`; the runner starts it once its networks are attached.
  final RunSpec spec;

  /// Networks after the first, connected before start.
  final List<ComposeAttach> extraNetworks;
  final List<ComposeDependency> dependsOn;

  /// Same hash as last time: the existing container is kept.
  final String configHash;

  /// The commands this service turns into, for the preview.
  final List<String> commands;

  const ComposeService({
    required this.service,
    required this.containerName,
    required this.image,
    this.build,
    required this.pullPolicy,
    required this.spec,
    required this.extraNetworks,
    required this.dependsOn,
    required this.configHash,
    required this.commands,
  });

  @override
  int get hashCode =>
      service.hashCode ^
      containerName.hashCode ^
      image.hashCode ^
      build.hashCode ^
      pullPolicy.hashCode ^
      spec.hashCode ^
      extraNetworks.hashCode ^
      dependsOn.hashCode ^
      configHash.hashCode ^
      commands.hashCode;

  @override
  bool operator ==(Object other) =>
      identical(this, other) ||
      other is ComposeService &&
          runtimeType == other.runtimeType &&
          service == other.service &&
          containerName == other.containerName &&
          image == other.image &&
          build == other.build &&
          pullPolicy == other.pullPolicy &&
          spec == other.spec &&
          extraNetworks == other.extraNetworks &&
          dependsOn == other.dependsOn &&
          configHash == other.configHash &&
          commands == other.commands;
}

class ComposeVolume implements WslcJsonModel {
  factory ComposeVolume.fromJson(Map<String, dynamic> json) => ComposeVolume(
    key: json['key'] as String,
    isExternal: json['is_external'] as bool,
    spec: VolumeSpec.fromJson((json['spec'] as Map).cast<String, dynamic>()),
  );
  @override
  Map<String, dynamic> toJson() => {
    'key': wslcEncode(key),
    'is_external': wslcEncode(isExternal),
    'spec': wslcEncode(spec),
  };

  final String key;
  final bool isExternal;
  final VolumeSpec spec;

  const ComposeVolume({required this.key, required this.isExternal, required this.spec});

  @override
  int get hashCode => key.hashCode ^ isExternal.hashCode ^ spec.hashCode;

  @override
  bool operator ==(Object other) =>
      identical(this, other) ||
      other is ComposeVolume &&
          runtimeType == other.runtimeType &&
          key == other.key &&
          isExternal == other.isExternal &&
          spec == other.spec;
}

class ContainerDetails implements WslcJsonModel {
  factory ContainerDetails.fromJson(Map<String, dynamic> json) => ContainerDetails(
    id: json['id'] as String,
    name: json['name'] == null ? null : json['name'] as String,
    imageId: json['image_id'] == null ? null : json['image_id'] as String,
    imageName: json['image_name'] == null ? null : json['image_name'] as String,
    state: ContainerState.values.firstWhere(
      (e) => e.name == (json['state'] as String).toLowerCase(),
      orElse: () => ContainerState.unknown,
    ),
    rawState: json['raw_state'] == null ? null : json['raw_state'] as String,
    autoRemove: json['auto_remove'] == null ? null : json['auto_remove'] as bool,
    stopTimeoutSeconds: json['stop_timeout_seconds'] == null
        ? null
        : BigInt.parse(json['stop_timeout_seconds'].toString()),
    exitCode: json['exit_code'] == null ? null : (json['exit_code'] as num).toInt(),
    createdAt: json['created_at'] == null ? null : json['created_at'] as String,
    startedAt: json['started_at'] == null ? null : json['started_at'] as String,
    finishedAt: json['finished_at'] == null ? null : json['finished_at'] as String,
    healthStatus: json['health_status'] == null ? null : json['health_status'] as String,
    command: json['command'] == null ? null : json['command'] as String,
    mounts: json['mounts'] == null
        ? null
        : (json['mounts'] as List).map((item) => MountInfo.fromJson((item as Map).cast<String, dynamic>())).toList(),
    ports: (json['ports'] as List).map((item) => PortBinding.fromJson((item as Map).cast<String, dynamic>())).toList(),
    networks: (json['networks'] as List)
        .map((item) => NetworkAttachment.fromJson((item as Map).cast<String, dynamic>()))
        .toList(),
    inspectJson: json['inspect_json'] as String,
  );
  @override
  Map<String, dynamic> toJson() => {
    'id': wslcEncode(id),
    'name': wslcEncode(name),
    'image_id': wslcEncode(imageId),
    'image_name': wslcEncode(imageName),
    'state': wslcEncode(state),
    'raw_state': wslcEncode(rawState),
    'auto_remove': wslcEncode(autoRemove),
    'stop_timeout_seconds': wslcEncode(stopTimeoutSeconds),
    'exit_code': wslcEncode(exitCode),
    'created_at': wslcEncode(createdAt),
    'started_at': wslcEncode(startedAt),
    'finished_at': wslcEncode(finishedAt),
    'health_status': wslcEncode(healthStatus),
    'command': wslcEncode(command),
    'mounts': wslcEncode(mounts),
    'ports': wslcEncode(ports),
    'networks': wslcEncode(networks),
    'inspect_json': wslcEncode(inspectJson),
  };

  final String id;
  final String? name;
  final String? imageId;
  final String? imageName;
  final ContainerState state;
  final String? rawState;
  final bool? autoRemove;
  final BigInt? stopTimeoutSeconds;
  final int? exitCode;
  final String? createdAt;
  final String? startedAt;
  final String? finishedAt;
  final String? healthStatus;
  final String? command;
  final List<MountInfo>? mounts;
  final List<PortBinding> ports;
  final List<NetworkAttachment> networks;
  final String inspectJson;

  const ContainerDetails({
    required this.id,
    this.name,
    this.imageId,
    this.imageName,
    required this.state,
    this.rawState,
    this.autoRemove,
    this.stopTimeoutSeconds,
    this.exitCode,
    this.createdAt,
    this.startedAt,
    this.finishedAt,
    this.healthStatus,
    this.command,
    this.mounts,
    required this.ports,
    required this.networks,
    required this.inspectJson,
  });

  @override
  int get hashCode =>
      id.hashCode ^
      name.hashCode ^
      imageId.hashCode ^
      imageName.hashCode ^
      state.hashCode ^
      rawState.hashCode ^
      autoRemove.hashCode ^
      stopTimeoutSeconds.hashCode ^
      exitCode.hashCode ^
      createdAt.hashCode ^
      startedAt.hashCode ^
      finishedAt.hashCode ^
      healthStatus.hashCode ^
      command.hashCode ^
      mounts.hashCode ^
      ports.hashCode ^
      networks.hashCode ^
      inspectJson.hashCode;

  @override
  bool operator ==(Object other) =>
      identical(this, other) ||
      other is ContainerDetails &&
          runtimeType == other.runtimeType &&
          id == other.id &&
          name == other.name &&
          imageId == other.imageId &&
          imageName == other.imageName &&
          state == other.state &&
          rawState == other.rawState &&
          autoRemove == other.autoRemove &&
          stopTimeoutSeconds == other.stopTimeoutSeconds &&
          exitCode == other.exitCode &&
          createdAt == other.createdAt &&
          startedAt == other.startedAt &&
          finishedAt == other.finishedAt &&
          healthStatus == other.healthStatus &&
          command == other.command &&
          mounts == other.mounts &&
          ports == other.ports &&
          networks == other.networks &&
          inspectJson == other.inspectJson;
}

/// What one container uses, for the relations map.
class ContainerLinks implements WslcJsonModel {
  factory ContainerLinks.fromJson(Map<String, dynamic> json) => ContainerLinks(
    id: json['id'] as String,
    name: json['name'] == null ? null : json['name'] as String,
    state: ContainerState.values.firstWhere(
      (e) => e.name == (json['state'] as String).toLowerCase(),
      orElse: () => ContainerState.unknown,
    ),
    imageId: json['image_id'] == null ? null : json['image_id'] as String,
    imageName: json['image_name'] == null ? null : json['image_name'] as String,
    networks: (json['networks'] as List).map((item) => item as String).toList(),
    volumes: (json['volumes'] as List).map((item) => item as String).toList(),
    binds: (json['binds'] as List).map((item) => item as String).toList(),
  );
  @override
  Map<String, dynamic> toJson() => {
    'id': wslcEncode(id),
    'name': wslcEncode(name),
    'state': wslcEncode(state),
    'image_id': wslcEncode(imageId),
    'image_name': wslcEncode(imageName),
    'networks': wslcEncode(networks),
    'volumes': wslcEncode(volumes),
    'binds': wslcEncode(binds),
  };

  final String id;
  final String? name;
  final ContainerState state;
  final String? imageId;
  final String? imageName;
  final List<String> networks;
  final List<String> volumes;

  /// Windows folders bind-mounted into the container.
  final List<String> binds;

  const ContainerLinks({
    required this.id,
    this.name,
    required this.state,
    this.imageId,
    this.imageName,
    required this.networks,
    required this.volumes,
    required this.binds,
  });

  @override
  int get hashCode =>
      id.hashCode ^
      name.hashCode ^
      state.hashCode ^
      imageId.hashCode ^
      imageName.hashCode ^
      networks.hashCode ^
      volumes.hashCode ^
      binds.hashCode;

  @override
  bool operator ==(Object other) =>
      identical(this, other) ||
      other is ContainerLinks &&
          runtimeType == other.runtimeType &&
          id == other.id &&
          name == other.name &&
          state == other.state &&
          imageId == other.imageId &&
          imageName == other.imageName &&
          networks == other.networks &&
          volumes == other.volumes &&
          binds == other.binds;
}

enum ContainerState { created, running, exited, unknown }

/// One `wslc stats` snapshot row. Values are display strings from the runtime.
class ContainerStats implements WslcJsonModel {
  factory ContainerStats.fromJson(Map<String, dynamic> json) => ContainerStats(
    id: json['id'] as String,
    cpuPercent: json['cpu_percent'] == null ? null : (json['cpu_percent'] as num).toDouble(),
    memoryPercent: json['memory_percent'] == null ? null : (json['memory_percent'] as num).toDouble(),
    memoryUsage: json['memory_usage'] == null ? null : json['memory_usage'] as String,
    netIo: json['net_io'] == null ? null : json['net_io'] as String,
    blockIo: json['block_io'] == null ? null : json['block_io'] as String,
    pids: json['pids'] == null ? null : BigInt.parse(json['pids'].toString()),
  );
  @override
  Map<String, dynamic> toJson() => {
    'id': wslcEncode(id),
    'cpu_percent': wslcEncode(cpuPercent),
    'memory_percent': wslcEncode(memoryPercent),
    'memory_usage': wslcEncode(memoryUsage),
    'net_io': wslcEncode(netIo),
    'block_io': wslcEncode(blockIo),
    'pids': wslcEncode(pids),
  };

  final String id;
  final double? cpuPercent;
  final double? memoryPercent;
  final String? memoryUsage;
  final String? netIo;
  final String? blockIo;
  final BigInt? pids;

  const ContainerStats({
    required this.id,
    this.cpuPercent,
    this.memoryPercent,
    this.memoryUsage,
    this.netIo,
    this.blockIo,
    this.pids,
  });

  @override
  int get hashCode =>
      id.hashCode ^
      cpuPercent.hashCode ^
      memoryPercent.hashCode ^
      memoryUsage.hashCode ^
      netIo.hashCode ^
      blockIo.hashCode ^
      pids.hashCode;

  @override
  bool operator ==(Object other) =>
      identical(this, other) ||
      other is ContainerStats &&
          runtimeType == other.runtimeType &&
          id == other.id &&
          cpuPercent == other.cpuPercent &&
          memoryPercent == other.memoryPercent &&
          memoryUsage == other.memoryUsage &&
          netIo == other.netIo &&
          blockIo == other.blockIo &&
          pids == other.pids;
}

class ContainerSummary implements WslcJsonModel {
  factory ContainerSummary.fromJson(Map<String, dynamic> json) => ContainerSummary(
    id: json['id'] as String,
    name: json['name'] == null ? null : json['name'] as String,
    image: json['image'] == null ? null : json['image'] as String,
    state: ContainerState.values.firstWhere(
      (e) => e.name == (json['state'] as String).toLowerCase(),
      orElse: () => ContainerState.unknown,
    ),
    rawState: json['raw_state'] == null ? null : json['raw_state'] as String,
    portsDisplay: json['ports_display'] == null ? null : json['ports_display'] as String,
    createdAt: json['created_at'] == null ? null : json['created_at'] as String,
    healthStatus: json['health_status'] == null ? null : json['health_status'] as String,
    statusDisplay: json['status_display'] == null ? null : json['status_display'] as String,
  );
  @override
  Map<String, dynamic> toJson() => {
    'id': wslcEncode(id),
    'name': wslcEncode(name),
    'image': wslcEncode(image),
    'state': wslcEncode(state),
    'raw_state': wslcEncode(rawState),
    'ports_display': wslcEncode(portsDisplay),
    'created_at': wslcEncode(createdAt),
    'health_status': wslcEncode(healthStatus),
    'status_display': wslcEncode(statusDisplay),
  };

  final String id;
  final String? name;
  final String? image;
  final ContainerState state;
  final String? rawState;
  final String? portsDisplay;
  final String? createdAt;
  final String? healthStatus;
  final String? statusDisplay;

  const ContainerSummary({
    required this.id,
    this.name,
    this.image,
    required this.state,
    this.rawState,
    this.portsDisplay,
    this.createdAt,
    this.healthStatus,
    this.statusDisplay,
  });

  @override
  int get hashCode =>
      id.hashCode ^
      name.hashCode ^
      image.hashCode ^
      state.hashCode ^
      rawState.hashCode ^
      portsDisplay.hashCode ^
      createdAt.hashCode ^
      healthStatus.hashCode ^
      statusDisplay.hashCode;

  @override
  bool operator ==(Object other) =>
      identical(this, other) ||
      other is ContainerSummary &&
          runtimeType == other.runtimeType &&
          id == other.id &&
          name == other.name &&
          image == other.image &&
          state == other.state &&
          rawState == other.rawState &&
          portsDisplay == other.portsDisplay &&
          createdAt == other.createdAt &&
          healthStatus == other.healthStatus &&
          statusDisplay == other.statusDisplay;
}

/// Wizard settings saved under a name for the next container.
class ContainerTemplate implements WslcJsonModel {
  factory ContainerTemplate.fromJson(Map<String, dynamic> json) => ContainerTemplate(
    name: json['name'] as String,
    spec: RunSpec.fromJson((json['spec'] as Map).cast<String, dynamic>()),
  );
  @override
  Map<String, dynamic> toJson() => {'name': wslcEncode(name), 'spec': wslcEncode(spec)};

  final String name;
  final RunSpec spec;

  const ContainerTemplate({required this.name, required this.spec});

  @override
  int get hashCode => name.hashCode ^ spec.hashCode;

  @override
  bool operator ==(Object other) =>
      identical(this, other) ||
      other is ContainerTemplate && runtimeType == other.runtimeType && name == other.name && spec == other.spec;
}

/// A runtime event: `kind` is container/image/volume/network, `action` e.g. start or die.
class DeskEvent implements WslcJsonModel {
  factory DeskEvent.fromJson(Map<String, dynamic> json) => DeskEvent(
    kind: json['kind'] as String,
    action: json['action'] as String,
    id: json['id'] as String,
    name: json['name'] == null ? null : json['name'] as String,
    image: json['image'] == null ? null : json['image'] as String,
    exitCode: json['exit_code'] == null ? null : (json['exit_code'] as num).toInt(),
    time: (json['time'] as num).toInt(),
  );
  @override
  Map<String, dynamic> toJson() => {
    'kind': wslcEncode(kind),
    'action': wslcEncode(action),
    'id': wslcEncode(id),
    'name': wslcEncode(name),
    'image': wslcEncode(image),
    'exit_code': wslcEncode(exitCode),
    'time': wslcEncode(time),
  };

  final String kind;
  final String action;
  final String id;
  final String? name;
  final String? image;
  final int? exitCode;
  final int time;

  const DeskEvent({
    required this.kind,
    required this.action,
    required this.id,
    this.name,
    this.image,
    this.exitCode,
    required this.time,
  });

  @override
  int get hashCode =>
      kind.hashCode ^
      action.hashCode ^
      id.hashCode ^
      name.hashCode ^
      image.hashCode ^
      exitCode.hashCode ^
      time.hashCode;

  @override
  bool operator ==(Object other) =>
      identical(this, other) ||
      other is DeskEvent &&
          runtimeType == other.runtimeType &&
          kind == other.kind &&
          action == other.action &&
          id == other.id &&
          name == other.name &&
          image == other.image &&
          exitCode == other.exitCode &&
          time == other.time;
}

/// Saved preferences. Every field may be absent, so older and newer files keep loading.
class DeskSettings implements WslcJsonModel {
  factory DeskSettings.fromJson(Map<String, dynamic> json) => DeskSettings(
    theme: json['theme'] == null ? null : json['theme'] as String,
    accentColor: json['accent_color'] == null ? null : json['accent_color'] as String,
    backgroundTint: json['background_tint'] == null ? null : json['background_tint'] as String,
    refreshSeconds: json['refresh_seconds'] == null ? null : (json['refresh_seconds'] as num).toInt(),
    notifications: json['notifications'] == null ? null : json['notifications'] as bool,
    closeToTray: json['close_to_tray'] == null ? null : json['close_to_tray'] as bool,
    cliPath: json['cli_path'] == null ? null : json['cli_path'] as String,
    templates: (json['templates'] as List)
        .map((item) => ContainerTemplate.fromJson((item as Map).cast<String, dynamic>()))
        .toList(),
  );
  @override
  Map<String, dynamic> toJson() => {
    'theme': wslcEncode(theme),
    'accent_color': wslcEncode(accentColor),
    'background_tint': wslcEncode(backgroundTint),
    'refresh_seconds': wslcEncode(refreshSeconds),
    'notifications': wslcEncode(notifications),
    'close_to_tray': wslcEncode(closeToTray),
    'cli_path': wslcEncode(cliPath),
    'templates': wslcEncode(templates),
  };

  /// `light`, `dark` or `system`.
  final String? theme;

  /// Optional opaque sRGB colors, written as `#RRGGBB`; absent keeps the built-in palette.
  final String? accentColor;
  final String? backgroundTint;
  final int? refreshSeconds;
  final bool? notifications;
  final bool? closeToTray;

  /// A `wslc.exe` outside the default location.
  final String? cliPath;
  final List<ContainerTemplate> templates;

  const DeskSettings({
    this.theme,
    this.accentColor,
    this.backgroundTint,
    this.refreshSeconds,
    this.notifications,
    this.closeToTray,
    this.cliPath,
    required this.templates,
  });

  @override
  int get hashCode =>
      theme.hashCode ^
      accentColor.hashCode ^
      backgroundTint.hashCode ^
      refreshSeconds.hashCode ^
      notifications.hashCode ^
      closeToTray.hashCode ^
      cliPath.hashCode ^
      templates.hashCode;

  @override
  bool operator ==(Object other) =>
      identical(this, other) ||
      other is DeskSettings &&
          runtimeType == other.runtimeType &&
          theme == other.theme &&
          accentColor == other.accentColor &&
          backgroundTint == other.backgroundTint &&
          refreshSeconds == other.refreshSeconds &&
          notifications == other.notifications &&
          closeToTray == other.closeToTray &&
          cliPath == other.cliPath &&
          templates == other.templates;
}

/// One variable of a container; `from_image` marks values inherited unchanged from the image.
class EnvEntry implements WslcJsonModel {
  factory EnvEntry.fromJson(Map<String, dynamic> json) =>
      EnvEntry(key: json['key'] as String, value: json['value'] as String, fromImage: json['from_image'] as bool);
  @override
  Map<String, dynamic> toJson() => {
    'key': wslcEncode(key),
    'value': wslcEncode(value),
    'from_image': wslcEncode(fromImage),
  };

  final String key;
  final String value;
  final bool fromImage;

  const EnvEntry({required this.key, required this.value, required this.fromImage});

  @override
  int get hashCode => key.hashCode ^ value.hashCode ^ fromImage.hashCode;

  @override
  bool operator ==(Object other) =>
      identical(this, other) ||
      other is EnvEntry &&
          runtimeType == other.runtimeType &&
          key == other.key &&
          value == other.value &&
          fromImage == other.fromImage;
}

class EnvVar implements WslcJsonModel {
  factory EnvVar.fromJson(Map<String, dynamic> json) =>
      EnvVar(key: json['key'] as String, value: json['value'] as String);
  @override
  Map<String, dynamic> toJson() => {'key': wslcEncode(key), 'value': wslcEncode(value)};

  final String key;
  final String value;

  const EnvVar({required this.key, required this.value});

  @override
  int get hashCode => key.hashCode ^ value.hashCode;

  @override
  bool operator ==(Object other) =>
      identical(this, other) ||
      other is EnvVar && runtimeType == other.runtimeType && key == other.key && value == other.value;
}

class EventBatch implements WslcJsonModel {
  factory EventBatch.fromJson(Map<String, dynamic> json) => EventBatch(
    events: (json['events'] as List).map((item) => DeskEvent.fromJson((item as Map).cast<String, dynamic>())).toList(),
    running: json['running'] as bool,
  );
  @override
  Map<String, dynamic> toJson() => {'events': wslcEncode(events), 'running': wslcEncode(running)};

  final List<DeskEvent> events;
  final bool running;

  const EventBatch({required this.events, required this.running});

  @override
  int get hashCode => events.hashCode ^ running.hashCode;

  @override
  bool operator ==(Object other) =>
      identical(this, other) ||
      other is EventBatch && runtimeType == other.runtimeType && events == other.events && running == other.running;
}

class FileEntry implements WslcJsonModel {
  factory FileEntry.fromJson(Map<String, dynamic> json) =>
      FileEntry(name: json['name'] as String, isDir: json['is_dir'] as bool, isLink: json['is_link'] as bool);
  @override
  Map<String, dynamic> toJson() => {
    'name': wslcEncode(name),
    'is_dir': wslcEncode(isDir),
    'is_link': wslcEncode(isLink),
  };

  final String name;
  final bool isDir;
  final bool isLink;

  const FileEntry({required this.name, required this.isDir, required this.isLink});

  @override
  int get hashCode => name.hashCode ^ isDir.hashCode ^ isLink.hashCode;

  @override
  bool operator ==(Object other) =>
      identical(this, other) ||
      other is FileEntry &&
          runtimeType == other.runtimeType &&
          name == other.name &&
          isDir == other.isDir &&
          isLink == other.isLink;
}

/// Container health check; durations as `30s`, `1m30s`.
class HealthSpec implements WslcJsonModel {
  factory HealthSpec.fromJson(Map<String, dynamic> json) => HealthSpec(
    cmd: json['cmd'] as String,
    interval: json['interval'] == null ? null : json['interval'] as String,
    timeout: json['timeout'] == null ? null : json['timeout'] as String,
    retries: json['retries'] == null ? null : (json['retries'] as num).toInt(),
    startPeriod: json['start_period'] == null ? null : json['start_period'] as String,
  );
  @override
  Map<String, dynamic> toJson() => {
    'cmd': wslcEncode(cmd),
    'interval': wslcEncode(interval),
    'timeout': wslcEncode(timeout),
    'retries': wslcEncode(retries),
    'start_period': wslcEncode(startPeriod),
  };

  /// Shell command, as `--health-cmd` expects.
  final String cmd;
  final String? interval;
  final String? timeout;
  final int? retries;
  final String? startPeriod;

  const HealthSpec({required this.cmd, this.interval, this.timeout, this.retries, this.startPeriod});

  @override
  int get hashCode => cmd.hashCode ^ interval.hashCode ^ timeout.hashCode ^ retries.hashCode ^ startPeriod.hashCode;

  @override
  bool operator ==(Object other) =>
      identical(this, other) ||
      other is HealthSpec &&
          runtimeType == other.runtimeType &&
          cmd == other.cmd &&
          interval == other.interval &&
          timeout == other.timeout &&
          retries == other.retries &&
          startPeriod == other.startPeriod;
}

class ImageSummary implements WslcJsonModel {
  factory ImageSummary.fromJson(Map<String, dynamic> json) => ImageSummary(
    id: json['id'] as String,
    repository: json['repository'] == null ? null : json['repository'] as String,
    tag: json['tag'] == null ? null : json['tag'] as String,
    sizeDisplay: json['size_display'] == null ? null : json['size_display'] as String,
    createdAt: json['created_at'] == null ? null : json['created_at'] as String,
    containers: json['containers'] == null ? null : (json['containers'] as num).toInt(),
  );
  @override
  Map<String, dynamic> toJson() => {
    'id': wslcEncode(id),
    'repository': wslcEncode(repository),
    'tag': wslcEncode(tag),
    'size_display': wslcEncode(sizeDisplay),
    'created_at': wslcEncode(createdAt),
    'containers': wslcEncode(containers),
  };

  final String id;
  final String? repository;
  final String? tag;
  final String? sizeDisplay;
  final String? createdAt;

  /// Containers created from this image (absent in 3.0.1).
  final int? containers;

  const ImageSummary({required this.id, this.repository, this.tag, this.sizeDisplay, this.createdAt, this.containers});

  @override
  int get hashCode =>
      id.hashCode ^
      repository.hashCode ^
      tag.hashCode ^
      sizeDisplay.hashCode ^
      createdAt.hashCode ^
      containers.hashCode;

  @override
  bool operator ==(Object other) =>
      identical(this, other) ||
      other is ImageSummary &&
          runtimeType == other.runtimeType &&
          id == other.id &&
          repository == other.repository &&
          tag == other.tag &&
          sizeDisplay == other.sizeDisplay &&
          createdAt == other.createdAt &&
          containers == other.containers;
}

class JobBatch implements WslcJsonModel {
  factory JobBatch.fromJson(Map<String, dynamic> json) => JobBatch(
    lines: (json['lines'] as List).map((item) => JobLine.fromJson((item as Map).cast<String, dynamic>())).toList(),
    finished: json['finished'] as bool,
    success: json['success'] as bool,
    exitCode: json['exit_code'] == null ? null : (json['exit_code'] as num).toInt(),
  );
  @override
  Map<String, dynamic> toJson() => {
    'lines': wslcEncode(lines),
    'finished': wslcEncode(finished),
    'success': wslcEncode(success),
    'exit_code': wslcEncode(exitCode),
  };

  final List<JobLine> lines;
  final bool finished;
  final bool success;
  final int? exitCode;

  const JobBatch({required this.lines, required this.finished, required this.success, this.exitCode});

  @override
  int get hashCode => lines.hashCode ^ finished.hashCode ^ success.hashCode ^ exitCode.hashCode;

  @override
  bool operator ==(Object other) =>
      identical(this, other) ||
      other is JobBatch &&
          runtimeType == other.runtimeType &&
          lines == other.lines &&
          finished == other.finished &&
          success == other.success &&
          exitCode == other.exitCode;
}

class JobLine implements WslcJsonModel {
  factory JobLine.fromJson(Map<String, dynamic> json) =>
      JobLine(text: json['text'] as String, progress: json['progress'] as bool);
  @override
  Map<String, dynamic> toJson() => {'text': wslcEncode(text), 'progress': wslcEncode(progress)};

  final String text;

  /// Ended by a lone CR: replaces the previous progress line.
  final bool progress;

  const JobLine({required this.text, required this.progress});

  @override
  int get hashCode => text.hashCode ^ progress.hashCode;

  @override
  bool operator ==(Object other) =>
      identical(this, other) ||
      other is JobLine && runtimeType == other.runtimeType && text == other.text && progress == other.progress;
}

class LogBatch implements WslcJsonModel {
  factory LogBatch.fromJson(Map<String, dynamic> json) => LogBatch(
    entries: (json['entries'] as List).map((item) => LogEntry.fromJson((item as Map).cast<String, dynamic>())).toList(),
    status: json['status'] as String,
    droppedTotal: BigInt.parse(json['dropped_total'].toString()),
    truncatedTotal: BigInt.parse(json['truncated_total'].toString()),
  );
  @override
  Map<String, dynamic> toJson() => {
    'entries': wslcEncode(entries),
    'status': wslcEncode(status),
    'dropped_total': wslcEncode(droppedTotal),
    'truncated_total': wslcEncode(truncatedTotal),
  };

  final List<LogEntry> entries;
  final String status;
  final BigInt droppedTotal;
  final BigInt truncatedTotal;

  const LogBatch({
    required this.entries,
    required this.status,
    required this.droppedTotal,
    required this.truncatedTotal,
  });

  @override
  int get hashCode => entries.hashCode ^ status.hashCode ^ droppedTotal.hashCode ^ truncatedTotal.hashCode;

  @override
  bool operator ==(Object other) =>
      identical(this, other) ||
      other is LogBatch &&
          runtimeType == other.runtimeType &&
          entries == other.entries &&
          status == other.status &&
          droppedTotal == other.droppedTotal &&
          truncatedTotal == other.truncatedTotal;
}

class LogEntry implements WslcJsonModel {
  factory LogEntry.fromJson(Map<String, dynamic> json) => LogEntry(
    sequence: BigInt.parse(json['sequence'].toString()),
    stream: json['stream'] as String,
    text: json['text'] as String,
    truncated: json['truncated'] as bool,
  );
  @override
  Map<String, dynamic> toJson() => {
    'sequence': wslcEncode(sequence),
    'stream': wslcEncode(stream),
    'text': wslcEncode(text),
    'truncated': wslcEncode(truncated),
  };

  final BigInt sequence;
  final String stream;
  final String text;
  final bool truncated;

  const LogEntry({required this.sequence, required this.stream, required this.text, required this.truncated});

  @override
  int get hashCode => sequence.hashCode ^ stream.hashCode ^ text.hashCode ^ truncated.hashCode;

  @override
  bool operator ==(Object other) =>
      identical(this, other) ||
      other is LogEntry &&
          runtimeType == other.runtimeType &&
          sequence == other.sequence &&
          stream == other.stream &&
          text == other.text &&
          truncated == other.truncated;
}

/// History window for a log subscription; `follow` is always on.
class LogOptions implements WslcJsonModel {
  factory LogOptions.fromJson(Map<String, dynamic> json) => LogOptions(
    tail: json['tail'] == null ? null : (json['tail'] as num).toInt(),
    since: json['since'] == null ? null : json['since'] as String,
    timestamps: json['timestamps'] as bool,
  );
  @override
  Map<String, dynamic> toJson() => {
    'tail': wslcEncode(tail),
    'since': wslcEncode(since),
    'timestamps': wslcEncode(timestamps),
  };

  /// Last N lines; `None` with `since` reads everything after that moment.
  final int? tail;

  /// Unix seconds or RFC 3339, as accepted by `wslc container logs --since`.
  final String? since;
  final bool timestamps;

  const LogOptions({this.tail, this.since, required this.timestamps});

  @override
  int get hashCode => tail.hashCode ^ since.hashCode ^ timestamps.hashCode;

  @override
  bool operator ==(Object other) =>
      identical(this, other) ||
      other is LogOptions &&
          runtimeType == other.runtimeType &&
          tail == other.tail &&
          since == other.since &&
          timestamps == other.timestamps;
}

class MountInfo implements WslcJsonModel {
  factory MountInfo.fromJson(Map<String, dynamic> json) => MountInfo(
    kind: json['kind'] == null ? null : json['kind'] as String,
    name: json['name'] == null ? null : json['name'] as String,
    source: json['source'] == null ? null : json['source'] as String,
    destination: json['destination'] == null ? null : json['destination'] as String,
    readWrite: json['read_write'] == null ? null : json['read_write'] as bool,
  );
  @override
  Map<String, dynamic> toJson() => {
    'kind': wslcEncode(kind),
    'name': wslcEncode(name),
    'source': wslcEncode(source),
    'destination': wslcEncode(destination),
    'read_write': wslcEncode(readWrite),
  };

  final String? kind;
  final String? name;
  final String? source;
  final String? destination;
  final bool? readWrite;

  const MountInfo({this.kind, this.name, this.source, this.destination, this.readWrite});

  @override
  int get hashCode => kind.hashCode ^ name.hashCode ^ source.hashCode ^ destination.hashCode ^ readWrite.hashCode;

  @override
  bool operator ==(Object other) =>
      identical(this, other) ||
      other is MountInfo &&
          runtimeType == other.runtimeType &&
          kind == other.kind &&
          name == other.name &&
          source == other.source &&
          destination == other.destination &&
          readWrite == other.readWrite;
}

class MountSpec implements WslcJsonModel {
  factory MountSpec.fromJson(Map<String, dynamic> json) => MountSpec(
    volume: json['volume'] as bool,
    source: json['source'] as String,
    target: json['target'] as String,
    readOnly: json['read_only'] as bool,
  );
  @override
  Map<String, dynamic> toJson() => {
    'volume': wslcEncode(volume),
    'source': wslcEncode(source),
    'target': wslcEncode(target),
    'read_only': wslcEncode(readOnly),
  };

  /// Named volume when true, otherwise an existing Windows folder or file.
  final bool volume;
  final String source;
  final String target;
  final bool readOnly;

  const MountSpec({required this.volume, required this.source, required this.target, required this.readOnly});

  @override
  int get hashCode => volume.hashCode ^ source.hashCode ^ target.hashCode ^ readOnly.hashCode;

  @override
  bool operator ==(Object other) =>
      identical(this, other) ||
      other is MountSpec &&
          runtimeType == other.runtimeType &&
          volume == other.volume &&
          source == other.source &&
          target == other.target &&
          readOnly == other.readOnly;
}

class NetworkAttachment implements WslcJsonModel {
  factory NetworkAttachment.fromJson(Map<String, dynamic> json) => NetworkAttachment(
    name: json['name'] as String,
    ipAddress: json['ip_address'] == null ? null : json['ip_address'] as String,
    gateway: json['gateway'] == null ? null : json['gateway'] as String,
    macAddress: json['mac_address'] == null ? null : json['mac_address'] as String,
  );
  @override
  Map<String, dynamic> toJson() => {
    'name': wslcEncode(name),
    'ip_address': wslcEncode(ipAddress),
    'gateway': wslcEncode(gateway),
    'mac_address': wslcEncode(macAddress),
  };

  final String name;
  final String? ipAddress;
  final String? gateway;
  final String? macAddress;

  const NetworkAttachment({required this.name, this.ipAddress, this.gateway, this.macAddress});

  @override
  int get hashCode => name.hashCode ^ ipAddress.hashCode ^ gateway.hashCode ^ macAddress.hashCode;

  @override
  bool operator ==(Object other) =>
      identical(this, other) ||
      other is NetworkAttachment &&
          runtimeType == other.runtimeType &&
          name == other.name &&
          ipAddress == other.ipAddress &&
          gateway == other.gateway &&
          macAddress == other.macAddress;
}

class NetworkSpec implements WslcJsonModel {
  factory NetworkSpec.fromJson(Map<String, dynamic> json) => NetworkSpec(
    name: json['name'] as String,
    driver: json['driver'] == null ? null : json['driver'] as String,
    subnet: json['subnet'] == null ? null : json['subnet'] as String,
    gateway: json['gateway'] == null ? null : json['gateway'] as String,
    ipRange: json['ip_range'] == null ? null : json['ip_range'] as String,
    internal: json['internal'] as bool,
    labels: (json['labels'] as List).map((item) => EnvVar.fromJson((item as Map).cast<String, dynamic>())).toList(),
  );
  @override
  Map<String, dynamic> toJson() => {
    'name': wslcEncode(name),
    'driver': wslcEncode(driver),
    'subnet': wslcEncode(subnet),
    'gateway': wslcEncode(gateway),
    'ip_range': wslcEncode(ipRange),
    'internal': wslcEncode(internal),
    'labels': wslcEncode(labels),
  };

  final String name;
  final String? driver;
  final String? subnet;
  final String? gateway;
  final String? ipRange;
  final bool internal;
  final List<EnvVar> labels;

  const NetworkSpec({
    required this.name,
    this.driver,
    this.subnet,
    this.gateway,
    this.ipRange,
    required this.internal,
    required this.labels,
  });

  @override
  int get hashCode =>
      name.hashCode ^
      driver.hashCode ^
      subnet.hashCode ^
      gateway.hashCode ^
      ipRange.hashCode ^
      internal.hashCode ^
      labels.hashCode;

  @override
  bool operator ==(Object other) =>
      identical(this, other) ||
      other is NetworkSpec &&
          runtimeType == other.runtimeType &&
          name == other.name &&
          driver == other.driver &&
          subnet == other.subnet &&
          gateway == other.gateway &&
          ipRange == other.ipRange &&
          internal == other.internal &&
          labels == other.labels;
}

class NetworkSummary implements WslcJsonModel {
  factory NetworkSummary.fromJson(Map<String, dynamic> json) => NetworkSummary(
    id: json['id'] as String,
    name: json['name'] == null ? null : json['name'] as String,
    driver: json['driver'] == null ? null : json['driver'] as String,
    scope: json['scope'] == null ? null : json['scope'] as String,
    ipv4: json['ipv4'] == null ? null : json['ipv4'] as bool,
    ipv6: json['ipv6'] == null ? null : json['ipv6'] as bool,
    internal: json['internal'] == null ? null : json['internal'] as bool,
  );
  @override
  Map<String, dynamic> toJson() => {
    'id': wslcEncode(id),
    'name': wslcEncode(name),
    'driver': wslcEncode(driver),
    'scope': wslcEncode(scope),
    'ipv4': wslcEncode(ipv4),
    'ipv6': wslcEncode(ipv6),
    'internal': wslcEncode(internal),
  };

  final String id;
  final String? name;
  final String? driver;
  final String? scope;
  final bool? ipv4;
  final bool? ipv6;
  final bool? internal;

  const NetworkSummary({required this.id, this.name, this.driver, this.scope, this.ipv4, this.ipv6, this.internal});

  @override
  int get hashCode =>
      id.hashCode ^
      name.hashCode ^
      driver.hashCode ^
      scope.hashCode ^
      ipv4.hashCode ^
      ipv6.hashCode ^
      internal.hashCode;

  @override
  bool operator ==(Object other) =>
      identical(this, other) ||
      other is NetworkSummary &&
          runtimeType == other.runtimeType &&
          id == other.id &&
          name == other.name &&
          driver == other.driver &&
          scope == other.scope &&
          ipv4 == other.ipv4 &&
          ipv6 == other.ipv6 &&
          internal == other.internal;
}

class OperationResult implements WslcJsonModel {
  factory OperationResult.fromJson(Map<String, dynamic> json) => OperationResult(
    containerId: json['container_id'] as String,
    uncertain: json['uncertain'] as bool,
    commandError: json['command_error'] == null
        ? null
        : AppError.fromJson((json['command_error'] as Map).cast<String, dynamic>()),
    current: json['current'] == null
        ? null
        : ContainerDetails.fromJson((json['current'] as Map).cast<String, dynamic>()),
    verificationError: json['verification_error'] == null
        ? null
        : AppError.fromJson((json['verification_error'] as Map).cast<String, dynamic>()),
  );
  @override
  Map<String, dynamic> toJson() => {
    'container_id': wslcEncode(containerId),
    'uncertain': wslcEncode(uncertain),
    'command_error': wslcEncode(commandError),
    'current': wslcEncode(current),
    'verification_error': wslcEncode(verificationError),
  };

  final String containerId;
  final bool uncertain;
  final AppError? commandError;
  final ContainerDetails? current;
  final AppError? verificationError;

  const OperationResult({
    required this.containerId,
    required this.uncertain,
    this.commandError,
    this.current,
    this.verificationError,
  });

  @override
  int get hashCode =>
      containerId.hashCode ^ uncertain.hashCode ^ commandError.hashCode ^ current.hashCode ^ verificationError.hashCode;

  @override
  bool operator ==(Object other) =>
      identical(this, other) ||
      other is OperationResult &&
          runtimeType == other.runtimeType &&
          containerId == other.containerId &&
          uncertain == other.uncertain &&
          commandError == other.commandError &&
          current == other.current &&
          verificationError == other.verificationError;
}

class PortBinding implements WslcJsonModel {
  factory PortBinding.fromJson(Map<String, dynamic> json) => PortBinding(
    containerPort: json['container_port'] as String,
    hostIp: json['host_ip'] == null ? null : json['host_ip'] as String,
    hostPort: json['host_port'] == null ? null : json['host_port'] as String,
  );
  @override
  Map<String, dynamic> toJson() => {
    'container_port': wslcEncode(containerPort),
    'host_ip': wslcEncode(hostIp),
    'host_port': wslcEncode(hostPort),
  };

  final String containerPort;
  final String? hostIp;
  final String? hostPort;

  const PortBinding({required this.containerPort, this.hostIp, this.hostPort});

  @override
  int get hashCode => containerPort.hashCode ^ hostIp.hashCode ^ hostPort.hashCode;

  @override
  bool operator ==(Object other) =>
      identical(this, other) ||
      other is PortBinding &&
          runtimeType == other.runtimeType &&
          containerPort == other.containerPort &&
          hostIp == other.hostIp &&
          hostPort == other.hostPort;
}

class PortSpec implements WslcJsonModel {
  factory PortSpec.fromJson(Map<String, dynamic> json) => PortSpec(
    hostIp: json['host_ip'] == null ? null : json['host_ip'] as String,
    hostPort: (json['host_port'] as num).toInt(),
    containerPort: (json['container_port'] as num).toInt(),
    udp: json['udp'] as bool,
  );
  @override
  Map<String, dynamic> toJson() => {
    'host_ip': wslcEncode(hostIp),
    'host_port': wslcEncode(hostPort),
    'container_port': wslcEncode(containerPort),
    'udp': wslcEncode(udp),
  };

  /// `None` keeps the WSLc default (127.0.0.1); `0.0.0.0` publishes on every interface.
  final String? hostIp;
  final int hostPort;
  final int containerPort;
  final bool udp;

  const PortSpec({this.hostIp, required this.hostPort, required this.containerPort, required this.udp});

  @override
  int get hashCode => hostIp.hashCode ^ hostPort.hashCode ^ containerPort.hashCode ^ udp.hashCode;

  @override
  bool operator ==(Object other) =>
      identical(this, other) ||
      other is PortSpec &&
          runtimeType == other.runtimeType &&
          hostIp == other.hostIp &&
          hostPort == other.hostPort &&
          containerPort == other.containerPort &&
          udp == other.udp;
}

/// `wslc run` options beyond the wizard form.
class RunExtras implements WslcJsonModel {
  factory RunExtras.fromJson(Map<String, dynamic> json) => RunExtras(
    labels: (json['labels'] as List).map((item) => EnvVar.fromJson((item as Map).cast<String, dynamic>())).toList(),
    hostname: json['hostname'] == null ? null : json['hostname'] as String,
    domainname: json['domainname'] == null ? null : json['domainname'] as String,
    networkAliases: (json['network_aliases'] as List).map((item) => item as String).toList(),
    ip: json['ip'] == null ? null : json['ip'] as String,
    health: json['health'] == null ? null : HealthSpec.fromJson((json['health'] as Map).cast<String, dynamic>()),
    noHealthcheck: json['no_healthcheck'] as bool,
    tmpfs: (json['tmpfs'] as List).map((item) => item as String).toList(),
    ulimits: (json['ulimits'] as List).map((item) => item as String).toList(),
    shmSize: json['shm_size'] == null ? null : json['shm_size'] as String,
    stopSignal: json['stop_signal'] == null ? null : json['stop_signal'] as String,
    stopTimeout: json['stop_timeout'] == null ? null : (json['stop_timeout'] as num).toInt(),
    dns: (json['dns'] as List).map((item) => item as String).toList(),
    dnsSearch: (json['dns_search'] as List).map((item) => item as String).toList(),
    dnsOptions: (json['dns_options'] as List).map((item) => item as String).toList(),
    tty: json['tty'] as bool,
    interactive: json['interactive'] as bool,
    gpus: json['gpus'] == null ? null : json['gpus'] as String,
  );
  @override
  Map<String, dynamic> toJson() => {
    'labels': wslcEncode(labels),
    'hostname': wslcEncode(hostname),
    'domainname': wslcEncode(domainname),
    'network_aliases': wslcEncode(networkAliases),
    'ip': wslcEncode(ip),
    'health': wslcEncode(health),
    'no_healthcheck': wslcEncode(noHealthcheck),
    'tmpfs': wslcEncode(tmpfs),
    'ulimits': wslcEncode(ulimits),
    'shm_size': wslcEncode(shmSize),
    'stop_signal': wslcEncode(stopSignal),
    'stop_timeout': wslcEncode(stopTimeout),
    'dns': wslcEncode(dns),
    'dns_search': wslcEncode(dnsSearch),
    'dns_options': wslcEncode(dnsOptions),
    'tty': wslcEncode(tty),
    'interactive': wslcEncode(interactive),
    'gpus': wslcEncode(gpus),
  };

  final List<EnvVar> labels;
  final String? hostname;
  final String? domainname;

  /// Aliases on the network the container is created in.
  final List<String> networkAliases;

  /// Fixed IPv4 address on that network.
  final String? ip;
  final HealthSpec? health;
  final bool noHealthcheck;

  /// `path[:options]`, e.g. `/run:size=64m`.
  final List<String> tmpfs;

  /// `name=soft[:hard]`.
  final List<String> ulimits;
  final String? shmSize;
  final String? stopSignal;
  final int? stopTimeout;
  final List<String> dns;
  final List<String> dnsSearch;
  final List<String> dnsOptions;
  final bool tty;
  final bool interactive;

  /// `all` or a GPU count.
  final String? gpus;

  const RunExtras({
    required this.labels,
    this.hostname,
    this.domainname,
    required this.networkAliases,
    this.ip,
    this.health,
    required this.noHealthcheck,
    required this.tmpfs,
    required this.ulimits,
    this.shmSize,
    this.stopSignal,
    this.stopTimeout,
    required this.dns,
    required this.dnsSearch,
    required this.dnsOptions,
    required this.tty,
    required this.interactive,
    this.gpus,
  });

  @override
  int get hashCode =>
      labels.hashCode ^
      hostname.hashCode ^
      domainname.hashCode ^
      networkAliases.hashCode ^
      ip.hashCode ^
      health.hashCode ^
      noHealthcheck.hashCode ^
      tmpfs.hashCode ^
      ulimits.hashCode ^
      shmSize.hashCode ^
      stopSignal.hashCode ^
      stopTimeout.hashCode ^
      dns.hashCode ^
      dnsSearch.hashCode ^
      dnsOptions.hashCode ^
      tty.hashCode ^
      interactive.hashCode ^
      gpus.hashCode;

  @override
  bool operator ==(Object other) =>
      identical(this, other) ||
      other is RunExtras &&
          runtimeType == other.runtimeType &&
          labels == other.labels &&
          hostname == other.hostname &&
          domainname == other.domainname &&
          networkAliases == other.networkAliases &&
          ip == other.ip &&
          health == other.health &&
          noHealthcheck == other.noHealthcheck &&
          tmpfs == other.tmpfs &&
          ulimits == other.ulimits &&
          shmSize == other.shmSize &&
          stopSignal == other.stopSignal &&
          stopTimeout == other.stopTimeout &&
          dns == other.dns &&
          dnsSearch == other.dnsSearch &&
          dnsOptions == other.dnsOptions &&
          tty == other.tty &&
          interactive == other.interactive &&
          gpus == other.gpus;
}

/// Everything the "new container" wizard can set; turned into validated `wslc run` arguments.
class RunSpec implements WslcJsonModel {
  factory RunSpec.fromJson(Map<String, dynamic> json) => RunSpec(
    image: json['image'] as String,
    name: json['name'] == null ? null : json['name'] as String,
    ports: (json['ports'] as List).map((item) => PortSpec.fromJson((item as Map).cast<String, dynamic>())).toList(),
    mounts: (json['mounts'] as List).map((item) => MountSpec.fromJson((item as Map).cast<String, dynamic>())).toList(),
    env: (json['env'] as List).map((item) => EnvVar.fromJson((item as Map).cast<String, dynamic>())).toList(),
    command: (json['command'] as List).map((item) => item as String).toList(),
    entrypoint: json['entrypoint'] == null ? null : json['entrypoint'] as String,
    workdir: json['workdir'] == null ? null : json['workdir'] as String,
    user: json['user'] == null ? null : json['user'] as String,
    network: json['network'] == null ? null : json['network'] as String,
    cpus: json['cpus'] == null ? null : json['cpus'] as String,
    memory: json['memory'] == null ? null : json['memory'] as String,
    autoRemove: json['auto_remove'] as bool,
    start: json['start'] as bool,
    extras: json['extras'] == null ? null : RunExtras.fromJson((json['extras'] as Map).cast<String, dynamic>()),
  );
  @override
  Map<String, dynamic> toJson() => {
    'image': wslcEncode(image),
    'name': wslcEncode(name),
    'ports': wslcEncode(ports),
    'mounts': wslcEncode(mounts),
    'env': wslcEncode(env),
    'command': wslcEncode(command),
    'entrypoint': wslcEncode(entrypoint),
    'workdir': wslcEncode(workdir),
    'user': wslcEncode(user),
    'network': wslcEncode(network),
    'cpus': wslcEncode(cpus),
    'memory': wslcEncode(memory),
    'auto_remove': wslcEncode(autoRemove),
    'start': wslcEncode(start),
    'extras': wslcEncode(extras),
  };

  final String image;
  final String? name;
  final List<PortSpec> ports;
  final List<MountSpec> mounts;
  final List<EnvVar> env;
  final List<String> command;
  final String? entrypoint;
  final String? workdir;
  final String? user;
  final String? network;
  final String? cpus;
  final String? memory;
  final bool autoRemove;

  /// `run -d` when true, `create` otherwise.
  final bool start;

  /// Less common `run` options, filled by Docker Compose import.
  final RunExtras? extras;

  const RunSpec({
    required this.image,
    this.name,
    required this.ports,
    required this.mounts,
    required this.env,
    required this.command,
    this.entrypoint,
    this.workdir,
    this.user,
    this.network,
    this.cpus,
    this.memory,
    required this.autoRemove,
    required this.start,
    this.extras,
  });

  @override
  int get hashCode =>
      image.hashCode ^
      name.hashCode ^
      ports.hashCode ^
      mounts.hashCode ^
      env.hashCode ^
      command.hashCode ^
      entrypoint.hashCode ^
      workdir.hashCode ^
      user.hashCode ^
      network.hashCode ^
      cpus.hashCode ^
      memory.hashCode ^
      autoRemove.hashCode ^
      start.hashCode ^
      extras.hashCode;

  @override
  bool operator ==(Object other) =>
      identical(this, other) ||
      other is RunSpec &&
          runtimeType == other.runtimeType &&
          image == other.image &&
          name == other.name &&
          ports == other.ports &&
          mounts == other.mounts &&
          env == other.env &&
          command == other.command &&
          entrypoint == other.entrypoint &&
          workdir == other.workdir &&
          user == other.user &&
          network == other.network &&
          cpus == other.cpus &&
          memory == other.memory &&
          autoRemove == other.autoRemove &&
          start == other.start &&
          extras == other.extras;
}

class RuntimeInfo implements WslcJsonModel {
  factory RuntimeInfo.fromJson(Map<String, dynamic> json) => RuntimeInfo(
    cliPath: json['cli_path'] as String,
    cliVersion: json['cli_version'] as String,
    versionSupported: json['version_supported'] as bool,
    versionVerified: json['version_verified'] as bool,
    readable: json['readable'] as bool,
    error: json['error'] == null ? null : AppError.fromJson((json['error'] as Map).cast<String, dynamic>()),
  );
  @override
  Map<String, dynamic> toJson() => {
    'cli_path': wslcEncode(cliPath),
    'cli_version': wslcEncode(cliVersion),
    'version_supported': wslcEncode(versionSupported),
    'version_verified': wslcEncode(versionVerified),
    'readable': wslcEncode(readable),
    'error': wslcEncode(error),
  };

  final String cliPath;
  final String cliVersion;
  final bool versionSupported;
  final bool versionVerified;
  final bool readable;
  final AppError? error;

  const RuntimeInfo({
    required this.cliPath,
    required this.cliVersion,
    required this.versionSupported,
    required this.versionVerified,
    required this.readable,
    this.error,
  });

  @override
  int get hashCode =>
      cliPath.hashCode ^
      cliVersion.hashCode ^
      versionSupported.hashCode ^
      versionVerified.hashCode ^
      readable.hashCode ^
      error.hashCode;

  @override
  bool operator ==(Object other) =>
      identical(this, other) ||
      other is RuntimeInfo &&
          runtimeType == other.runtimeType &&
          cliPath == other.cliPath &&
          cliVersion == other.cliVersion &&
          versionSupported == other.versionSupported &&
          versionVerified == other.versionVerified &&
          readable == other.readable &&
          error == other.error;
}

class TerminalOutput implements WslcJsonModel {
  factory TerminalOutput.fromJson(Map<String, dynamic> json) =>
      TerminalOutput(data: Uint8List.fromList((json['data'] as List).cast<int>()), alive: json['alive'] as bool);
  @override
  Map<String, dynamic> toJson() => {'data': wslcEncode(data), 'alive': wslcEncode(alive)};

  final Uint8List data;
  final bool alive;

  const TerminalOutput({required this.data, required this.alive});

  @override
  int get hashCode => data.hashCode ^ alive.hashCode;

  @override
  bool operator ==(Object other) =>
      identical(this, other) ||
      other is TerminalOutput && runtimeType == other.runtimeType && data == other.data && alive == other.alive;
}

class VolumeSpec implements WslcJsonModel {
  factory VolumeSpec.fromJson(Map<String, dynamic> json) => VolumeSpec(
    name: json['name'] as String,
    driver: json['driver'] == null ? null : json['driver'] as String,
    options: (json['options'] as List).map((item) => EnvVar.fromJson((item as Map).cast<String, dynamic>())).toList(),
    labels: (json['labels'] as List).map((item) => EnvVar.fromJson((item as Map).cast<String, dynamic>())).toList(),
  );
  @override
  Map<String, dynamic> toJson() => {
    'name': wslcEncode(name),
    'driver': wslcEncode(driver),
    'options': wslcEncode(options),
    'labels': wslcEncode(labels),
  };

  final String name;

  /// `guest` (default) or `vhd`.
  final String? driver;
  final List<EnvVar> options;
  final List<EnvVar> labels;

  const VolumeSpec({required this.name, this.driver, required this.options, required this.labels});

  @override
  int get hashCode => name.hashCode ^ driver.hashCode ^ options.hashCode ^ labels.hashCode;

  @override
  bool operator ==(Object other) =>
      identical(this, other) ||
      other is VolumeSpec &&
          runtimeType == other.runtimeType &&
          name == other.name &&
          driver == other.driver &&
          options == other.options &&
          labels == other.labels;
}

class VolumeSummary implements WslcJsonModel {
  factory VolumeSummary.fromJson(Map<String, dynamic> json) => VolumeSummary(
    name: json['name'] as String,
    driver: json['driver'] == null ? null : json['driver'] as String,
    sizeDisplay: json['size_display'] == null ? null : json['size_display'] as String,
  );
  @override
  Map<String, dynamic> toJson() => {
    'name': wslcEncode(name),
    'driver': wslcEncode(driver),
    'size_display': wslcEncode(sizeDisplay),
  };

  final String name;
  final String? driver;
  final String? sizeDisplay;

  const VolumeSummary({required this.name, this.driver, this.sizeDisplay});

  @override
  int get hashCode => name.hashCode ^ driver.hashCode ^ sizeDisplay.hashCode;

  @override
  bool operator ==(Object other) =>
      identical(this, other) ||
      other is VolumeSummary &&
          runtimeType == other.runtimeType &&
          name == other.name &&
          driver == other.driver &&
          sizeDisplay == other.sizeDisplay;
}
