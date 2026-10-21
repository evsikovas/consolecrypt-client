import 'package:consolecrypt/app/platform.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/rdp_mobile.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

class RdpMobileSettingsSection extends ConsumerWidget {
  const RdpMobileSettingsSection({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    if (!AppPlatform.isMobile) return const SizedBox.shrink();
    final service = ref.read(settingsServiceProvider);
    final s = ref.watch(localSettingsProvider).value?.rdpMobile ?? service.currentLocal.rdpMobile;
    final l = context.l10n;
    void save(RdpMobileSettings next) {
      runWithFeedback(context, () => service.updateLocal(service.currentLocal.copyWith(rdpMobile: next)));
    }

    Widget slider(String label, double value, double min, double max, int divisions, ValueChanged<double> change) =>
        Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text('$label · ${value.toStringAsFixed(value == value.roundToDouble() ? 0 : 1)}'),
            Slider(value: value, min: min, max: max, divisions: divisions, onChanged: change),
          ],
        );
    return SectionCard(
      key: const ValueKey('settings-rdp-mobile'),
      title: l.rdpMobileSettingsTitle,
      subtitle: l.rdpMobileSettingsLocal,
      icon: Icons.mouse_outlined,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Text(l.rdpMobileDefaultMode),
          GlassSelect<RdpMobileMode>(
            value: s.mode,
            expand: true,
            semanticLabel: l.rdpMobileDefaultMode,
            items: [
              GlassSelectItem(value: RdpMobileMode.automatic, label: l.rdpMobileAutoDefault),
              GlassSelectItem(value: RdpMobileMode.touchpad, label: l.rdpMobileTouchpad),
              GlassSelectItem(value: RdpMobileMode.direct, label: l.rdpMobileDirectTouch),
            ],
            onChanged: (mode) => save(s.copyWith(mode: mode)),
          ),
          slider(l.rdpMobileSpeed, s.speed, .5, 3, 25, (v) => save(s.copyWith(speed: v))),
          SwitchListTile(
            title: Text(l.rdpMobileAcceleration),
            value: s.acceleration,
            onChanged: (v) => save(s.copyWith(acceleration: v)),
          ),
          slider(l.rdpMobileCursorSize, s.cursorSize, 20, 36, 16, (v) => save(s.copyWith(cursorSize: v))),
          SwitchListTile(
            title: Text(l.rdpMobileAutoResize),
            value: s.autoResize,
            onChanged: (v) => save(s.copyWith(autoResize: v)),
          ),
          slider(l.rdpMobileClarity, s.clarity, 1, 2, 10, (v) => save(s.copyWith(clarity: v))),
          Text(l.rdpMobileWindowsScale),
          GlassSelect<int>(
            value: s.windowsScale,
            expand: true,
            semanticLabel: l.rdpMobileWindowsScale,
            items: [
              for (final scale in [100, 125, 150]) GlassSelectItem(value: scale, label: '$scale %'),
            ],
            onChanged: (scale) => save(s.copyWith(windowsScale: scale)),
          ),
          SwitchListTile(
            title: Text(l.rdpMobileAutoHide),
            value: s.autoHide,
            onChanged: (v) => save(s.copyWith(autoHide: v)),
          ),
          Text(l.rdpMobileOrientation),
          GlassSelect<RdpMobileOrientation>(
            value: s.orientation,
            expand: true,
            semanticLabel: l.rdpMobileOrientation,
            items: [
              GlassSelectItem(value: RdpMobileOrientation.automatic, label: l.rdpMobileAutomatic),
              GlassSelectItem(value: RdpMobileOrientation.landscape, label: l.rdpMobileLandscape),
              GlassSelectItem(value: RdpMobileOrientation.portrait, label: l.rdpMobilePortrait),
            ],
            onChanged: (o) => save(s.copyWith(orientation: o)),
          ),
        ],
      ),
    );
  }
}
