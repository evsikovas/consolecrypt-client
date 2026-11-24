import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/credentials/credential_dialogs.dart';
import 'package:consolecrypt/docker/docker_kube_plan.dart';
import 'package:consolecrypt/docker/docker_providers.dart';
import 'package:consolecrypt/docker/docker_service.dart';
import 'package:file_selector/file_selector.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

class RegistryPublisher extends ConsumerStatefulWidget {
  const RegistryPublisher({
    required this.enabled,
    required this.publishing,
    required this.engineName,
    required this.sourceImage,
    required this.publish,
    required this.allowBuild,
    required this.remoteBuild,
    super.key,
  });
  final bool enabled;
  final bool publishing, allowBuild, remoteBuild;
  final String engineName, sourceImage;
  final Future<void> Function(Credential, String, String, DockerBuildContext?) publish;
  @override
  ConsumerState<RegistryPublisher> createState() => _RegistryPublisherState();
}

class _RegistryPublisherState extends ConsumerState<RegistryPublisher> {
  String? selected;
  bool repositoryEdited = false;
  bool rebuild = false;
  String? directory;
  final platforms = <String>{'linux/amd64', 'linux/arm64'};

  Future<void> chooseDirectory() async {
    final chosen = await getDirectoryPath();
    if (mounted && chosen != null) setState(() => directory = chosen);
  }

  late final String sourceName;
  final repository = TextEditingController(), tag = TextEditingController();
  static final repoPattern = RegExp(
    r'^[a-z0-9]+(?:(?:[._]|__|-+)[a-z0-9]+)*(?:/[a-z0-9]+(?:(?:[._]|__|-+)[a-z0-9]+)*)*$',
  );

  @override
  void initState() {
    super.initState();
    final name = widget.sourceImage.split('@').first.split('/').last;
    final parts = name.split(':');
    sourceName = widget.sourceImage.startsWith('sha256:') ? '' : parts.first;
    repository.text = sourceName;
    tag.text = parts.length == 2 && !widget.sourceImage.startsWith('sha256:') ? parts.last : 'latest';
  }

  @override
  void dispose() {
    repository.dispose();
    tag.dispose();
    super.dispose();
  }

  Future<void> add() async {
    final scope = ref.read(dockerScopeProvider);
    final created = await showCreateCredentialDialog(context, NewCredentialKind.registry);
    if (mounted && created != null && identical(scope, ref.read(dockerScopeProvider))) select(created);
  }

  void select(Credential? account) {
    setState(() {
      selected = account?.id.value;
      if (repositoryEdited) return;
      final user = account?.username;
      repository.text =
          account?.registry == 'docker.io' && sourceName.isNotEmpty && user != null && repoPattern.hasMatch(user)
          ? '$user/$sourceName'
          : sourceName;
    });
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    final credentials = (ref.watch(credentialsProvider).value ?? const <Credential>[]).where((c) => c.registry != null);
    final account = credentials.where((c) => c.id.value == selected).firstOrNull;
    final path = repository.text.trim(), version = tag.text.trim();
    final dest = '${account?.registry}/$path:$version';
    final buildDirectory = directory;
    final directoryReady =
        buildDirectory != null &&
        (!widget.remoteBuild ||
            (buildDirectory.startsWith('/') &&
                buildDirectory != '/' &&
                !RegExp(r'[\x00-\x1f\x7f]').hasMatch(buildDirectory)));
    final valid =
        account != null &&
        repoPattern.hasMatch(path) &&
        !version.contains('@') &&
        DockerKubeTarget.validImage(dest) &&
        '${account.registry}/$path'.length <= 255 &&
        (account.registry != 'docker.io' || path.contains('/'));
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      spacing: 12,
      children: [
        Row(
          children: [
            Expanded(
              child: DropdownButtonFormField<String>(
                key: ValueKey('registry-picker-${account?.id.value}'),
                initialValue: account?.id.value,
                isExpanded: true,
                decoration: InputDecoration(labelText: l.registryLabel),
                hint: Text(l.registryChoose),
                items: [
                  for (final c in credentials)
                    DropdownMenuItem(
                      value: c.id.value,
                      child: Text('${c.name} · ${c.registry}', overflow: TextOverflow.ellipsis),
                    ),
                ],
                onChanged: widget.enabled
                    ? (id) => select(credentials.where((c) => c.id.value == id).firstOrNull)
                    : null,
              ),
            ),
            const SizedBox(width: 8),
            GlassIconButton(
              key: const ValueKey('registry-add'),
              tooltip: l.registryAdd,
              icon: Icons.add_rounded,
              onPressed: widget.enabled ? add : null,
            ),
          ],
        ),
        if (account != null) ...[
          Row(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Expanded(
                flex: 3,
                child: TextField(
                  key: const ValueKey('registry-repo'),
                  controller: repository,
                  enabled: widget.enabled,
                  autocorrect: false,
                  decoration: InputDecoration(labelText: l.registryRepo, hintText: 'team/app'),
                  onChanged: (_) => setState(() => repositoryEdited = true),
                ),
              ),
              const SizedBox(width: 8),
              Expanded(
                child: TextField(
                  key: const ValueKey('registry-tag'),
                  controller: tag,
                  enabled: widget.enabled,
                  autocorrect: false,
                  decoration: InputDecoration(labelText: l.registryTag),
                  onChanged: (_) => setState(() {}),
                ),
              ),
            ],
          ),
          if (widget.allowBuild)
            CheckboxListTile(
              key: const ValueKey('registry-build-mode'),
              contentPadding: EdgeInsets.zero,
              dense: true,
              controlAffinity: ListTileControlAffinity.leading,
              value: rebuild,
              title: Text(widget.remoteBuild ? l.registryBuildOnSsh(widget.engineName) : l.registryBuildFromSource),
              onChanged: widget.enabled ? (v) => setState(() => rebuild = v == true) : null,
            ),
          if (rebuild) ...[
            if (widget.remoteBuild)
              TextFormField(
                key: const ValueKey('registry-remote-folder'),
                initialValue: directory,
                enabled: widget.enabled,
                autocorrect: false,
                enableSuggestions: false,
                decoration: InputDecoration(
                  labelText: l.registryRemoteDirectory,
                  hintText: '/srv/my-app',
                  helperText: l.registryRemoteAuthHelp,
                  helperMaxLines: 4,
                ),
                onChanged: (value) => setState(() => directory = value),
              )
            else
              Wrap(
                spacing: 12,
                runSpacing: 8,
                crossAxisAlignment: WrapCrossAlignment.center,
                children: [
                  GlassButton(
                    icon: Icons.folder_open_rounded,
                    key: const ValueKey('registry-build-folder'),
                    label: l.registryBuildFolder,
                    onPressed: widget.enabled ? chooseDirectory : null,
                  ),
                  if (directory != null) SelectableText(directory!, style: GlassTokens.of(context).typography.caption),
                ],
              ),
            Wrap(
              spacing: 8,
              runSpacing: 8,
              children: [
                for (final platform in const ['linux/amd64', 'linux/arm64'])
                  FilterChip(
                    label: Text(platform),
                    selected: platforms.contains(platform),
                    onSelected: !widget.enabled
                        ? null
                        : (selected) => setState(() {
                            if (selected) {
                              platforms.add(platform);
                            } else {
                              platforms.remove(platform);
                            }
                          }),
                  ),
              ],
            ),
          ],
          Text(
            rebuild && widget.remoteBuild
                ? '${widget.engineName} · ${directory ?? ''} → $dest'
                : rebuild
                ? dest
                : [if (widget.engineName.isNotEmpty) widget.engineName, '${widget.sourceImage} → $dest'].join(' · '),
            style: GlassTokens.of(context).typography.caption,
          ),
          Align(
            alignment: AlignmentDirectional.centerStart,
            child: GlassButton(
              key: const ValueKey('registry-publish'),
              label: rebuild ? l.registryBuildPush : l.registryPublish,
              busy: widget.publishing,
              icon: Icons.cloud_upload_outlined,
              onPressed: widget.enabled && valid && (!rebuild || (directoryReady && platforms.isNotEmpty))
                  ? () => widget.publish(
                      account,
                      path,
                      version,
                      rebuild ? (directory: directory!, platforms: platforms.toList()..sort()) : null,
                    )
                  : null,
            ),
          ),
        ],
      ],
    );
  }
}
