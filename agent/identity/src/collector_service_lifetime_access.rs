// Include ONLY inside the existing collector_service::access module.
// This is original authenticated metadata, not a serialized/reconstructed guard.
impl<S: CompareAndSwap> CollectorServiceAccess<'_, S> {
    pub(crate) fn credential_original_clock(&self) -> &CollectorServiceClockStamp {
        &self.original_clock
    }
}
