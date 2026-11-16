// Included only inside node_prepare::linux. Existing selected epoch2 custody,
// new closed statement type; no raw-message or generic key signing interface.
pub(crate) fn sign_selected_collector_recipient(
    guard: &Guard, active: &PublicNodeIdentity, body: CollectorRecipientAttestation,
) -> Result<Signed<CollectorRecipientAttestation>> {
    let selected = crate::node_activation::selected(guard, active)?.ok_or(Error::WrongState)?;
    if body.agent_id != active.agent_id || body.key_epoch != 2 { return Err(Error::WrongClaim); }
    let material = read(&guard.directory.join(MATERIAL), MATERIAL_BYTES)?;
    if pending(&material, active)? != selected.prepared { return Err(Error::WrongClaim); }
    let mut seed = Zeroizing::new([0u8; 32]);
    seed.copy_from_slice(&material[116..148]);
    let key = SigningKey::from_bytes(&seed);
    let signature = SignatureBytes::from_bytes(key.sign(&body.signing_bytes()?).to_bytes());
    guard_inode(guard)?;
    Ok(Signed { body, signature })
}
