import 'package:consolecrypt/core/bridge/mapping.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/models/rdp_mobile.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  test('device-local preferences round trip without changing local AI preferences', () {
    final host = 'a' * 64;
    final settings = const LocalSettings(preferDeviceLocalAi: false).copyWith(
      rdpMobile: RdpMobileSettings(
        speed: 1.7,
        acceleration: false,
        hostModes: {host: true},
        windowsScale: 150,
        portraitButton: const [0, .2],
        landscapeButton: const [1, .8],
        hideIntroduction: true,
      ),
    );
    final back = localSettingsFromJson(decodeObject(encodeJson(localSettingsToJson(settings))));
    expect(back.rdpMobile.toJson(), settings.rdpMobile.toJson());
    expect(back.preferDeviceLocalAi, isFalse);
    expect(localSettingsFromJson({}).rdpMobile.toJson(), const RdpMobileSettings().toJson());
  });
  test('automatic defaults adapt to phone, tablet, accessibility and per-host choice', () {
    final host = 'b' * 64;
    const settings = RdpMobileSettings();
    expect(settings.touchpad(phone: true), isTrue);
    expect(settings.touchpad(phone: false), isFalse);
    expect(settings.touchpad(phone: true, accessible: true), isFalse);
    final saved = settings.copyWith(hostModes: {host: false});
    expect(saved.touchpad(phone: true, host: host), isFalse);
    expect(saved.touchpad(phone: true, host: 'c' * 64), isTrue);
  });
  test('malformed stored fields are bounded and do not expose host names', () {
    final settings = RdpMobileSettings.fromJson({
      'speed': double.nan,
      'clarity': 900,
      'cursor': -5,
      'windows_scale': 150.0,
      'portrait_button': [-20, 42],
      'landscape_button': 'bad',
      'hosts': {'host.example': true, 'd' * 64: false},
      'mode': 'unknown',
    });
    expect(settings.speed, 1);
    expect(settings.clarity, 2);
    expect(settings.cursorSize, 20);
    expect(settings.windowsScale, 150);
    expect(settings.portraitButton, [0, 1]);
    expect(settings.landscapeButton, [1, .65]);
    expect(settings.hostModes, {'d' * 64: false});
    expect(settings.mode, RdpMobileMode.automatic);
  });
}
