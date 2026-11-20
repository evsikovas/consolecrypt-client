import 'package:consolecrypt/core/bridge/mapping.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/models/navigation.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  test('profile choices round-trip independently through the actual device settings codec', () {
    final original = LocalSettings(
      navigationByProfile: {
        'profile-a': NavigationPreferences(visibility: {'hosts': true, 'docker': false, 'future-section': false}),
        'profile-b': NavigationPreferences(visibility: {'docker': true}),
      },
    );
    final restored = localSettingsFromJson(decodeObject(encodeJson(localSettingsToJson(original))));
    expect(restored.navigationByProfile['profile-a']!.visible('docker'), isFalse);
    expect(restored.navigationByProfile['profile-b']!.visible('docker'), isTrue);
    expect(restored.navigationByProfile['profile-a']!.visibility, contains('future-section'));
    expect(restored.copyWith(terminalFontSize: 16).navigationByProfile, same(restored.navigationByProfile));
    expect(localSettingsToJson(restored), localSettingsToJson(original));
    expect(localSettingsFromJson(<String, Object?>{}).navigationByProfile, isEmpty);
  });

  test('new sections are not pinned in existing explicit choices; unknown IDs cannot alter defaults', () {
    final preferences = NavigationPreferences(visibility: {'hosts': true, '/arbitrary/code': true});
    expect(preferences.visible('automation'), isFalse);
    final changed = preferences.withVisibility('hosts', false);
    expect(preferences.visible('hosts'), isTrue);
    expect(changed.visible('hosts'), isFalse);
    expect(() => changed.visibility['hosts'] = true, throwsUnsupportedError);
  });

  test('malformed records fall back independently; future schema is retained without editing', () {
    final restored = NavigationPreferences.profilesFromJson({
      'valid': {
        'version': 1,
        'visibility': {'hosts': true, 'docker': 'no'},
      },
      'bad': {'version': 'one', 'visibility': <String, bool>{}},
      'future': {
        'version': 2,
        'visibility': {'unknown-v2': false},
      },
    });
    expect(restored.keys, unorderedEquals(['valid', 'future']));
    expect(restored['valid']!.visibility, {'hosts': true});
    expect(restored['future']!.supported, isFalse);
    expect(restored['future']!.visible('hosts'), isTrue);
    expect(restored['future']!.toJson()['version'], 2);
    expect(() => restored['future']!.withVisibility('hosts', false), throwsStateError);
  });
}
