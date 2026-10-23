/// Mirrors `cc_models::note` through `NoteDto` (synced E2EE vault objects).
library;

import 'dart:convert';

typedef NoteJson = Map<String, dynamic>;

/// Inventory objects a note can be attached to (wire names of the core).
enum NoteObjectKind {
  host('host'),
  rdpHost('rdp_host'),
  dbConnection('db_connection'),
  kubeContext('kube_context');

  const NoteObjectKind(this.wireName);
  final String wireName;
  static NoteObjectKind? parse(Object? value) => values.where((e) => e.wireName == value).firstOrNull;
}

/// What a note is about. Equality is structural so the panel can match notes
/// to the object that is open right now.
sealed class NoteTarget {
  const NoteTarget();

  static NoteTarget? fromJson(Object? json) {
    if (json is! NoteJson || json['kind'] is! String) return null;
    switch (json['kind']) {
      case 'object':
        final kind = NoteObjectKind.parse(json['object_kind']);
        final id = json['id'];
        if (kind != null && id is String) return NoteObjectTarget(kind, id);
      case 'kube_resource':
        return NoteKubeTarget(
          contextId: json['context_id'] as String,
          group: json['group'] as String? ?? '',
          resource: json['resource'] as String,
          kind: json['object_kind'] as String,
          namespace: json['namespace'] as String?,
          name: json['name'] as String,
        );
      case 'docker_container':
        return NoteDockerTarget(hostId: json['host_id'] as String?, name: json['name'] as String);
    }
    // A later version's target: kept as is, shown as not linked.
    return NoteUnknownTarget(json);
  }

  NoteJson toJson();
}

final class NoteObjectTarget extends NoteTarget {
  const NoteObjectTarget(this.kind, this.id);
  final NoteObjectKind kind;
  final String id;

  @override
  NoteJson toJson() => {'kind': 'object', 'object_kind': kind.wireName, 'id': id};
  @override
  bool operator ==(Object other) => other is NoteObjectTarget && other.kind == kind && other.id == id;
  @override
  int get hashCode => Object.hash(kind, id);
}

/// `Kind namespace/name`: Kubernetes identifiers, not prose.
String noteKubeLabel(String kind, String? namespace, String name) =>
    '$kind ${namespace == null ? name : '$namespace/$name'}'; // l10n-ignore: Kubernetes identifiers

/// A Kubernetes object, without its API version.
final class NoteKubeTarget extends NoteTarget {
  const NoteKubeTarget({
    required this.contextId,
    required this.resource,
    required this.kind,
    required this.name,
    this.group = '',
    this.namespace,
  });
  final String contextId, group, resource, kind, name;
  final String? namespace;

  @override
  NoteJson toJson() => {
    'kind': 'kube_resource',
    'context_id': contextId,
    'group': group,
    'resource': resource,
    'object_kind': kind,
    'namespace': namespace,
    'name': name,
  };
  @override
  bool operator ==(Object other) =>
      other is NoteKubeTarget &&
      other.contextId == contextId &&
      other.group == group &&
      other.resource == resource &&
      other.namespace == namespace &&
      other.name == name;
  @override
  int get hashCode => Object.hash(contextId, group, resource, namespace, name);
}

/// A target this version does not know (written by a later version). It is
/// written back unchanged so editing the note here keeps the link.
final class NoteUnknownTarget extends NoteTarget {
  NoteUnknownTarget(NoteJson json) : json = Map.unmodifiable(json), _key = jsonEncode(_sorted(json));
  final NoteJson json;
  final String _key;

  static Object? _sorted(Object? value) => switch (value) {
    final Map<dynamic, dynamic> map => {
      for (final key in (map.keys.map((k) => '$k').toList()..sort())) key: _sorted(map[key]),
    },
    final List<dynamic> list => [for (final item in list) _sorted(item)],
    _ => value,
  };

  @override
  NoteJson toJson() => Map.of(json);
  @override
  bool operator ==(Object other) => other is NoteUnknownTarget && other._key == _key;
  @override
  int get hashCode => _key.hashCode;
}

/// A Docker container by name on an SSH host's engine (null: this computer).
final class NoteDockerTarget extends NoteTarget {
  const NoteDockerTarget({required this.name, this.hostId});
  final String? hostId;
  final String name;

  @override
  NoteJson toJson() => {'kind': 'docker_container', 'host_id': hostId, 'name': name};
  @override
  bool operator ==(Object other) => other is NoteDockerTarget && other.hostId == hostId && other.name == name;
  @override
  int get hashCode => Object.hash(hostId, name);
}

final class NoteTask {
  const NoteTask({this.done = false, this.dueAt, this.doneAt});
  final bool done;
  final DateTime? dueAt, doneAt;

  factory NoteTask.fromJson(NoteJson json) => NoteTask(
    done: json['done'] as bool? ?? false,
    dueAt: _time(json['due_at_ms']),
    doneAt: _time(json['done_at_ms']),
  );
  NoteJson toJson() => {
    'done': done,
    'due_at_ms': dueAt?.millisecondsSinceEpoch,
    'done_at_ms': doneAt?.millisecondsSinceEpoch,
  };
}

final class Note {
  const Note({
    required this.title,
    this.id = '',
    this.body = '',
    this.tags = const [],
    this.pinned = false,
    this.task,
    this.remindAt,
    this.target,
    this.targetLabel,
    this.createdAt,
    this.updatedAt,
  });

  /// Empty for a note that is not saved yet.
  final String id;
  final String title, body;
  final List<String> tags;
  final bool pinned;
  final NoteTask? task;
  final DateTime? remindAt;
  final NoteTarget? target;

  /// The target's name when it was attached; shown once the object is gone.
  final String? targetLabel;
  final DateTime? createdAt, updatedAt;

  bool get isTask => task != null;

  /// An open task past its due time.
  bool overdue(DateTime now) {
    final due = task?.dueAt;
    return task?.done == false && due != null && due.isBefore(now);
  }

  bool reminderDue(DateTime now) => remindAt != null && !remindAt!.isAfter(now);

  factory Note.fromJson(NoteJson json) => Note(
    id: json['id'] as String,
    title: json['title'] as String,
    body: json['body'] as String? ?? '',
    tags: (json['tags'] as List? ?? const []).cast<String>(),
    pinned: json['pinned'] as bool? ?? false,
    task: json['task'] is NoteJson ? NoteTask.fromJson(json['task'] as NoteJson) : null,
    remindAt: _time(json['remind_at_ms']),
    target: NoteTarget.fromJson(json['target']),
    targetLabel: json['target_label'] as String?,
    createdAt: _time(json['created_at_ms']),
    updatedAt: _time(json['updated_at_ms']),
  );

  NoteJson toJson() => {
    'id': id,
    'title': title,
    'body': body,
    'tags': tags,
    'pinned': pinned,
    'task': task?.toJson(),
    'remind_at_ms': remindAt?.millisecondsSinceEpoch,
    'target': target?.toJson(),
    'target_label': target == null ? null : targetLabel,
    'created_at_ms': createdAt?.millisecondsSinceEpoch ?? 0,
    'updated_at_ms': updatedAt?.millisecondsSinceEpoch ?? 0,
  };

  Note copyWith({
    String? title,
    String? body,
    List<String>? tags,
    bool? pinned,
    NoteTask? Function()? task,
    DateTime? Function()? remindAt,
    NoteTarget? Function()? target,
    String? Function()? targetLabel,
  }) => Note(
    id: id,
    title: title ?? this.title,
    body: body ?? this.body,
    tags: tags ?? this.tags,
    pinned: pinned ?? this.pinned,
    task: task == null ? this.task : task(),
    remindAt: remindAt == null ? this.remindAt : remindAt(),
    target: target == null ? this.target : target(),
    targetLabel: targetLabel == null ? this.targetLabel : targetLabel(),
    createdAt: createdAt,
    updatedAt: updatedAt,
  );
}

DateTime? _time(Object? ms) => ms is int ? DateTime.fromMillisecondsSinceEpoch(ms) : null;

/// Pinned first, then open tasks by due time, then the most recently changed.
int compareNotes(Note a, Note b) {
  if (a.pinned != b.pinned) return a.pinned ? -1 : 1;
  final updated = (b.updatedAt ?? DateTime(0)).compareTo(a.updatedAt ?? DateTime(0));
  return updated != 0 ? updated : a.title.compareTo(b.title);
}

/// Open tasks with the nearest due time first; tasks without a due time last.
int compareTasks(Note a, Note b) {
  final ad = a.task?.dueAt, bd = b.task?.dueAt;
  if (ad != null && bd != null) return ad.compareTo(bd);
  if (ad != null || bd != null) return ad != null ? -1 : 1;
  return compareNotes(a, b);
}

/// A note travelling with a share, decrypted and verified by the core.
final class SharedNote {
  const SharedNote({
    required this.id,
    required this.title,
    required this.authorUserId,
    this.body = '',
    this.tags = const [],
    this.scope,
    this.mine = false,
    this.canDelete = false,
    this.revision = 0,
    this.createdAt,
    this.updatedAt,
  });
  final String id, title, body, authorUserId;
  final List<String> tags;

  /// Narrows the note to a part of the shared object (see [noteScope]).
  final Map<String, dynamic>? scope;
  final bool mine, canDelete;
  final int revision;
  final DateTime? createdAt, updatedAt;

  factory SharedNote.fromJson(NoteJson json) => SharedNote(
    id: json['id'] as String,
    title: json['title'] as String,
    body: json['body'] as String? ?? '',
    tags: (json['tags'] as List? ?? const []).cast<String>(),
    scope: json['scope'] as NoteJson?,
    authorUserId: json['author_user_id'] as String,
    mine: json['mine'] as bool? ?? false,
    canDelete: json['can_delete'] as bool? ?? false,
    revision: json['revision'] as int? ?? 0,
    createdAt: _time(json['created_at_ms']),
    updatedAt: _time(json['updated_at_ms']),
  );
}

/// The notes of one share as this device can read them.
final class SharedNotes {
  const SharedNotes({
    required this.shareId,
    this.enabled = false,
    this.owner = false,
    this.ownerUserId = '',
    this.waitingForOwner = false,
    this.notes = const [],
    this.pending = 0,
  });
  final String shareId, ownerUserId;

  /// The server offers shared notes.
  final bool enabled;
  final bool owner, waitingForOwner;
  final List<SharedNote> notes;

  /// Notes this device can open only after the owner moves them to the current key.
  final int pending;

  factory SharedNotes.fromJson(NoteJson json) => SharedNotes(
    shareId: json['share_id'] as String,
    enabled: json['enabled'] as bool? ?? false,
    owner: json['owner'] as bool? ?? false,
    ownerUserId: json['owner_user_id'] as String? ?? '',
    waitingForOwner: json['waiting_for_owner'] as bool? ?? false,
    notes: [for (final note in (json['notes'] as List? ?? const [])) SharedNote.fromJson(note as NoteJson)],
    pending: json['pending'] as int? ?? 0,
  );
}

/// The local object whose share carries notes about [target]: the object
/// itself, the context of a Kubernetes resource or the SSH host of a Docker
/// engine (`kind:id`, as the core links them). Null on this computer.
String? noteLinkKey(NoteTarget target) => switch (target) {
  NoteObjectTarget(:final kind, :final id) => '${kind.wireName}:$id',
  NoteKubeTarget(:final contextId) => 'kube_context:$contextId',
  NoteDockerTarget(:final hostId?) => 'host:$hostId',
  NoteDockerTarget() || NoteUnknownTarget() => null,
};

/// The part of the shared object a note is about. It names no local ids,
/// so it matches on every member's copy.
NoteJson? noteScope(NoteTarget target) => switch (target) {
  NoteObjectTarget() || NoteUnknownTarget() => null,
  NoteKubeTarget(:final group, :final resource, :final namespace, :final name) => {
    'kube_resource': {'group': group, 'resource': resource, 'namespace': namespace, 'name': name},
  },
  NoteDockerTarget(:final name) => {'docker_container': name},
};

/// Structural equality of two scopes (JSON maps).
bool sameNoteScope(Object? a, Object? b) {
  if (a is Map && b is Map) {
    return a.length == b.length && a.keys.every((key) => b.containsKey(key) && sameNoteScope(a[key], b[key]));
  }
  return a == b;
}
