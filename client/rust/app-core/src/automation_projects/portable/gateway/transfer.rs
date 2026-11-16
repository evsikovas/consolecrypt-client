//! A bounded zeroizing VALUE, not a registry or authority. Synchronous creation
//! prevents a normal Vec password being retained in an unpolled async frame.
use super::*;
use zeroize::Zeroizing;

pub struct PortablePasswordTransfer {
    value: Mutex<Option<SecretString>>,
    cancelled: CancellationToken,
    timer_done: CancellationToken,
    handoff: Mutex<Option<Weak<Control>>>,
    deadline: Instant,
}
impl std::fmt::Debug for PortablePasswordTransfer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PortablePasswordTransfer([REDACTED])")
    }
}
impl PortablePasswordTransfer {
    /// Call from the owning runtime context, including a synchronous Bridge
    /// runtime.enter guard. No await occurs before bytes become SecretString.
    pub fn from_bytes(bytes: Vec<u8>) -> AppResult<Arc<Self>> {
        Self::from_bytes_lifetime(bytes, Duration::from_secs(300))
    }
    fn from_bytes_lifetime(bytes: Vec<u8>, ttl: Duration) -> AppResult<Arc<Self>> {
        let bytes = Zeroizing::new(bytes);
        if ttl.is_zero() || ttl > Duration::from_secs(300) {
            return Err(invalid());
        }
        if bytes.is_empty() || bytes.len() > 4096 {
            return Err(invalid());
        }
        // Borrowed UTF-8 validation never creates/retains FromUtf8Error's Vec.
        let text = std::str::from_utf8(&bytes).map_err(|_| invalid())?;
        let value = SecretString::from(text.to_owned());
        let runtime = tokio::runtime::Handle::try_current().map_err(|_| invalid())?;
        let transfer = Arc::new(Self {
            value: Mutex::new(Some(value)),
            cancelled: CancellationToken::new(),
            timer_done: CancellationToken::new(),
            handoff: Mutex::new(None),
            deadline: Instant::now() + ttl,
        });
        let weak = Arc::downgrade(&transfer);
        let terminal = transfer.timer_done.clone();
        let until = transfer.deadline;
        runtime.spawn(async move {
            tokio::select! { biased;
                _ = terminal.cancelled() => {},
                _ = tokio::time::sleep_until(until.into()) => {
                    if let Some(transfer) = weak.upgrade() { transfer.cancel(); }
                }
            }
        });
        Ok(transfer)
    }
    /// Wipes synchronously even if a not-yet-ACKed registration holds an Arc.
    pub fn cancel(&self) {
        self.cancelled.cancel();
        self.wipe_unconsumed();
        let operation = self
            .handoff
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .and_then(Weak::upgrade);
        if let Some(operation) = operation {
            operation.cancel();
        }
    }
    fn wipe_unconsumed(&self) {
        self.timer_done.cancel();
        let value = self.value.lock().unwrap_or_else(|e| e.into_inner()).take();
        drop(value);
    }
    fn attach(&self, operation: &PortableOperation) -> AppResult<()> {
        {
            let mut handoff = self.handoff.lock().unwrap_or_else(|e| e.into_inner());
            if handoff.is_some() {
                return Err(invalid());
            }
            *handoff = Some(Arc::downgrade(&operation.control));
        }
        // Either cancel observed the bound Weak, or this check observes its
        // sticky flag. No cancelled late ACK may leave an idle password alive.
        if self.cancelled.is_cancelled() || operation.control.cancel.is_cancelled() {
            operation.cancel();
            return Err(AppError::Cancelled);
        }
        Ok(())
    }
    fn take(&self) -> AppResult<SecretString> {
        if self.cancelled.is_cancelled() || Instant::now() >= self.deadline {
            self.cancel();
            return Err(invalid());
        }
        let mut value = self.value.lock().unwrap_or_else(|e| e.into_inner());
        if self.cancelled.is_cancelled() || Instant::now() >= self.deadline {
            let rejected = value.take();
            drop(value);
            drop(rejected);
            return Err(invalid());
        }
        let password = value.take().ok_or_else(invalid)?;
        self.timer_done.cancel(); // consumed; native idle operation owns TTL now
        Ok(password)
    }
}
impl Drop for PortablePasswordTransfer {
    fn drop(&mut self) {
        // Successful handoff Drop does not cancel the independent issued owner.
        self.timer_done.cancel();
        drop(
            self.value
                .get_mut()
                .unwrap_or_else(|e| e.into_inner())
                .take(),
        );
    }
}
struct CancelTransfer(Arc<PortablePasswordTransfer>);
impl Drop for CancelTransfer {
    fn drop(&mut self) {
        self.0.wipe_unconsumed();
    }
}

impl AppCore {
    pub async fn automation_portable_register_export_json(
        &self,
        request: String,
        password: Arc<PortablePasswordTransfer>,
        destination: String,
    ) -> AppResult<PortableOperation> {
        let _wipe = CancelTransfer(password.clone());
        let parsed = PortableExportRequest::parse(&request)?;
        let destination = PathBuf::from(destination);
        selected_file(&destination)?;
        // The password stays in its independently cancelable transfer THROUGH
        // all original-owner/unlock awaits. Take + idle insertion are synchronous
        // under those same scope guards. No ready/plain password future queue.
        let transfer = password.clone();
        let operation = self
            .portable_register_build(
                parsed.scope,
                move || {
                    let password = password.take()?;
                    Ok(Body::Export {
                        request,
                        password,
                        destination,
                    })
                },
                Duration::from_secs(300),
            )
            .await?;
        transfer.attach(&operation)?;
        Ok(operation)
    }
    pub async fn automation_portable_register_inspect_json(
        &self,
        scope_json: String,
        source: String,
        password: Arc<PortablePasswordTransfer>,
    ) -> AppResult<PortableOperation> {
        let _wipe = CancelTransfer(password.clone());
        let scope = super::facade::scope(&scope_json)?;
        let file = PathBuf::from(source);
        selected_file(&file)?;
        let transfer = password.clone();
        let body_scope = scope.clone();
        let operation = self
            .portable_register_build(
                scope,
                move || {
                    let password = password.take()?;
                    Ok(Body::Inspect {
                        scope: body_scope,
                        file,
                        password,
                    })
                },
                Duration::from_secs(300),
            )
            .await?;
        transfer.attach(&operation)?;
        Ok(operation)
    }
    pub async fn automation_portable_register_import_json(
        &self,
        scope_json: String,
        source: String,
        requested_name: String,
        password: Arc<PortablePasswordTransfer>,
    ) -> AppResult<PortableOperation> {
        let _wipe = CancelTransfer(password.clone());
        if requested_name.len() > 1024 {
            return Err(invalid());
        }
        let name = valid_name(&requested_name)?;
        let scope = super::facade::scope(&scope_json)?;
        let source = PathBuf::from(source);
        selected_file(&source)?;
        let transfer = password.clone();
        let body_scope = scope.clone();
        let operation = self
            .portable_register_build(
                scope,
                move || {
                    Ok(Body::PrepareImport {
                        scope: body_scope,
                        source,
                        password: password.take()?,
                        requested_name,
                        name,
                        project: Uuid::new_v4(),
                        transaction: Uuid::new_v4(),
                        review: Uuid::new_v4(),
                    })
                },
                Duration::from_secs(300),
            )
            .await?;
        transfer.attach(&operation)?;
        Ok(operation)
    }
    pub(crate) async fn portable_register_import_recovery(
        &self,
        scope_json: String,
        source: String,
        project: String,
        password: Arc<PortablePasswordTransfer>,
    ) -> AppResult<PortableOperation> {
        let _wipe = CancelTransfer(password.clone());
        project_id(&project)?;
        let scope = super::facade::scope(&scope_json)?;
        let source = PathBuf::from(source);
        selected_file(&source)?;
        let transfer = password.clone();
        let body_scope = scope.clone();
        let operation = self
            .portable_register_build(
                scope,
                move || {
                    Ok(Body::RecoverImport {
                        scope: body_scope,
                        source,
                        password: password.take()?,
                        project,
                    })
                },
                Duration::from_secs(300),
            )
            .await?;
        transfer.attach(&operation)?;
        Ok(operation)
    }
}

#[cfg(test)]
mod tests;
