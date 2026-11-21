import 'dart:async';

import 'package:consolecrypt/wslc/app/desk_controller.dart';
import 'package:consolecrypt/wslc/shared/error_text.dart';
import 'package:consolecrypt/wslc/src/rust/api/desk.dart';
import 'package:consolecrypt/wslc/src/rust/models.dart';
import 'package:flutter/foundation.dart';


enum StepStatus { pending, running, done, skipped, failed }

class ComposeStep {
  ComposeStep(this.title);
  final String title;
  StepStatus status = StepStatus.pending;
  String? note;

  /// Live output of a pull or build.
  final log = <String>[];
  bool _progress = false;
}

class _Failure implements Exception {
  const _Failure(this.message);
  final String message;
  @override
  String toString() => message;
}

class _Cancelled implements Exception {
  const _Cancelled();
}

/// `repository[:tag]` as Docker resolves it: Docker Hub prefixes dropped, `latest` implied.
String normalizeImage(String ref) {
  var r = ref;
  for (final prefix in const ['docker.io/library/', 'index.docker.io/library/', 'docker.io/']) {
    if (r.startsWith(prefix)) {
      r = r.substring(prefix.length);
      break;
    }
  }
  final name = r.substring(r.lastIndexOf('/') + 1);
  return name.contains(':') || r.contains('@') ? r : '$r:latest';
}

bool hasImage(List<ImageSummary> images, String ref) {
  final want = normalizeImage(ref);
  return images.any((i) => i.repository != null && normalizeImage('${i.repository}:${i.tag ?? 'latest'}') == want);
}

/// `docker compose up -d` and `down` as visible steps over the existing WSLc calls.
class ComposeRunner extends ChangeNotifier {
  ComposeRunner(this.plan, this.controller);
  final ComposePlan plan;
  final DeskController controller;
  WslcApi get api => controller.api;
  final steps = <ComposeStep>[];
  bool busy = false;
  bool cancelled = false;

  /// Why the last run stopped; null after success.
  String? error;

  /// How long a dependency may take to become healthy or finish.
  static Duration waitLimit = const Duration(minutes: 5);
  static Duration pollEvery = const Duration(seconds: 2);

  void cancel() {
    cancelled = true;
    notifyListeners();
  }

  ComposeStep _add(String title) {
    final step = ComposeStep(title);
    steps.add(step);
    notifyListeners();
    return step;
  }

  void _note(ComposeStep step, String note) {
    step.note = note;
    notifyListeners();
  }

  void _skip(ComposeStep step, String note) {
    step.status = StepStatus.skipped;
    step.note = note;
    notifyListeners();
  }

  Future<void> _run(ComposeStep step, Future<void> Function() body, {String? done}) async {
    if (cancelled) throw const _Cancelled();
    step.status = StepStatus.running;
    notifyListeners();
    try {
      await body();
      if (step.status == StepStatus.running) {
        step.status = StepStatus.done;
        if (done != null) step.note = done;
      }
      notifyListeners();
    } catch (e) {
      step.status = StepStatus.failed;
      step.note = e is _Cancelled ? 'отменено' : errorText(e);
      notifyListeners();
      rethrow;
    }
  }

  Future<bool> up({bool rebuild = false}) => _guard(() => _up(rebuild));

  Future<bool> down({bool volumes = false}) => _guard(() => _down(volumes));

  Future<bool> _guard(Future<void> Function() body) async {
    busy = true;
    cancelled = false;
    error = null;
    steps.clear();
    notifyListeners();
    try {
      await body();
      return true;
    } catch (e) {
      error = e is _Cancelled ? 'Остановлено по запросу' : errorText(e);
      return false;
    } finally {
      busy = false;
      notifyListeners();
      unawaited(controller.refreshSections(inventorySections));
    }
  }

  Future<void> _up(bool rebuild) async {
    if (plan.hostFolders.isNotEmpty) {
      await _run(
        _add('Папки для подключений'),
        () => api.createHostFolders(paths: plan.hostFolders),
        done: plan.hostFolders.join('\n'),
      );
    }
    final networks = {for (final n in await api.listNetworks()) n.name};
    for (final n in plan.networks) {
      final step = _add('Сеть ${n.spec.name}');
      if (networks.contains(n.spec.name)) {
        _skip(step, n.isExternal ? 'внешняя, найдена' : 'уже есть');
      } else if (n.isExternal) {
        await _run(step, () async => throw _Failure('Внешняя сеть ${n.spec.name} не найдена'));
      } else {
        await _run(step, () => api.createNetwork(spec: n.spec), done: 'создана');
      }
    }
    final volumes = {for (final v in await api.listVolumes()) v.name};
    for (final v in plan.volumes) {
      final step = _add('Том ${v.spec.name}');
      if (volumes.contains(v.spec.name)) {
        _skip(step, v.isExternal ? 'внешний, найден' : 'уже есть');
      } else if (v.isExternal) {
        await _run(step, () async => throw _Failure('Внешний том ${v.spec.name} не найден'));
      } else {
        await _run(step, () => api.createVolume(spec: v.spec), done: 'создан');
      }
    }

    final images = await api.listImages();
    final seen = <String>{};
    for (final svc in plan.services) {
      if (!seen.add(svc.image)) continue;
      final build = svc.build;
      if (build != null) {
        final step = _add('Сборка ${svc.image}');
        if (!rebuild && svc.pullPolicy != 'build' && hasImage(images, svc.image)) {
          _skip(step, 'образ уже собран');
          continue;
        }
        await _run(step, () => _job(step, () => api.startBuild(spec: build)), done: 'собран');
      } else {
        final step = _add('Образ ${svc.image}');
        final need = switch (svc.pullPolicy) {
          'always' => true,
          'never' => false,
          _ => !hasImage(images, svc.image),
        };
        if (!need) {
          _skip(step, svc.pullPolicy == 'never' ? 'pull_policy: never' : 'уже загружен');
          continue;
        }
        await _run(step, () => _job(step, () => api.startPull(image: svc.image)), done: 'загружен');
      }
    }

    final existing = {for (final c in await api.composeContainers(project: plan.project)) c.name: c};
    for (final svc in plan.services) {
      final step = _add('${svc.service} → ${svc.containerName}');
      await _run(step, () => _service(step, svc, existing));
    }
    final wanted = {for (final s in plan.services) s.containerName};
    final orphans = [
      for (final c in existing.values)
        if (!wanted.contains(c.name)) c.name,
    ];
    if (orphans.isNotEmpty) {
      _skip(_add('Лишние контейнеры проекта'), '${orphans.join(', ')} — их сервисов нет в файле, оставлены как есть');
    }
  }

  Future<void> _service(ComposeStep step, ComposeService svc, Map<String, ComposeContainer> existing) async {
    for (final d in svc.dependsOn) {
      if (d.condition != 'started') await _wait(step, d);
    }
    final old = existing[svc.containerName];
    if (old != null && old.configHash == svc.configHash) {
      if (old.state == ContainerState.running) {
        _skip(step, 'без изменений, уже запущен');
        return;
      }
      _note(step, 'запуск…');
      await api.startContainer(containerId: old.id);
      step.note = 'без изменений, запущен';
      return;
    }
    if (old != null) {
      _note(step, 'конфигурация изменилась — пересоздание…');
      await api.removeContainer(containerId: old.id, force: true);
    } else if (controller.containers.any((c) => c.name == svc.containerName)) {
      throw _Failure('Имя ${svc.containerName} занято контейнером не из этого проекта');
    }
    _note(step, 'создание…');
    final id = await api.runContainer(spec: svc.spec);
    for (final n in svc.extraNetworks) {
      _note(step, 'подключение к ${n.network}…');
      await api.composeConnect(networkName: n.network, containerId: id, aliases: n.aliases, ip: n.ip);
    }
    _note(step, 'запуск…');
    await api.startContainer(containerId: id);
    step.note = old == null ? 'создан и запущен' : 'пересоздан и запущен';
  }

  /// `service_healthy` and `service_completed_successfully`.
  Future<void> _wait(ComposeStep step, ComposeDependency d) async {
    final name = plan.services.firstWhere((s) => s.service == d.service).containerName;
    final healthy = d.condition == 'healthy';
    _note(step, healthy ? 'ждём, пока ${d.service} станет здоровым…' : 'ждём, пока ${d.service} завершится…');
    final until = DateTime.now().add(waitLimit);
    DateTime? runningWithoutHealth;
    while (true) {
      if (cancelled) throw const _Cancelled();
      final c = (await api.composeContainers(project: plan.project)).where((c) => c.name == name).firstOrNull;
      if (c != null && healthy) {
        if (c.health == 'healthy') return;
        if (c.health == 'unhealthy') {
          throw _Failure('${d.service} нездоров — зависимые сервисы не запущены');
        }
        if (c.state == ContainerState.exited) {
          throw _Failure('${d.service} остановился, не став здоровым');
        }
        if (c.health == null && c.state == ContainerState.running) {
          runningWithoutHealth ??= DateTime.now();
          if (DateTime.now().difference(runningWithoutHealth).inSeconds > 10) {
            throw _Failure('У ${d.service} нет проверки здоровья — условие service_healthy не выполнить');
          }
        }
      } else if (c != null && c.state == ContainerState.exited) {
        if ((c.exitCode ?? 1) == 0) return;
        throw _Failure('${d.service} завершился с кодом ${c.exitCode}');
      }
      if (DateTime.now().isAfter(until)) {
        throw _Failure('${d.service}: не дождались за ${waitLimit.inMinutes} мин');
      }
      await Future<void>.delayed(pollEvery);
    }
  }

  Future<void> _job(ComposeStep step, Future<BigInt> Function() start) async {
    final job = await start();
    try {
      while (true) {
        final batch = await api.takeJob(job: job);
        for (final line in batch.lines) {
          // A progress line is replaced by whatever comes next.
          if (step._progress && step.log.isNotEmpty) step.log.removeLast();
          step.log.add(line.text);
          step._progress = line.progress;
        }
        if (step.log.length > 500) {
          step.log.removeRange(0, step.log.length - 500);
        }
        if (batch.lines.isNotEmpty) notifyListeners();
        if (batch.finished) {
          if (!batch.success) {
            throw _Failure(
              'Завершилось с кодом ${batch.exitCode ?? '?'}'
              '${step.log.isEmpty ? '' : ': ${step.log.last}'}',
            );
          }
          return;
        }
        if (cancelled) {
          await api.cancelJob(job: job);
          throw const _Cancelled();
        }
        await Future<void>.delayed(const Duration(milliseconds: 300));
      }
    } finally {
      unawaited(api.forgetJob(job: job).catchError((Object _) {}));
    }
  }

  Future<void> _down(bool withVolumes) async {
    final containers = await api.composeContainers(project: plan.project);
    if (containers.isEmpty) _skip(_add('Контейнеры проекта'), 'не найдены');
    for (final c in containers.reversed) {
      await _run(
        _add('Удаление ${c.name}'),
        () => api.removeContainer(containerId: c.id, force: true),
        done: 'остановлен и удалён',
      );
    }
    final networks = {for (final n in await api.listNetworks()) n.name};
    for (final n in plan.networks.where((n) => !n.isExternal)) {
      final step = _add('Сеть ${n.spec.name}');
      if (!networks.contains(n.spec.name)) {
        _skip(step, 'уже нет');
        continue;
      }
      await _run(step, () => api.removeNetwork(networkName: n.spec.name), done: 'удалена');
    }
    if (!withVolumes) return;
    final volumes = {for (final v in await api.listVolumes()) v.name};
    for (final v in plan.volumes.where((v) => !v.isExternal)) {
      final step = _add('Том ${v.spec.name}');
      if (!volumes.contains(v.spec.name)) {
        _skip(step, 'уже нет');
        continue;
      }
      await _run(step, () => api.removeVolume(volumeName: v.spec.name), done: 'удалён');
    }
  }
}
