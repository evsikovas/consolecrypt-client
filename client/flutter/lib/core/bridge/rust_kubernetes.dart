import 'dart:convert';

import 'package:consolecrypt/core/bridge/mapping.dart';
import 'package:consolecrypt/core/bridge/rust_account.dart';
import 'package:consolecrypt/core/bridge/rust_backend.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/kubernetes/kube_service.dart';
import 'package:consolecrypt/src/rust/api/kubernetes.dart' as native;

final class RustKubeService implements KubeService {
  const RustKubeService(this._hub);
  final RustBackend _hub;

  @override
  Stream<void> changes() => Stream<void>.multi((controller) {
    void changed(Json event) {
      if (const {
        'objects_changed',
        'conflict_resolved',
        'lagged',
        'vault_unlocked',
        'vault_locked',
        'profiles_changed',
      }.contains(event['type'])) {
        controller.add(null);
      }
    }

    _hub.eventListeners.add(changed);
    controller.add(null);
    controller.onCancel = () => _hub.eventListeners.remove(changed);
  }, isBroadcast: true);

  @override
  Future<List<KubeConnection>> inventory() async {
    final data = decodeObject(await guard(native.kubeInventory));
    final clusters = {for (final c in (data['clusters'] as List).cast<Json>()) c['id'] as String: c};
    return List.unmodifiable(
      (data['contexts'] as List).cast<Json>().map(
        (context) => KubeConnection.fromJson(context, clusters[context['cluster_id']]),
      ),
    );
  }

  @override
  Future<KubeImportPreview> preview(SecretText yaml, {String? sourceFile}) async => KubeImportPreview.fromJson(
    decodeObject(
      await withSecret(yaml, (bytes) => guard(() => native.kubeImportPreview(yaml: bytes, sourceFile: sourceFile))),
    ),
  );

  @override
  Future<KubeImportPreview> previewFile(String path) async =>
      KubeImportPreview.fromJson(decodeObject(await guard(() => native.kubeImportPreviewFile(path: path))));

  @override
  Future<void> discard(String ticket) => guard(() => native.kubeImportDiscard(ticket: ticket));

  @override
  Future<void> commit(String ticket, List<KubeImportChoice> choices) async {
    await guard(
      () => native.kubeImportCommit(ticket: ticket, choicesJson: jsonEncode(choices.map((c) => c.toJson()).toList())),
    );
  }

  @override
  Future<void> deleteContext(String id) => guard(() => native.kubeContextDelete(id: id));
}
