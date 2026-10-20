import 'package:consolecrypt/kubernetes/kube_browser_service.dart';

final class KubeLogPin {
  const KubeLogPin({required this.resource, required this.containers});
  final KubeResourceRef resource;
  final List<String> containers;
  String get id => resource.id;
}

final class KubePanelOutput {
  const KubePanelOutput({required this.id, required this.title, required this.text});
  final String id, title, text;
}
