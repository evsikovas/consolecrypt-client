//! One actual kernel sample, not a caller clock proof or a time service.
use super::*;

/// Non-Serde/non-Clone. Production constructs samples only from fixed kernel
/// paths; signed-journal replay reconstructs ORIGINAL observations as data.
pub struct CollectorServiceClockObservation {
    pub(super) boot_id: ResourceId,
    pub(super) boottime_ns: u64,
    pub(super) wall_time: Timestamp,
    wall_ns: u64,
}
impl CollectorServiceClockObservation {
    pub fn now(&self) -> Now {
        Now(self.wall_time)
    }
    pub fn boottime_ns(&self) -> u64 {
        self.boottime_ns
    }
    #[cfg(target_os = "linux")]
    pub fn observe_local() -> Result<Self> {
        use std::io::Read;
        use std::os::unix::fs::OpenOptionsExt;
        use std::time::{SystemTime, UNIX_EPOCH};
        fn kernel(path: &str) -> Result<String> {
            let mut file = std::fs::OpenOptions::new()
                .read(true)
                .custom_flags((rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::NONBLOCK | rustix::fs::OFlags::CLOEXEC).bits() as i32)
                .open(path)
                .map_err(|_| Error::Storage)?;
            let mut raw = String::new();
            (&mut file)
                .take(129)
                .read_to_string(&mut raw)
                .map_err(|_| Error::Storage)?;
            if raw.is_empty() || raw.len() > 128 {
                return Err(Error::Storage);
            }
            Ok(raw)
        }
        let boot = kernel("/proc/sys/kernel/random/boot_id")?;
        let boot_id = ResourceId::new(boot.trim())?;
        let uptime = kernel("/proc/uptime")?;
        let first = uptime.split_whitespace().next().ok_or(Error::InvalidTime)?;
        let (sec, frac) = first.split_once('.').ok_or(Error::InvalidTime)?;
        if sec.is_empty()
            || !sec.bytes().all(|b| b.is_ascii_digit())
            || frac.is_empty()
            || frac.len() > 9
            || !frac.bytes().all(|b| b.is_ascii_digit())
        {
            return Err(Error::InvalidTime);
        }
        let sec = sec.parse::<u64>().map_err(|_| Error::InvalidTime)?;
        let frac_value = frac.parse::<u64>().map_err(|_| Error::InvalidTime)?;
        let boottime_ns = sec
            .checked_mul(1_000_000_000)
            .and_then(|n| n.checked_add(frac_value * 10u64.pow((9 - frac.len()) as u32)))
            .ok_or(Error::InvalidTime)?;
        let wall_ns = u64::try_from(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|_| Error::InvalidTime)?
                .as_nanos(),
        )
        .map_err(|_| Error::InvalidTime)?;
        if kernel("/proc/sys/kernel/random/boot_id")? != boot {
            return Err(Error::WrongState);
        }
        Ok(Self {
            boot_id,
            boottime_ns,
            wall_time: Timestamp::new(wall_ns / 1_000_000_000)?,
            wall_ns,
        })
    }
    pub(crate) fn stamp_for(&self, expires: Timestamp) -> Result<CollectorServiceClockStamp> {
        let expiry = expires
            .seconds()
            .checked_mul(1_000_000_000)
            .ok_or(Error::InvalidTime)?;
        let remaining = expiry
            .checked_sub(self.wall_ns)
            .filter(|n| *n > 0 && *n <= COLLECTOR_SERVICE_TTL * 1_000_000_000)
            .ok_or(Error::Expired)?;
        let stamp = CollectorServiceClockStamp {
            boot_id: self.boot_id,
            created_boottime_ns: self.boottime_ns,
            deadline_boottime_ns: self
                .boottime_ns
                .checked_add(remaining)
                .ok_or(Error::InvalidTime)?,
        };
        stamp.validate()?;
        Ok(stamp)
    }
    /// Synthetic signed-time inputs are confined to unit tests; production
    /// still constructs observations only through the original kernel reader.
    #[cfg(test)]
    pub(crate) fn test_journal(
        boot_id: ResourceId, boottime_ns: u64, wall_time: Timestamp,
    ) -> Result<Self> {
        Self::journal(boot_id, boottime_ns, wall_time)
    }
    /// Called only while replaying authenticated anchored journal transitions.
    pub(super) fn journal(
        boot_id: ResourceId,
        boottime_ns: u64,
        wall_time: Timestamp,
    ) -> Result<Self> {
        wall_time.valid()?;
        Ok(Self {
            boot_id,
            boottime_ns,
            wall_time,
            wall_ns: wall_time
                .seconds()
                .checked_mul(1_000_000_000)
                .ok_or(Error::InvalidTime)?,
        })
    }
    pub(super) fn check(&self, original: &CollectorServiceClockStamp) -> Result<()> {
        original.validate()?;
        if self.boot_id != original.boot_id {
            return Err(Error::WrongState);
        }
        if self.boottime_ns < original.created_boottime_ns {
            return Err(Error::InvalidTime);
        }
        if self.boottime_ns >= original.deadline_boottime_ns {
            return Err(Error::Expired);
        }
        Ok(())
    }
}
