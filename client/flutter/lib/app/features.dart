/// Internal builds may exercise the complete Kubernetes integration before
/// its public release. Production packaging leaves this opt-in unset.
const kubernetesEnabled = bool.fromEnvironment('CC_KUBERNETES');
