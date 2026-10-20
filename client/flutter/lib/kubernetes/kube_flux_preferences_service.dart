import 'package:consolecrypt/kubernetes/kube_service.dart';

abstract interface class KubeFluxPreferencesService {
  Future<FluxPreferencesView> fluxPreferences({String? sessionId});
  Future<void> saveFluxPreferences(FluxPreferences preferences);
  Future<FluxPathAdvice> addFluxUserPath();
}

final class FluxPreferences {
  FluxPreferences({this.manualPath, this.method, this.dismissAdvice = false, Map<String, String>? diffDirectories})
    : diffDirectories = diffDirectories ?? {};
  String? manualPath, method;
  bool dismissAdvice;
  final Map<String, String> diffDirectories;
  factory FluxPreferences.fromJson(KubeJson json) => FluxPreferences(
    manualPath: json['manual_path'] as String?,
    method: json['install_method'] as String?,
    dismissAdvice: json['dismiss_install_advice'] as bool? ?? false,
    diffDirectories: (json['diff_directories'] as Map? ?? {}).cast<String, String>(),
  );
  KubeJson toJson() => {
    'manual_path': manualPath,
    'install_method': method,
    'dismiss_install_advice': dismissAdvice,
    'diff_directories': diffDirectories,
  };
}

final class FluxPreferencesView {
  const FluxPreferencesView({required this.preferences, required this.path, this.controllerVersion});
  final FluxPreferences preferences;
  final FluxPathAdvice path;
  final String? controllerVersion;
  factory FluxPreferencesView.fromJson(KubeJson json) => FluxPreferencesView(
    preferences: FluxPreferences.fromJson(json['preferences'] as KubeJson),
    path: FluxPathAdvice.fromJson(json['path'] as KubeJson),
    controllerVersion: json['controller_version'] as String?,
  );
}

final class FluxPathAdvice {
  const FluxPathAdvice({
    required this.directory,
    required this.present,
    required this.canAdd,
    this.profileFile,
    this.line,
  });
  final String directory;
  final bool present, canAdd;
  final String? profileFile, line;
  factory FluxPathAdvice.fromJson(KubeJson json) => FluxPathAdvice(
    directory: json['directory'] as String,
    present: json['present'] as bool,
    canAdd: json['can_add'] as bool,
    profileFile: json['profile_file'] as String?,
    line: json['line'] as String?,
  );
}
