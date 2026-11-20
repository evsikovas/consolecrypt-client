/// Device-local, per-profile sidebar choices. IDs are protocol names, not labels
/// or router indices. Unknown IDs survive updates but never create routes.
final class NavigationPreferences {
  NavigationPreferences({this.schemaVersion = 1, required Map<String, bool> visibility, List<String> order = const []})
    : visibility = Map.unmodifiable(visibility),
      order = List.unmodifiable(order.toSet());

  final int schemaVersion;
  final Map<String, bool> visibility;
  final List<String> order;
  bool get supported => schemaVersion == 1;

  bool visible(String id, {bool defaultVisible = true}) => supported ? visibility[id] ?? false : defaultVisible;

  NavigationPreferences withVisibility(String id, bool value) {
    if (!supported) throw StateError('Unsupported navigation preferences version');
    return NavigationPreferences(visibility: {...visibility, id: value}, order: order);
  }

  List<T> ordered<T>(Iterable<T> items, String Function(T) id) {
    final remaining = {for (final item in items) id(item): item};
    return [
      if (supported)
        for (final key in order)
          if (remaining.remove(key) case final item?) item,
      ...remaining.values,
    ];
  }

  Map<String, Object?> toJson() => {
    'version': schemaVersion,
    'visibility': visibility,
    if (order.isNotEmpty) 'order': order,
  };

  static NavigationPreferences? fromJson(Object? value) {
    if (value is! Map || value['version'] is! int || value['visibility'] is! Map) return null;
    final version = value['version'] as int;
    if (version < 1) return null;
    final entries = value['visibility'] as Map;
    final order = value['order'];
    if (entries.length > 256) return null;
    return NavigationPreferences(
      schemaVersion: version,
      order: order is List && order.length <= 256
          ? [
              for (final id in order)
                if (id is String && id.length <= 80) id,
            ]
          : const [],
      visibility: {
        for (final entry in entries.entries)
          if (entry.key is String && (entry.key as String).length <= 80 && entry.value is bool)
            entry.key as String: entry.value as bool,
      },
    );
  }

  static Map<String, NavigationPreferences> profilesFromJson(Object? value) => {
    if (value is Map)
      for (final entry in value.entries)
        if (entry.key is String && (entry.key as String).length <= 128)
          if (fromJson(entry.value) case final NavigationPreferences preferences) entry.key as String: preferences,
  };
}
