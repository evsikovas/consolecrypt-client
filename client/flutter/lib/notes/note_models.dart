/// Mirrors `cc_models::note` through `NoteDto` (synced E2EE vault objects).
library;

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
    if (json is! NoteJson) return null;
    switch (json['kind']) {
      case 'object':
        final kind = NoteObjectKind.parse(json['object_kind']);
        final id = json['id'];
        return kind == null || id is! String ? null : NoteObjectTarget(kind, id);
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
    return null;
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
  }) => Note(
    id: id,
    title: title ?? this.title,
    body: body ?? this.body,
    tags: tags ?? this.tags,
    pinned: pinned ?? this.pinned,
    task: task == null ? this.task : task(),
    remindAt: remindAt == null ? this.remindAt : remindAt(),
    target: target == null ? this.target : target(),
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
