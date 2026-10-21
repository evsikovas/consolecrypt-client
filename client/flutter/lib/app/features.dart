/// Kubernetes is available in release builds. Diagnostics may explicitly
/// disable it when comparing an earlier navigation layout.
const kubernetesEnabled = bool.fromEnvironment('CC_KUBERNETES', defaultValue: true);

/// Databases are in development (0.7) and stay behind a build flag.
const databasesEnabled = bool.fromEnvironment('CC_DATABASES', defaultValue: true);

/// Docker containers on this computer or a saved SSH host.
const dockerEnabled = bool.fromEnvironment('CC_DOCKER', defaultValue: true);
