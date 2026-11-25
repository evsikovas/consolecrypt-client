/// Device-local dashboard selection. Only inventory IDs are persisted.
final class MonitoringPreferences {
  const MonitoringPreferences({this.hosts = const {}, this.groups = const {}, this.seconds = 30, this.percent = false});

  final Set<String> hosts, groups;
  static const intervals = [0, 15, 30, 60, 300];
  final int seconds;
  final bool percent;

  MonitoringPreferences copyWith({Set<String>? hosts, Set<String>? groups, int? seconds, bool? percent}) =>
      MonitoringPreferences(
        hosts: hosts ?? this.hosts,
        groups: groups ?? this.groups,
        seconds: seconds ?? this.seconds,
        percent: percent ?? this.percent,
      );

  Map<String, Object?> toJson() => {
    'hosts': hosts.toList(),
    'groups': groups.toList(),
    'seconds': seconds,
    'percent': percent,
  };

  static Map<String, MonitoringPreferences> profilesFromJson(Object? value) {
    Set<String> ids(Object? raw) => raw is List ? raw.whereType<String>().toSet() : {};
    return {
      if (value is Map)
        for (final entry in value.entries)
          if (entry.key case final String profile)
            if (entry.value case final Map<Object?, Object?> fields)
              profile: MonitoringPreferences(
                hosts: ids(fields['hosts']),
                groups: ids(fields['groups']),
                seconds: intervals.contains(fields['seconds']) ? fields['seconds'] as int : 30,
                percent: fields['percent'] == true,
              ),
    };
  }
}
