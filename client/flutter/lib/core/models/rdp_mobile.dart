enum RdpMobileMode { automatic, touchpad, direct }

enum RdpMobileOrientation { automatic, landscape, portrait }

/// Non-secret device preferences. Host keys are opaque hashes; this state is
/// deliberately outside the encrypted/synced host documents.
class RdpMobileSettings {
  const RdpMobileSettings({
    this.mode = RdpMobileMode.automatic,
    this.speed = 1,
    this.acceleration = true,
    this.cursorSize = 24,
    this.autoResize = true,
    this.clarity = 1.5,
    this.windowsScale = 125,
    this.autoHide = true,
    this.orientation = RdpMobileOrientation.automatic,
    this.hideIntroduction = false,
    this.hostModes = const {},
    this.portraitButton = const [1, .65],
    this.landscapeButton = const [1, .65],
  });

  final RdpMobileMode mode;
  final double speed, cursorSize, clarity;
  final bool acceleration, autoResize, autoHide, hideIntroduction;
  final int windowsScale;
  final RdpMobileOrientation orientation;
  final Map<String, bool> hostModes;
  final List<double> portraitButton, landscapeButton;

  bool touchpad({required bool phone, bool accessible = false, String? host}) {
    if (accessible) return false;
    return hostModes[host] ??
        switch (mode) {
          RdpMobileMode.automatic => phone,
          RdpMobileMode.touchpad => true,
          RdpMobileMode.direct => false,
        };
  }

  factory RdpMobileSettings.fromJson(Object? value) {
    if (value is! Map) return const RdpMobileSettings();
    double number(String key, double fallback, double min, double max) {
      final n = value[key];
      return n is num && n.isFinite ? n.toDouble().clamp(min, max) : fallback;
    }

    List<double> position(String key) {
      final p = value[key];
      if (p is! List || p.length != 2) return const [1, .65];
      return [for (final n in p) n is num && n.isFinite ? n.toDouble().clamp(0, 1) : .65];
    }

    final modes = value['hosts'];
    return RdpMobileSettings(
      mode: RdpMobileMode.values.where((e) => e.name == value['mode']).firstOrNull ?? RdpMobileMode.automatic,
      speed: number('speed', 1, .5, 3),
      cursorSize: number('cursor', 24, 20, 36),
      clarity: number('clarity', 1.5, 1, 2),
      windowsScale: [100, 125, 150].contains(value['windows_scale']) ? (value['windows_scale'] as num).toInt() : 125,
      acceleration: value['acceleration'] != false,
      autoResize: value['auto_resize'] != false,
      autoHide: value['auto_hide'] != false,
      hideIntroduction: value['hide_introduction'] == true,
      orientation:
          RdpMobileOrientation.values.where((e) => e.name == value['orientation']).firstOrNull ??
          RdpMobileOrientation.automatic,
      hostModes: modes is Map
          ? Map.unmodifiable({
              for (final entry in modes.entries.take(512))
                if (entry.key is String &&
                    RegExp(r'^[a-f0-9]{64}$').hasMatch(entry.key as String) &&
                    entry.value is bool)
                  entry.key as String: entry.value as bool,
            })
          : const {},
      portraitButton: position('portrait_button'),
      landscapeButton: position('landscape_button'),
    );
  }

  Map<String, Object?> toJson() => {
    'mode': mode.name,
    'speed': speed,
    'acceleration': acceleration,
    'cursor': cursorSize,
    'auto_resize': autoResize,
    'clarity': clarity,
    'windows_scale': windowsScale,
    'auto_hide': autoHide,
    'orientation': orientation.name,
    'hide_introduction': hideIntroduction,
    'hosts': hostModes,
    'portrait_button': portraitButton,
    'landscape_button': landscapeButton,
  };

  RdpMobileSettings copyWith({
    RdpMobileMode? mode,
    double? speed,
    bool? acceleration,
    double? cursorSize,
    bool? autoResize,
    double? clarity,
    int? windowsScale,
    bool? autoHide,
    RdpMobileOrientation? orientation,
    bool? hideIntroduction,
    Map<String, bool>? hostModes,
    List<double>? portraitButton,
    List<double>? landscapeButton,
  }) => RdpMobileSettings.fromJson({
    ...toJson(),
    if (mode != null) 'mode': mode.name,
    'speed': ?speed,
    'acceleration': ?acceleration,
    'cursor': ?cursorSize,
    'auto_resize': ?autoResize,
    'clarity': ?clarity,
    'windows_scale': ?windowsScale,
    'auto_hide': ?autoHide,
    if (orientation != null) 'orientation': orientation.name,
    'hide_introduction': ?hideIntroduction,
    'hosts': ?hostModes,
    'portrait_button': ?portraitButton,
    'landscape_button': ?landscapeButton,
  });
}
