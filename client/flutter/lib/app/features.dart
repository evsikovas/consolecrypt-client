/// Kubernetes is available in release builds. Diagnostics may explicitly
/// disable it when comparing an earlier navigation layout.
const kubernetesEnabled = bool.fromEnvironment(
  'CC_KUBERNETES',
  defaultValue: true,
);
