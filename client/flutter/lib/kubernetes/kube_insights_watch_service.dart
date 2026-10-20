/// One coalesced native subscription covers the served GitOps resource kinds.
/// It carries invalidation signals only; object bodies come from the bounded
/// and sanitized insights reader.
abstract interface class KubeInsightsWatchService {
  Stream<KubeInsightsInvalidation> watchInsights(String sessionId, {String? namespace});
}

final class KubeInsightsInvalidation {
  const KubeInsightsInvalidation({this.partial = false, this.closed = false});
  final bool partial, closed;
}
