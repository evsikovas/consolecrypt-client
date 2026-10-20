import 'package:consolecrypt/app/platform.dart';
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
          DropdownButton<RdpMobileMode>(
            value: s.mode,
            isExpanded: true,
            items: [
              DropdownMenuItem(value: RdpMobileMode.automatic, child: Text(l.rdpMobileAutoDefault)),
              DropdownMenuItem(value: RdpMobileMode.touchpad, child: Text(l.rdpMobileTouchpad)),
              DropdownMenuItem(value: RdpMobileMode.direct, child: Text(l.rdpMobileDirectTouch)),
            ],
            onChanged: (mode) {
              if (mode != null) save(s.copyWith(mode: mode));
            },
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
          DropdownButton<int>(
            value: s.windowsScale,
            isExpanded: true,
            items: [
              for (final scale in [100, 125, 150]) DropdownMenuItem(value: scale, child: Text('$scale %')),
            ],
            onChanged: (scale) {
              if (scale != null) save(s.copyWith(windowsScale: scale));
            },
          ),
          SwitchListTile(
            title: Text(l.rdpMobileAutoHide),
            value: s.autoHide,
            onChanged: (v) => save(s.copyWith(autoHide: v)),
          ),
          Text(l.rdpMobileOrientation),
          DropdownButton<RdpMobileOrientation>(
            value: s.orientation,
            isExpanded: true,
            items: [
              DropdownMenuItem(value: RdpMobileOrientation.automatic, child: Text(l.rdpMobileAutomatic)),
              DropdownMenuItem(value: RdpMobileOrientation.landscape, child: Text(l.rdpMobileLandscape)),
              DropdownMenuItem(value: RdpMobileOrientation.portrait, child: Text(l.rdpMobilePortrait)),
            ],
            onChanged: (o) {
              if (o != null) save(s.copyWith(orientation: o));
            },
          ),
        ],
      ),
    );
  }
}
