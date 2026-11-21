import 'dart:async';

import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/widgets/common.dart' show InfoBanner, BannerTone;
import 'package:consolecrypt/core/widgets/secret_exposure.dart';
import 'package:consolecrypt/docker/docker_container_details.dart' show showDockerSecretRead;
import 'package:consolecrypt/wslc/app/desk_controller.dart';
import 'package:consolecrypt/wslc/features/cleanup/cleanup_page.dart';
import 'package:consolecrypt/wslc/features/containers/container_details_page.dart';
import 'package:consolecrypt/wslc/features/containers/containers_page.dart';
import 'package:consolecrypt/wslc/features/inventory/inventory_pages.dart';
import 'package:consolecrypt/wslc/features/settings/settings_page.dart';
import 'package:consolecrypt/wslc/features/topology/topology_page.dart';
import 'package:consolecrypt/wslc/src/rust/api/desk.dart';
import 'package:consolecrypt/wslc/wslc_scope.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';


typedef WslcApiFactory = WslcApi Function({required String profileId, required WslcReveal reveal});
final wslcApiFactoryProvider = Provider<WslcApiFactory>((ref) => WslcApi.new);

/// A workspace inside the existing ConsoleCrypt shell. Its theme, app-wide
/// shortcuts, command palette, profile lock and clipboard remain shared.
class WslcWorkspace extends ConsumerStatefulWidget {
  const WslcWorkspace({required this.profileId, required this.onBack, super.key});
  final String profileId;
  final VoidCallback onBack;
  @override
  ConsumerState<WslcWorkspace> createState() => _WslcWorkspaceState();
}

class _WslcWorkspaceState extends ConsumerState<WslcWorkspace> {
  late final WslcApi _api;
  late final DeskController _controller;
  bool _revealed = false;
  bool get _active =>
      mounted &&
      ref.read(activeProfileProvider)?.id.value == widget.profileId &&
      ref.read(appStageProvider) == AppStage.unlocked;
  @override
  void initState() {
    super.initState();
    _api = ref.read(wslcApiFactoryProvider)(
      profileId: widget.profileId,
      reveal: (object, read) async {
        if (!_active) return null;
        final result = await showDockerSecretRead<dynamic>(
          context,
          object: object,
          title: 'Показать данные контейнера',
          help: 'Подтвердите доступ к скрытому окружению WSLc.',
          read: read,
        );
        if (!_active) return null;
        if (result != null) setState(() => _revealed = true);
        return result;
      },
    );
    _controller = DeskController(api: _api);
    unawaited(_controller.start());
  }

  @override
  void dispose() {
    _controller.dispose();
    unawaited(_api.close());
    super.dispose();
  }

  Future<void> _copy(String value) async {
    if (!_active) return;
    await ref.read(secureClipboardProvider).copySecret(value, isCurrent: () => _active);
  }

  @override
  Widget build(BuildContext context) => WslcApiScope(
    api: _api,
    copySecret: _copy,
    child: RevealedSecret(
      visible: _revealed,
      child: ListenableBuilder(
        listenable: _controller,
        builder: (context, _) {
          final c = _controller;
          final page = switch (c.section) {
            DeskSection.containers when c.openedContainerId != null => ContainerDetailsPage(
              key: ValueKey(c.openedContainerId),
              controller: c,
              containerId: c.openedContainerId!,
            ),
            DeskSection.containers => ContainersPage(controller: c),
            DeskSection.images => ImagesPage(controller: c),
            DeskSection.storage => VolumesPage(controller: c),
            DeskSection.networks => NetworksPage(controller: c),
            DeskSection.topology => TopologyPage(controller: c),
            DeskSection.cleanup => CleanupPage(controller: c),
            DeskSection.settings => SettingsPage(controller: c),
          };
          return Padding(
            padding: const EdgeInsets.all(12),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                Wrap(
                  spacing: 6,
                  runSpacing: 6,
                  crossAxisAlignment: WrapCrossAlignment.center,
                  children: [
                    GlassIconButton(
                      key: const ValueKey('wslc-back'),
                      icon: Icons.arrow_back_rounded,
                      tooltip: 'Движки контейнеров',
                      onPressed: widget.onBack,
                    ),
                    const Text('WSLc'),
                    for (final item in const [
                      (DeskSection.containers, 'Контейнеры'),
                      (DeskSection.images, 'Образы'),
                      (DeskSection.storage, 'Тома'),
                      (DeskSection.networks, 'Сети'),
                      (DeskSection.topology, 'Связи'),
                      (DeskSection.cleanup, 'Очистка'),
                      (DeskSection.settings, 'Подключение'),
                    ])
                      GlassButton(
                        key: ValueKey('wslc-section-${item.$1.name}'),
                        label: item.$2,
                        onPressed: () => c.selectSection(item.$1),
                      ),
                  ],
                ),
                if (c.error != null)
                  Padding(
                    padding: const EdgeInsets.only(top: 8),
                    child: InfoBanner(tone: BannerTone.danger, message: c.error!),
                  ),
                const SizedBox(height: 12),
                Expanded(child: page),
              ],
            ),
          );
        },
      ),
    ),
  );
}
