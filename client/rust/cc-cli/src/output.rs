//! Human-readable and `--json` output.

use cc_app_core::*;
use serde::Serialize;
use std::io::Write;

pub struct Out {
    json: bool,
}

fn time(ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(ms)
        .map(|t| t.format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_default()
}

fn opt(s: &Option<String>) -> &str {
    s.as_deref().unwrap_or("-")
}

fn short(id: &str) -> &str {
    id.get(..8).unwrap_or(id)
}

impl Out {
    pub fn new(json: bool) -> Self {
        Self { json }
    }

    fn print_json<T: Serialize + ?Sized>(&self, v: &T) {
        match serde_json::to_string_pretty(v) {
            Ok(s) => println!("{s}"),
            Err(e) => eprintln!("cc: cannot encode JSON: {e}"),
        }
    }

    /// JSON of `v`, or the human form.
    pub fn value<T: Serialize + ?Sized>(&self, v: &T, human: impl FnOnce()) {
        if self.json {
            self.print_json(v);
        } else {
            human();
        }
    }

    pub fn ok(&self, msg: &str) {
        self.value(&serde_json::json!({ "ok": true, "message": msg }), || {
            println!("{msg}")
        });
    }

    pub fn profiles(&self, ps: &[ProfileInfo]) {
        self.value(ps, || {
            if ps.is_empty() {
                println!("no profiles (run `consolecrypt init`)");
            }
            for p in ps {
                println!(
                    "{} {}  {:<24} {:?}",
                    if p.active { "*" } else { " " },
                    p.id,
                    p.display_name,
                    p.kind
                );
            }
        });
    }

    pub fn profile(&self, p: &ProfileInfo) {
        self.value(p, || {
            println!("profile  {} ({})", p.display_name, p.id);
            println!("kind     {:?}", p.kind);
            if let Some(s) = &p.server_url {
                println!("server   {s}");
            }
            if let Some(e) = &p.email {
                println!("account  {e}");
            }
            println!("vault    {} ({:?})", opt(&p.vault_id), p.vault_state);
            println!("device   {}", opt(&p.device_id));
        });
    }

    pub fn created(&self, c: &CreatedProfile) {
        if self.json {
            self.print_json(c);
            return;
        }
        self.profile(&c.profile);
        if let Some(k) = &c.recovery_kit {
            self.recovery_kit(k);
        }
    }

    pub fn recovery_kit(&self, k: &RecoveryKitDto) {
        self.value(k, || {
            let mut o = std::io::stdout().lock();
            let _ = writeln!(
                o,
                "\n==================== RECOVERY KIT ===================="
            );
            let _ = writeln!(o, "Store it offline. It is shown only once and is the only");
            let _ = writeln!(o, "way back into the vault if you forget the passphrase.");
            let _ = writeln!(o, "Vault ID:  {}", k.vault_id);
            let _ = writeln!(
                o,
                "Server:    {}",
                k.server_url.as_deref().unwrap_or("local only")
            );
            let _ = writeln!(o, "Created:   {}", time(k.created_at_ms));
            let _ = writeln!(o, "Recovery phrase:");
            for (i, row) in k.words.chunks(4).enumerate() {
                let line: Vec<String> = row
                    .iter()
                    .enumerate()
                    .map(|(j, w)| format!("{:>2}. {:<10}", i * 4 + j + 1, w))
                    .collect();
                let _ = writeln!(o, "  {}", line.join(" "));
            }
            let _ = writeln!(o, "QR payload: {}", k.qr_payload);
            let _ = writeln!(
                o,
                "=======================================================\n"
            );
        });
    }

    pub fn host(&self, h: &HostDto) {
        self.value(h, || {
            println!("host {} ({}) saved", h.name, h.id);
        });
    }

    pub fn hosts(&self, hs: &[HostDto], creds: &[CredentialDto]) {
        self.value(hs, || {
            if hs.is_empty() {
                println!("no hosts");
            }
            for h in hs {
                let cred = h
                    .credential_id
                    .as_ref()
                    .and_then(|c| creds.iter().find(|x| &x.id == c))
                    .map(|c| c.name.as_str())
                    .unwrap_or("-");
                println!(
                    "{:<20} {:<28} port {:<5} user {:<12} cred {:<16} {}",
                    h.name,
                    h.address,
                    h.port.map(|p| p.to_string()).unwrap_or_else(|| "22".into()),
                    opt(&h.username),
                    cred,
                    short(&h.id)
                );
            }
        });
    }

    pub fn host_detail(&self, h: &HostDto, route: Option<ConnectionRouteDto>) {
        if self.json {
            self.print_json(&serde_json::json!({
                "host": h,
                "route": route.as_ref().map(|r| &r.route),
                "warnings": route.as_ref().map(|r| &r.warnings),
            }));
            return;
        }
        println!("{} ({})", h.name, h.id);
        println!("  address   {}:{}", h.address, h.port.unwrap_or(22));
        println!("  user      {}", opt(&h.username));
        println!(
            "  policy    {:?}   backend {:?}",
            h.host_key_policy, h.backend
        );
        if !h.tags.is_empty() {
            println!("  tags      {}", h.tags.join(", "));
        }
        match route {
            Some(r) => {
                println!("  route     {}", r.route);
                for x in r.warnings {
                    println!("  warning   {x}");
                }
            }
            None => println!("  route     (cannot plan: check credential / username)"),
        }
    }

    pub fn credential(&self, c: &CredentialDto) {
        self.value(c, || {
            println!("credential {} ({}) saved", c.name, c.id);
            if let Some(k) = &c.public_key {
                println!("public key: {k}");
            }
        });
    }

    pub fn credentials(&self, cs: &[CredentialDto]) {
        self.value(cs, || {
            if cs.is_empty() {
                println!("no credentials");
            }
            for c in cs {
                println!(
                    "{:<20} {:<16} user {:<12} {} {}",
                    c.name,
                    format!("{:?}", c.kind),
                    opt(&c.username),
                    c.fingerprint.as_deref().unwrap_or(""),
                    short(&c.id)
                );
            }
        });
    }

    pub fn exec(&self, r: &ExecResultDto) {
        if self.json {
            self.print_json(&serde_json::json!({
                "stdout": String::from_utf8_lossy(&r.stdout),
                "stderr": String::from_utf8_lossy(&r.stderr),
                "exit_status": r.exit_status,
                "exit_signal": r.exit_signal,
            }));
            return;
        }
        let _ = std::io::stdout().write_all(&r.stdout);
        let _ = std::io::stderr().write_all(&r.stderr);
        let _ = std::io::stdout().flush();
    }

    pub fn tunnels(&self, ts: &[TunnelDto]) {
        self.value(ts, || {
            if ts.is_empty() {
                println!("no tunnels");
            }
            for t in ts {
                let target = match (&t.target_host, t.target_port) {
                    (Some(h), Some(p)) => format!("{h}:{p}"),
                    _ => "(socks5)".into(),
                };
                println!(
                    "{:<16} {:?} {}:{} -> {} via {}{}",
                    t.name,
                    t.kind,
                    t.bind_host,
                    t.bind_port,
                    target,
                    short(&t.host_id),
                    if t.auto_start { " [auto]" } else { "" }
                );
            }
        });
    }

    pub fn tunnel_statuses(&self, ss: &[TunnelStatusDto]) {
        self.value(ss, || {
            for s in ss {
                println!("{:<16} {:?} listening on {}", s.name, s.state, s.listen);
                for w in &s.warnings {
                    println!("  warning: {w}");
                }
            }
        });
    }

    pub fn entries(&self, es: &[RemoteEntryDto]) {
        self.value(es, || {
            for e in es {
                println!(
                    "{} {:>12} {} {}",
                    e.mode,
                    e.size,
                    e.modified_at_ms.map(time).unwrap_or_default(),
                    e.name
                );
            }
        });
    }

    pub fn transfer(&self, s: &TransferSummaryDto, what: &str) {
        self.value(s, || {
            println!("{what}: {} bytes in {} ms", s.bytes, s.elapsed_ms)
        });
    }

    pub fn devices(&self, d: &DeviceListDto) {
        self.value(d, || {
            for x in &d.devices {
                println!(
                    "{} {:<24} {:<8} {:<8} {}{}",
                    x.device_id,
                    x.name,
                    x.platform,
                    x.status,
                    if x.trusted_for_vault { "trusted" } else { "-" },
                    if x.is_current { " (this device)" } else { "" }
                );
            }
            if !d.pending_requests.is_empty() {
                println!("pending approval requests:");
                for r in &d.pending_requests {
                    println!(
                        "  {} from '{}' ({}) until {}",
                        r.request_id,
                        r.device_name,
                        r.device_id,
                        time(r.expires_at_ms)
                    );
                }
            }
        });
    }
}
