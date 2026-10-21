import 'dart:async';

import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/docker/docker_providers.dart';
import 'package:consolecrypt/docker/docker_service.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

/// Engine volumes grouped by compose project (unlabelled ones last), each
/// with its driver, mount point and the containers that use it.
class DockerVolumeList extends ConsumerStatefulWidget {
  const DockerVolumeList({required this.service, required this.session, this.query = '', super.key});
  final DockerService service;
  final DockerSessionInfo session;
  final String query;
  @override
  ConsumerState<DockerVolumeList> createState() => DockerVolumeListState();
}

class DockerVolumeListState extends ConsumerState<DockerVolumeList> {
  late final _scope = ref.read(dockerScopeProvider);
  List<DockerVolume>? _volumes;
  Object? _error;
  int _generation = 0;

  @override
  void initState() {
    super.initState();
    unawaited(refresh());
  }

  Future<void> refresh() async {
    final generation = ++_generation;
    try {
      final volumes = await widget.service.volumes(widget.session.id);
      if (mounted && generation == _generation && identical(_scope, ref.read(dockerScopeProvider))) {
        setState(() {
          _volumes = volumes;
          _error = null;
        });
      }
    } catch (error) {
      if (mounted && generation == _generation) setState(() => _error = error);
    }
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    final tokens = GlassTokens.of(context);
    final volumes = _volumes;
    if (_error != null && volumes == null) {
      return Center(
        child: InfoBanner(
          key: const ValueKey('docker-volumes-error'),
          tone: BannerTone.danger,
          message: l.dockerRequestFailed,
          action: GlassButton(label: l.dockerRetry, onPressed: refresh),
        ),
      );
    }
    if (volumes == null) return const Center(child: CircularProgressIndicator());
    final needle = widget.query.trim().toLowerCase();
    final visible = [
      for (final volume in volumes)
        if (needle.isEmpty ||
            volume.name.toLowerCase().contains(needle) ||
            volume.usedBy.any((name) => name.toLowerCase().contains(needle)))
          volume,
    ];
    if (visible.isEmpty) {
      return EmptyState(
        key: const ValueKey('docker-no-volumes'),
        icon: Icons.storage_rounded,
        title: volumes.isEmpty ? l.dockerNoVolumes : l.dockerNoMatches,
      );
    }
    final groups = <String?, List<DockerVolume>>{};
    for (final volume in visible) {
      groups.putIfAbsent(volume.composeProject, () => []).add(volume);
    }
    final keys = groups.keys.toList()..sort((a, b) => a == null ? 1 : (b == null ? -1 : a.compareTo(b)));
    final mono = tokens.typography.mono.copyWith(fontSize: 12, color: tokens.secondaryLabel);
    return ListView(
      key: const ValueKey('docker-volumes'),
      padding: const EdgeInsets.only(bottom: GlassSpacing.s12),
      children: [
        for (final key in keys)
          Padding(
            padding: const EdgeInsets.only(bottom: GlassSpacing.s8),
            child: DecoratedBox(
              key: ValueKey('docker-volume-block-${key ?? ''}'),
              decoration: ShapeDecoration(
                color: tokens.surfaces.fillField,
                shape: RoundedRectangleBorder(
                  borderRadius: BorderRadius.circular(tokens.radii.card),
                  side: BorderSide(color: tokens.surfaces.hairlineCard),
                ),
              ),
              child: Padding(
                padding: const EdgeInsets.all(GlassSpacing.s8),
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.stretch,
                  children: [
                    Padding(
                      padding: const EdgeInsets.fromLTRB(4, 2, 4, 6),
                      child: Row(
                        children: [
                          Icon(
                            key == null ? Icons.storage_rounded : Icons.layers_outlined,
                            size: 16,
                            color: tokens.secondaryLabel,
                          ),
                          const SizedBox(width: 8),
                          Expanded(
                            child: Text(
                              key ?? l.dockerVolumesOther,
                              style: tokens.typography.bodyEmph.copyWith(color: tokens.palette.label),
                            ),
                          ),
                          Text(
                            l.dockerVolumeCount(groups[key]!.length),
                            style: tokens.typography.caption.copyWith(color: tokens.secondaryLabel),
                          ),
                        ],
                      ),
                    ),
                    for (final volume in groups[key]!)
                      Opacity(
                        opacity: volume.usedBy.isEmpty ? .55 : 1,
                        child: Padding(
                          key: ValueKey('docker-volume-${volume.name}'),
                          padding: const EdgeInsets.symmetric(horizontal: 4, vertical: 6),
                          child: Column(
                            crossAxisAlignment: CrossAxisAlignment.start,
                            children: [
                              SelectableText(
                                volume.name,
                                style: tokens.typography.bodyEmph.copyWith(color: tokens.palette.label),
                              ),
                              const SizedBox(height: 2),
                              SelectableText('${volume.driver} · ${volume.mountpoint}', style: mono),
                              const SizedBox(height: 4),
                              Wrap(
                                spacing: 6,
                                runSpacing: 4,
                                children: [
                                  if (volume.usedBy.isEmpty)
                                    GlassBadge(label: l.dockerVolumeUnused, tone: GlassTone.neutral, dense: true)
                                  else
                                    for (final name in volume.usedBy)
                                      GlassBadge(
                                        label: name,
                                        tone: GlassTone.info,
                                        icon: Icons.view_in_ar_outlined,
                                        dense: true,
                                      ),
                                ],
                              ),
                            ],
                          ),
                        ),
                      ),
                  ],
                ),
              ),
            ),
          ),
      ],
    );
  }
}
