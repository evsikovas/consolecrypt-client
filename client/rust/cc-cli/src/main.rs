//! `cc` — headless ConsoleCrypt client (end-to-end tests, power users).
//!
//! Every command goes through the `cc-app-core` facade, exactly like the
//! desktop UI. Secrets come from environment variables (`CC_PASSPHRASE`,
//! `CC_PASSWORD`, `CC_NEW_PASSPHRASE`, `CC_SECRET`, `CC_KEY_PASSPHRASE`,
//! `CC_RECOVERY_KEY`, `CC_SSH_PASSWORD`) or a no-echo prompt; they are never printed except
//! the Recovery Kit when it is created.

mod ai;
mod input;
mod output;
mod shell;

use cc_app_core::*;
use clap::{ArgAction, Args, Parser, Subcommand, ValueEnum};
use input::*;
use output::Out;

/// CLI failure.
#[derive(Debug)]
pub enum CliError {
    App(AppError),
    Usage(String),
}

impl From<AppError> for CliError {
    fn from(e: AppError) -> Self {
        CliError::App(e)
    }
}

type R<T = ()> = Result<T, CliError>;

#[derive(Parser, Debug)]
#[command(
    name = "consolecrypt",
    version,
    author,
    about = "ConsoleCrypt headless client",
    after_help = "Author: Alexander Evsikov <i@evsikov.net>"
)]
struct Cli {
    /// Data directory (default: platform app-data dir).
    #[arg(long, global = true, env = "CONSOLECRYPT_DATA_DIR")]
    data_dir: Option<String>,
    /// Profile id or name (default: the last used profile).
    #[arg(long, global = true, env = "CC_PROFILE")]
    profile: Option<String>,
    /// Where keys and tokens are kept. `file` is INSECURE (plaintext file;
    /// headless/CI only).
    #[arg(long, global = true, value_enum, default_value_t = StoreArg::Os, env = "CC_SECURE_STORE")]
    secure_store: StoreArg,
    /// Machine-readable JSON output.
    #[arg(long, global = true)]
    json: bool,
    /// Do not sync automatically (synced profiles).
    #[arg(long, global = true)]
    offline: bool,
    /// Allow plain http:// servers on non-loopback hosts (trusted LAN only).
    #[arg(long, global = true)]
    insecure_http: bool,
    /// Device name used when signing in.
    #[arg(long, global = true, env = "CC_DEVICE_NAME")]
    device_name: Option<String>,
    /// More logging (-v info, -vv debug, -vvv trace; secrets are never logged).
    #[arg(short, long, global = true, action = ArgAction::Count)]
    verbose: u8,
    /// Test hook: cheapest valid KDF parameters.
    #[arg(long, global = true, hide = true, env = "CC_TEST_FAST_KDF",
          action = ArgAction::SetTrue, value_parser = clap::builder::BoolishValueParser::new())]
    fast_kdf: bool,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum StoreArg {
    Os,
    File,
    Memory,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// Create a profile: `--local`, or `--server URL (--register|--login) --email`.
    Init(InitArgs),
    /// List profiles, or `use` / `rm` one.
    Profiles {
        #[command(subcommand)]
        action: Option<ProfilesCmd>,
    },
    /// Check that the vault unlocks.
    UnlockCheck {
        /// Use the Recovery Key (`CC_RECOVERY_KEY`) instead of the passphrase.
        #[arg(long)]
        recovery: bool,
    },
    /// Hosts.
    Host {
        #[command(subcommand)]
        cmd: HostCmd,
    },
    /// Credentials (passwords, SSH keys).
    Cred {
        #[command(subcommand)]
        cmd: CredCmd,
    },
    /// Connect to a host (exec with `-- COMMAND`, else interactive shell).
    Ssh(SshArgs),
    /// Port forwarding.
    Tunnel {
        #[command(subcommand)]
        cmd: TunnelCmd,
    },
    /// File transfer.
    Sftp {
        #[command(subcommand)]
        cmd: SftpCmd,
    },
    /// Sync (synced profiles).
    Sync {
        #[command(subcommand)]
        cmd: SyncCmd,
    },
    /// Trusted devices.
    Device {
        #[command(subcommand)]
        cmd: DeviceCmd,
    },
    /// Encrypted backups (`.ccbackup`).
    Backup {
        #[command(subcommand)]
        cmd: BackupCmd,
    },
    /// Recovery Kit (shown once at creation; `--regenerate` makes a new one).
    RecoveryKit {
        #[arg(long)]
        regenerate: bool,
    },
    /// Vault passphrase.
    Passphrase {
        #[command(subcommand)]
        cmd: PassphraseCmd,
    },
    /// AI: search the knowledge base, ask, generate / explain commands.
    Ai {
        #[command(subcommand)]
        cmd: ai::AiCmd,
    },
}

#[derive(Args, Debug)]
struct InitArgs {
    /// Local profile: no server, no account.
    #[arg(long, conflicts_with = "server")]
    local: bool,
    /// Self-hosted server URL.
    #[arg(long)]
    server: Option<String>,
    #[arg(long, requires = "server", conflicts_with = "login")]
    register: bool,
    #[arg(long, requires = "server")]
    login: bool,
    #[arg(long, requires = "server")]
    email: Option<String>,
    /// Profile display name.
    #[arg(long)]
    name: Option<String>,
    /// Vault to join (default: the account's only vault).
    #[arg(long)]
    vault: Option<String>,
    /// Create a new vault even if the account has some.
    #[arg(long)]
    create_vault: bool,
    /// Ask a trusted device to approve this one instead of typing the passphrase.
    #[arg(long)]
    request_approval: bool,
    /// Join with the Recovery Key (`CC_RECOVERY_KEY`); optional new
    /// passphrase from `CC_NEW_PASSPHRASE`.
    #[arg(long)]
    recovery: bool,
}

#[derive(Subcommand, Debug)]
enum ProfilesCmd {
    /// List profiles.
    List,
    /// Make a profile the default.
    Use { profile: String },
    /// Delete a profile and its local data (irreversible).
    Rm {
        profile: String,
        #[arg(long)]
        yes: bool,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum PolicyArg {
    Ask,
    Strict,
    AcceptNew,
}

#[derive(Subcommand, Debug)]
enum HostCmd {
    /// Add a host.
    Add {
        name: String,
        address: String,
        #[arg(long)]
        port: Option<u16>,
        #[arg(long)]
        user: Option<String>,
        /// Shared credential name or id.
        #[arg(long, group = "auth")]
        cred: Option<String>,
        /// Ask for the password at connect (nothing stored; `CC_SSH_PASSWORD`).
        #[arg(long, group = "auth")]
        ask_password: bool,
        /// Store a password for this host only (`CC_SECRET` or prompt).
        #[arg(long, group = "auth")]
        password: bool,
        /// Authenticate with the OS SSH agent.
        #[arg(long, group = "auth")]
        agent: bool,
        /// Authenticate with an external agent socket / pipe.
        #[arg(long, group = "auth")]
        agent_path: Option<String>,
        /// Jump hosts (names or ids), comma-separated, closest first.
        #[arg(long, value_delimiter = ',')]
        jump: Vec<String>,
        #[arg(long, value_enum)]
        policy: Option<PolicyArg>,
        /// Use the system OpenSSH client for this host.
        #[arg(long)]
        openssh: bool,
        #[arg(long)]
        tag: Vec<String>,
        #[arg(long)]
        notes: Option<String>,
    },
    /// List hosts.
    List,
    /// Show a host and its planned route.
    Show { host: String },
    /// Delete a host.
    Rm { host: String },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum KeyTypeArg {
    Ed25519,
    Rsa3072,
    Rsa4096,
}

#[derive(Subcommand, Debug)]
enum CredCmd {
    /// Password credential (password from `CC_SECRET` or prompt).
    AddPassword {
        name: String,
        #[arg(long)]
        user: Option<String>,
    },
    /// Generate an SSH key (Ed25519 default).
    GenKey {
        name: String,
        #[arg(long)]
        user: Option<String>,
        #[arg(long = "type", value_enum, default_value_t = KeyTypeArg::Ed25519)]
        key_type: KeyTypeArg,
        /// Encrypt the stored key with a passphrase (`CC_KEY_PASSPHRASE`).
        #[arg(long)]
        protect: bool,
        /// Remember that passphrase in the vault.
        #[arg(long, requires = "protect")]
        remember: bool,
    },
    /// Import an OpenSSH / PEM / PPK private key file.
    ImportKey {
        name: String,
        file: String,
        #[arg(long)]
        user: Option<String>,
        /// OpenSSH user certificate file.
        #[arg(long)]
        cert: Option<String>,
        /// Remember the key passphrase (`CC_KEY_PASSPHRASE`) in the vault.
        #[arg(long)]
        remember: bool,
    },
    /// List credentials.
    List,
    /// Delete a credential.
    Rm { cred: String },
}

#[derive(Args, Debug)]
struct SshArgs {
    host: String,
    /// Trust and save unknown host keys without asking.
    #[arg(long)]
    accept_host_key: bool,
    /// Remote command (exec mode).
    #[arg(last = true)]
    command: Vec<String>,
}

#[derive(Subcommand, Debug)]
enum TunnelCmd {
    /// Save a tunnel: `--local [HOST:]PORT --to HOST:PORT`, `--remote …`, or `--dynamic [HOST:]PORT`.
    Add {
        name: String,
        host: String,
        #[arg(long, group = "kind")]
        local: Option<String>,
        #[arg(long, group = "kind")]
        remote: Option<String>,
        #[arg(long, group = "kind")]
        dynamic: Option<String>,
        #[arg(long)]
        to: Option<String>,
        #[arg(long)]
        auto_start: bool,
    },
    /// List saved tunnels.
    List,
    /// Start tunnels and keep them open until Ctrl-C.
    Start { tunnels: Vec<String> },
}

#[derive(Subcommand, Debug)]
enum SftpCmd {
    Ls {
        host: String,
        path: Option<String>,
    },
    Get {
        host: String,
        remote: String,
        local: Option<String>,
    },
    Put {
        host: String,
        local: String,
        remote: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
enum SyncCmd {
    Now,
    Status,
    /// Enable sync for a local profile.
    Enable {
        #[arg(long)]
        server: String,
        #[arg(long, conflicts_with = "login")]
        register: bool,
        #[arg(long)]
        login: bool,
        #[arg(long)]
        email: String,
    },
    /// Stop syncing; keep local data (profile becomes local).
    Disconnect {
        /// Also revoke this device on the server.
        #[arg(long)]
        revoke: bool,
    },
}

#[derive(Subcommand, Debug)]
enum DeviceCmd {
    List,
    /// This device's verification code.
    Code,
    /// New device: ask a trusted device for approval.
    Request {
        #[arg(long)]
        vault: Option<String>,
    },
    /// New device: complete the join after approval.
    Finish {
        #[arg(long)]
        vault: Option<String>,
    },
    /// Trusted device: approve a request after comparing verification codes.
    Approve {
        request_id: String,
        /// The code shown on the new device (compared with the one computed here).
        #[arg(long)]
        code: Option<String>,
    },
    Reject {
        request_id: String,
    },
    Revoke {
        device_id: String,
        #[arg(long)]
        reason: Option<String>,
    },
    /// Rename this device.
    Rename {
        device_id: String,
        name: String,
    },
}

#[derive(Subcommand, Debug)]
enum BackupCmd {
    Export {
        path: String,
    },
    /// Restore into a new local profile.
    Import {
        path: String,
        #[arg(long)]
        name: Option<String>,
        /// Use the Recovery Key (`CC_RECOVERY_KEY`); new passphrase from `CC_NEW_PASSPHRASE`.
        #[arg(long)]
        recovery: bool,
    },
}

#[derive(Subcommand, Debug)]
enum PassphraseCmd {
    /// Change (current from `CC_PASSPHRASE`, new from `CC_NEW_PASSPHRASE`).
    Change,
    /// Forgot: reset with the Recovery Key (`CC_RECOVERY_KEY`).
    Reset,
}

fn config(cli: &Cli) -> AppConfig {
    let mut c = AppConfig::new(Platform::Cli);
    c.data_dir = cli.data_dir.clone();
    c.secure_store = match cli.secure_store {
        StoreArg::Os => SecureStoreKind::Os,
        StoreArg::File => SecureStoreKind::InsecureFile,
        StoreArg::Memory => SecureStoreKind::InMemory,
    };
    c.kdf = if cli.fast_kdf {
        KdfPolicy::Floor
    } else {
        KdfPolicy::Calibrate
    };
    c.background_sync = false;
    c.auto_start_tunnels = false;
    c.allow_insecure_http = cli.insecure_http;
    if let Some(n) = &cli.device_name {
        c.device_name = n.clone();
    } else {
        c.device_name = format!("{} (cc)", default_device_name());
    }
    c
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let directives = if cli.verbose > 0 {
        logging::verbosity_directives(cli.verbose).to_owned()
    } else {
        std::env::var("RUST_LOG").unwrap_or_else(|_| "warn".into())
    };
    let _ = logging::init(Some(&directives));
    let app = match AppCore::new(config(&cli)) {
        Ok(a) => a,
        Err(e) => std::process::exit(report(&cli, &CliError::App(e))),
    };
    let result = run(&cli, &app).await;
    let _ = app.shutdown().await;
    let code = match result {
        Ok(code) => code,
        Err(e) => report(&cli, &e),
    };
    std::process::exit(code);
}

fn report(cli: &Cli, e: &CliError) -> i32 {
    let (code, message, exit) = match e {
        CliError::App(a) => (a.code().to_owned(), a.message(), 1),
        CliError::Usage(m) => ("usage".to_owned(), m.clone(), 2),
    };
    if cli.json {
        println!(
            "{}",
            serde_json::json!({ "error": { "code": code, "message": message } })
        );
    }
    eprintln!("cc: error [{code}]: {message}");
    exit
}

/// Open the selected profile (vault still locked).
async fn open(cli: &Cli, app: &AppCore) -> R<ProfileInfo> {
    let id = match &cli.profile {
        Some(p) => app.find_profile(p.clone()).await?.id,
        None => match app.last_active_profile_id().await? {
            Some(id) => id,
            None => {
                let all = app.list_profiles().await?;
                match all.as_slice() {
                    [one] => one.id.clone(),
                    [] => {
                        return Err(CliError::Usage(
                            "no profile yet; run `consolecrypt init`".into(),
                        ))
                    }
                    _ => return Err(CliError::Usage("several profiles; pass --profile".into())),
                }
            }
        },
    };
    Ok(app.open_profile(id).await?)
}

/// Open and unlock (passphrase), then pull if synced.
async fn unlocked(cli: &Cli, app: &AppCore) -> R<ProfileInfo> {
    let info = open(cli, app).await?;
    if info.vault_state == VaultStateDto::NoVault {
        return Err(CliError::Usage(
            "this profile has no vault yet (cc device finish, or init again)".into(),
        ));
    }
    let pass = secret(ENV_PASSPHRASE, "Vault passphrase", false)?;
    app.unlock_with_passphrase(pass).await?;
    auto_sync(cli, app, &info).await;
    Ok(info)
}

/// Best-effort sync for synced profiles (offline is not an error).
async fn auto_sync(cli: &Cli, app: &AppCore, info: &ProfileInfo) {
    if info.kind != ProfileKind::Synced || cli.offline {
        return;
    }
    if let Err(e) = app.sync_now().await {
        eprintln!("cc: warning: sync failed ({}): {}", e.code(), e.message());
    }
}

/// Answer SSH prompts on the terminal (or per flags) while connecting.
fn prompt_responder(app: &AppCore, accept_host_key: bool) -> tokio::task::JoinHandle<()> {
    let mut rx = app.subscribe_prompts();
    let app = app.clone();
    tokio::spawn(async move {
        while let Ok(p) = rx.recv().await {
            match p {
                PromptRequest::HostKey(h) => {
                    let decision = if accept_host_key {
                        HostKeyDecision::AcceptAndSave
                    } else if stdin_is_tty() {
                        let q = format!(
                            "The authenticity of host '{}' can't be established.\n{} key fingerprint is {}.\nTrust it? [yes/once/no]",
                            h.host_pattern, h.key_type, h.fingerprint_sha256
                        );
                        match tokio::task::spawn_blocking(move || ask(&q)).await {
                            Ok(Ok(a)) if a.eq_ignore_ascii_case("yes") || a == "y" => {
                                HostKeyDecision::AcceptAndSave
                            }
                            Ok(Ok(a)) if a.eq_ignore_ascii_case("once") => {
                                HostKeyDecision::AcceptOnce
                            }
                            _ => HostKeyDecision::Reject,
                        }
                    } else {
                        eprintln!(
                            "cc: unknown host key for {} ({} {}); rerun with --accept-host-key to trust it",
                            h.host_pattern, h.key_type, h.fingerprint_sha256
                        );
                        HostKeyDecision::Reject
                    };
                    let _ = app.answer_host_key_prompt(h.request_id, decision);
                }
                PromptRequest::Passphrase(p) => {
                    let name = p.credential_name.clone();
                    let answer = tokio::task::spawn_blocking(move || {
                        if let Ok(v) = std::env::var(ENV_KEY_PASSPHRASE) {
                            if p.attempt == 0 {
                                return Some(v);
                            }
                        }
                        if !stdin_is_tty() {
                            return None;
                        }
                        rpassword::prompt_password(format!("Passphrase for key '{name}': ")).ok()
                    })
                    .await
                    .ok()
                    .flatten();
                    let _ = app.answer_passphrase_prompt(p.request_id, answer);
                }
                PromptRequest::Password(p) => {
                    let host = p.host_name.clone();
                    let answer = tokio::task::spawn_blocking(move || {
                        if let Ok(v) = std::env::var(ENV_SSH_PASSWORD) {
                            return Some(v);
                        }
                        if !stdin_is_tty() {
                            return None;
                        }
                        rpassword::prompt_password(format!("Password for '{host}': ")).ok()
                    })
                    .await
                    .ok()
                    .flatten();
                    let _ = app.answer_password_prompt(p.request_id, answer);
                }
            }
        }
    })
}

fn policy(p: PolicyArg) -> HostKeyPolicy {
    match p {
        PolicyArg::Ask => HostKeyPolicy::Ask,
        PolicyArg::Strict => HostKeyPolicy::Strict,
        PolicyArg::AcceptNew => HostKeyPolicy::AcceptNew,
    }
}

/// `[host:]port` → (host, port); default host 127.0.0.1.
fn bind_spec(s: &str) -> R<(String, u16)> {
    let (h, p) = match s.rsplit_once(':') {
        Some((h, p)) => (h.trim_matches(|c| c == '[' || c == ']').to_owned(), p),
        None => ("127.0.0.1".to_owned(), s),
    };
    let port = p
        .parse()
        .map_err(|_| CliError::Usage(format!("invalid port in {s:?}")))?;
    Ok((h, port))
}

async fn run(cli: &Cli, app: &AppCore) -> R<i32> {
    let out = Out::new(cli.json);
    match &cli.cmd {
        Cmd::Init(a) => init(cli, app, a, &out).await?,
        Cmd::Profiles { action } => match action {
            None | Some(ProfilesCmd::List) => out.profiles(&app.list_profiles().await?),
            Some(ProfilesCmd::Use { profile }) => {
                let p = app.find_profile(profile.clone()).await?;
                let info = app.open_profile(p.id).await?;
                out.profile(&info);
            }
            Some(ProfilesCmd::Rm { profile, yes }) => {
                let p = app.find_profile(profile.clone()).await?;
                if !yes
                    && !confirm(&format!(
                        "Delete profile '{}' and its local data?",
                        p.display_name
                    ))?
                {
                    return Err(CliError::Usage("cancelled".into()));
                }
                app.remove_profile(p.id.clone()).await?;
                out.ok(&format!("profile {} removed", p.display_name));
            }
        },
        Cmd::UnlockCheck { recovery } => {
            let info = open(cli, app).await?;
            if *recovery {
                let k = secret(ENV_RECOVERY_KEY, "Recovery key", false)?;
                app.unlock_with_recovery_key(k).await?;
            } else {
                let p = secret(ENV_PASSPHRASE, "Vault passphrase", false)?;
                app.unlock_with_passphrase(p).await?;
            }
            out.value(
                &serde_json::json!({"ok": true, "profile": info.display_name, "vault_id": info.vault_id}),
                || println!("ok: vault {} unlocks", info.vault_id.clone().unwrap_or_default()),
            );
        }
        Cmd::Host { cmd } => host(cli, app, cmd, &out).await?,
        Cmd::Cred { cmd } => cred(cli, app, cmd, &out).await?,
        Cmd::Ssh(a) => return ssh(cli, app, a, &out).await,
        Cmd::Ai { cmd } => return ai::ai(cli, app, cmd, &out).await,
        Cmd::Tunnel { cmd } => tunnel(cli, app, cmd, &out).await?,
        Cmd::Sftp { cmd } => sftp(cli, app, cmd, &out).await?,
        Cmd::Sync { cmd } => sync(cli, app, cmd, &out).await?,
        Cmd::Device { cmd } => device(cli, app, cmd, &out).await?,
        Cmd::Backup { cmd } => backup(cli, app, cmd, &out).await?,
        Cmd::RecoveryKit { regenerate } => {
            if !regenerate {
                out.ok("The Recovery Kit is shown only once, when the vault is created. \
                        `consolecrypt recovery-kit --regenerate` creates a new one (the old key stops working).");
                return Ok(0);
            }
            unlocked(cli, app).await?;
            let kit = app.regenerate_recovery_kit().await?;
            out.recovery_kit(&kit);
            onboarding_check(app, cli).await?;
        }
        Cmd::Passphrase { cmd } => match cmd {
            PassphraseCmd::Change => {
                open(cli, app).await?;
                let current = secret(ENV_PASSPHRASE, "Current vault passphrase", false)?;
                app.unlock_with_passphrase(current.clone()).await?;
                let new = secret(ENV_NEW_PASSPHRASE, "New vault passphrase", true)?;
                app.change_passphrase(current, new).await?;
                out.ok("vault passphrase changed");
            }
            PassphraseCmd::Reset => {
                open(cli, app).await?;
                let k = secret(ENV_RECOVERY_KEY, "Recovery key", false)?;
                let new = secret(ENV_NEW_PASSPHRASE, "New vault passphrase", true)?;
                app.reset_passphrase_with_recovery_key(k, new).await?;
                out.ok("vault passphrase reset");
            }
        },
    }
    Ok(0)
}

/// Mandatory 3-word check when interactive; otherwise remind the user.
async fn onboarding_check(app: &AppCore, cli: &Cli) -> R {
    if cli.json || !stdin_is_tty() {
        eprintln!("cc: store the Recovery Kit above offline now — it is not shown again");
        app.recovery_kit_acknowledge().await?;
        return Ok(());
    }
    loop {
        let check = app.recovery_kit_start_check().await?;
        let mut answers = Vec::new();
        for p in &check.positions {
            answers.push(ask(&format!("Recovery phrase word #{p}:"))?);
        }
        let wrong = app.recovery_kit_verify(answers).await?;
        if wrong.is_empty() {
            eprintln!("Recovery Kit verified.");
            return Ok(());
        }
        eprintln!("Words {wrong:?} do not match; check your Recovery Kit and try again.");
    }
}

async fn init(cli: &Cli, app: &AppCore, a: &InitArgs, out: &Out) -> R {
    if a.local {
        let pass = secret(ENV_PASSPHRASE, "New vault passphrase", true)?;
        let name = a.name.clone().unwrap_or_else(|| "Local".into());
        let created = app.create_local_profile(name, pass).await?;
        out.created(&created);
        eprintln!("cc: local profile — there is no cloud copy: keep the Recovery Kit and make backups (cc backup export)");
        return onboarding_check(app, cli).await;
    }
    let Some(server) = a.server.clone() else {
        return Err(CliError::Usage("pass --local or --server URL".into()));
    };
    let mode = match (a.register, a.login) {
        (true, false) => AccountMode::Register,
        (false, true) => AccountMode::Login,
        _ => return Err(CliError::Usage("pass --register or --login".into())),
    };
    let email = a
        .email
        .clone()
        .ok_or_else(|| CliError::Usage("--email is required".into()))?;
    let password = secret(
        ENV_PASSWORD,
        "Account password",
        mode == AccountMode::Register,
    )?;
    let name = a.name.clone().unwrap_or_else(|| "Synced".into());
    let acct = app
        .create_synced_profile(name, server, email, password, mode)
        .await?;
    if a.request_approval {
        let r = app.request_device_approval(a.vault.clone()).await?;
        out.value(&r, || {
            println!("Approval requested (request {}).", r.request_id);
            println!("Verification code of this device: {}", r.verification_code);
            println!(
                "On a trusted device run: cc device approve {} --code \"{}\"",
                r.request_id, r.verification_code
            );
            println!("Then here: cc device finish");
        });
        return Ok(());
    }
    if acct.vaults.is_empty() || a.create_vault {
        let pass = secret(ENV_PASSPHRASE, "New vault passphrase", true)?;
        let kit = app.create_vault(pass).await?;
        out.recovery_kit(&kit);
        let _ = app.sync_now().await;
        return onboarding_check(app, cli).await;
    }
    let vault = match &a.vault {
        Some(v) => v.clone(),
        None => match acct.vaults.as_slice() {
            [one] => one.vault_id.clone(),
            _ => {
                return Err(CliError::Usage(
                    "the account has several vaults; pass --vault ID".into(),
                ))
            }
        },
    };
    let info = if a.recovery {
        let k = secret(ENV_RECOVERY_KEY, "Recovery key", false)?;
        let new = optional_secret(ENV_NEW_PASSPHRASE, "New vault passphrase")?;
        app.join_vault_with_recovery_key(vault, k, new).await?
    } else {
        let pass = secret(ENV_PASSPHRASE, "Vault passphrase", false)?;
        app.join_vault_with_passphrase(vault, pass).await?
    };
    let _ = app.sync_now().await;
    out.profile(&info);
    Ok(())
}

async fn host(cli: &Cli, app: &AppCore, cmd: &HostCmd, out: &Out) -> R {
    let info = unlocked(cli, app).await?;
    match cmd {
        HostCmd::Add {
            name,
            address,
            port,
            user,
            cred,
            ask_password,
            password,
            agent,
            agent_path,
            jump,
            policy: pol,
            openssh,
            tag,
            notes,
        } => {
            let mut h = HostDto::new(name.clone(), address.clone());
            h.port = *port;
            h.username = user.clone();
            let auth = if let Some(c) = cred {
                HostAuth::Credential {
                    credential_id: app.find_credential(c.clone()).await?.id,
                }
            } else if *ask_password {
                HostAuth::PasswordPrompt
            } else if *password {
                HostAuth::InlinePassword {
                    password: Some(secret(ENV_SECRET, "Host password", true)?),
                }
            } else if *agent {
                HostAuth::Agent {
                    kind: AgentKind::Os,
                    path: None,
                }
            } else if let Some(p) = agent_path {
                HostAuth::Agent {
                    kind: AgentKind::External,
                    path: Some(p.clone()),
                }
            } else {
                HostAuth::Inherit
            };
            for j in jump {
                h.jump_chain.push(app.find_host(j.clone()).await?.id);
            }
            if let Some(p) = pol {
                h.host_key_policy = policy(*p);
            }
            if *openssh {
                h.backend = SshBackend::OpenSsh;
            }
            h.tags = tag.clone();
            h.notes = notes.clone().unwrap_or_default();
            let saved = app.save_host_with_auth(h, auth).await?;
            auto_sync(cli, app, &info).await;
            out.host(&saved);
        }
        HostCmd::List => {
            let hosts = app.list_hosts().await?;
            let creds = app.list_credentials().await?;
            out.hosts(&hosts, &creds);
        }
        HostCmd::Show { host } => {
            let h = app.find_host(host.clone()).await?;
            let route = app.describe_connection(h.id.clone()).await;
            out.host_detail(&h, route.ok());
        }
        HostCmd::Rm { host } => {
            let h = app.find_host(host.clone()).await?;
            app.delete_host(h.id.clone()).await?;
            auto_sync(cli, app, &info).await;
            out.ok(&format!("host {} deleted", h.name));
        }
    }
    Ok(())
}

async fn cred(cli: &Cli, app: &AppCore, cmd: &CredCmd, out: &Out) -> R {
    let info = unlocked(cli, app).await?;
    match cmd {
        CredCmd::AddPassword { name, user } => {
            let pw = secret(ENV_SECRET, "Password", true)?;
            let c = app
                .add_password_credential(name.clone(), user.clone(), pw)
                .await?;
            auto_sync(cli, app, &info).await;
            out.credential(&c);
        }
        CredCmd::GenKey {
            name,
            user,
            key_type,
            protect,
            remember,
        } => {
            let passphrase = if *protect {
                Some(secret(ENV_KEY_PASSPHRASE, "Key passphrase", true)?)
            } else {
                None
            };
            let alg = match key_type {
                KeyTypeArg::Ed25519 => KeyGenAlgorithm::Ed25519,
                KeyTypeArg::Rsa3072 => KeyGenAlgorithm::Rsa3072,
                KeyTypeArg::Rsa4096 => KeyGenAlgorithm::Rsa4096,
            };
            let c = app
                .generate_ssh_key(name.clone(), user.clone(), alg, passphrase, *remember)
                .await?;
            auto_sync(cli, app, &info).await;
            out.credential(&c);
        }
        CredCmd::ImportKey {
            name,
            file,
            user,
            cert,
            remember,
        } => {
            let text = std::fs::read_to_string(file)
                .map_err(|e| CliError::Usage(format!("cannot read {file}: {e}")))?;
            let certificate = match cert {
                Some(p) => Some(
                    std::fs::read_to_string(p)
                        .map_err(|e| CliError::Usage(format!("cannot read {p}: {e}")))?,
                ),
                None => None,
            };
            let passphrase = std::env::var(ENV_KEY_PASSPHRASE)
                .ok()
                .filter(|v| !v.is_empty());
            let c = app
                .import_ssh_key(
                    name.clone(),
                    user.clone(),
                    text,
                    passphrase,
                    *remember,
                    certificate,
                )
                .await?;
            auto_sync(cli, app, &info).await;
            out.credential(&c);
        }
        CredCmd::List => out.credentials(&app.list_credentials().await?),
        CredCmd::Rm { cred } => {
            let c = app.find_credential(cred.clone()).await?;
            app.delete_credential(c.id.clone()).await?;
            auto_sync(cli, app, &info).await;
            out.ok(&format!("credential {} deleted", c.name));
        }
    }
    Ok(())
}

async fn ssh(cli: &Cli, app: &AppCore, a: &SshArgs, out: &Out) -> R<i32> {
    unlocked(cli, app).await?;
    let h = app.find_host(a.host.clone()).await?;
    let responder = prompt_responder(app, a.accept_host_key);
    let openssh = app.host_uses_openssh(h.id.clone()).await?;
    let code = if !a.command.is_empty() {
        let command = a.command.join(" ");
        if openssh && stdin_is_tty() && !cli.json {
            shell::openssh(app, h.id.clone(), Some(command), false).await?
        } else {
            let r = app.exec(h.id.clone(), command).await?;
            out.exec(&r);
            r.exit_status.map(|c| c as i32).unwrap_or(255)
        }
    } else if !stdin_is_tty() {
        return Err(CliError::Usage(
            "an interactive shell needs a terminal; pass `-- COMMAND` for exec mode".into(),
        ));
    } else if openssh {
        shell::openssh(app, h.id.clone(), None, true).await?
    } else {
        shell::native(app, h.id.clone()).await?
    };
    responder.abort();
    Ok(code)
}

async fn tunnel(cli: &Cli, app: &AppCore, cmd: &TunnelCmd, out: &Out) -> R {
    let info = unlocked(cli, app).await?;
    match cmd {
        TunnelCmd::Add {
            name,
            host,
            local,
            remote,
            dynamic,
            to,
            auto_start,
        } => {
            let h = app.find_host(host.clone()).await?;
            let (kind, bind) = match (local, remote, dynamic) {
                (Some(b), None, None) => (TunnelKind::Local, b),
                (None, Some(b), None) => (TunnelKind::Remote, b),
                (None, None, Some(b)) => (TunnelKind::Dynamic, b),
                _ => {
                    return Err(CliError::Usage(
                        "pass exactly one of --local, --remote, --dynamic".into(),
                    ))
                }
            };
            let (bind_host, bind_port) = bind_spec(bind)?;
            let (target_host, target_port) = match (kind, to) {
                (TunnelKind::Dynamic, _) => (None, None),
                (_, Some(t)) => {
                    let (th, tp) = bind_spec(t)?;
                    (Some(th), Some(tp))
                }
                (_, None) => return Err(CliError::Usage("--to HOST:PORT is required".into())),
            };
            let t = app
                .save_tunnel(TunnelDto {
                    id: String::new(),
                    name: name.clone(),
                    kind,
                    host_id: h.id,
                    bind_host,
                    bind_port,
                    target_host,
                    target_port,
                    auto_start: *auto_start,
                    binds_publicly: false,
                    created_at_ms: 0,
                    updated_at_ms: 0,
                })
                .await?;
            auto_sync(cli, app, &info).await;
            out.tunnels(&[t]);
        }
        TunnelCmd::List => out.tunnels(&app.list_tunnels().await?),
        TunnelCmd::Start { tunnels } => {
            let saved = app.list_tunnels().await?;
            let selected: Vec<&TunnelDto> = if tunnels.is_empty() {
                saved.iter().filter(|t| t.auto_start).collect()
            } else {
                tunnels
                    .iter()
                    .map(|n| {
                        saved
                            .iter()
                            .find(|t| t.id == *n || t.name.eq_ignore_ascii_case(n))
                            .ok_or_else(|| CliError::Usage(format!("no tunnel named {n}")))
                    })
                    .collect::<R<_>>()?
            };
            if selected.is_empty() {
                return Err(CliError::Usage("no tunnels to start".into()));
            }
            let responder = prompt_responder(app, false);
            let mut statuses = Vec::new();
            for t in selected {
                statuses.push(app.start_tunnel(t.id.clone()).await?);
            }
            out.tunnel_statuses(&statuses);
            eprintln!("cc: tunnels running; press Ctrl-C to stop");
            let _ = tokio::signal::ctrl_c().await;
            for s in statuses {
                let _ = app.stop_tunnel(s.id).await;
            }
            responder.abort();
        }
    }
    Ok(())
}

fn basename(p: &str) -> String {
    p.rsplit(['/', '\\']).next().unwrap_or(p).to_owned()
}

async fn sftp(cli: &Cli, app: &AppCore, cmd: &SftpCmd, out: &Out) -> R {
    unlocked(cli, app).await?;
    let host = match cmd {
        SftpCmd::Ls { host, .. } | SftpCmd::Get { host, .. } | SftpCmd::Put { host, .. } => host,
    };
    let h = app.find_host(host.clone()).await?;
    let responder = prompt_responder(app, false);
    let sid = app.sftp_open(h.id.clone()).await?;
    let r = async {
        match cmd {
            SftpCmd::Ls { path, .. } => {
                let path = match path {
                    Some(p) => p.clone(),
                    None => app.sftp_home(sid.clone()).await?,
                };
                out.entries(&app.sftp_list(sid.clone(), path).await?);
            }
            SftpCmd::Get { remote, local, .. } => {
                let local = local.clone().unwrap_or_else(|| basename(remote));
                let s = app
                    .sftp_download(sid.clone(), remote.clone(), local.clone())
                    .await?;
                out.transfer(&s, &format!("{remote} -> {local}"));
            }
            SftpCmd::Put { local, remote, .. } => {
                let remote = remote.clone().unwrap_or_else(|| basename(local));
                let s = app
                    .sftp_upload(sid.clone(), local.clone(), remote.clone())
                    .await?;
                out.transfer(&s, &format!("{local} -> {remote}"));
            }
        }
        Ok::<_, CliError>(())
    }
    .await;
    let _ = app.sftp_close(sid).await;
    responder.abort();
    r
}

async fn sync(cli: &Cli, app: &AppCore, cmd: &SyncCmd, out: &Out) -> R {
    match cmd {
        SyncCmd::Now => {
            open(cli, app).await?;
            app.unlock_with_passphrase(secret(ENV_PASSPHRASE, "Vault passphrase", false)?)
                .await?;
            let r = app.sync_now().await?;
            out.value(&r, || {
                println!(
                    "synced: pushed {}, pulled {}, conflicts {} (resolved {}), cursor {}",
                    r.pushed, r.pulled, r.conflicts, r.resolved, r.last_sequence
                )
            });
        }
        SyncCmd::Status => {
            let info = open(cli, app).await?;
            if info.vault_state != VaultStateDto::NoVault {
                if let Ok(p) = std::env::var(ENV_PASSPHRASE) {
                    app.unlock_with_passphrase(p).await?;
                }
            }
            let s = app.sync_status().await?;
            out.value(&s, || {
                println!(
                    "{:?}: pending {}, conflicts {}, failed {}",
                    s.phase, s.pending, s.conflicts, s.failed
                )
            });
        }
        SyncCmd::Enable {
            server,
            register,
            login,
            email,
        } => {
            let mode = match (register, login) {
                (true, false) => AccountMode::Register,
                (false, true) => AccountMode::Login,
                _ => return Err(CliError::Usage("pass --register or --login".into())),
            };
            unlocked(cli, app).await?;
            let pw = secret(
                ENV_PASSWORD,
                "Account password",
                mode == AccountMode::Register,
            )?;
            let info = app
                .enable_sync(server.clone(), email.clone(), pw, mode)
                .await?;
            let _ = app.sync_now().await;
            out.profile(&info);
        }
        SyncCmd::Disconnect { revoke } => {
            open(cli, app).await?;
            let info = app.disconnect(*revoke).await?;
            out.profile(&info);
        }
    }
    Ok(())
}

fn digits(s: &str) -> String {
    s.chars().filter(char::is_ascii_digit).collect()
}

async fn device(cli: &Cli, app: &AppCore, cmd: &DeviceCmd, out: &Out) -> R {
    match cmd {
        DeviceCmd::List => {
            open(cli, app).await?;
            out.devices(&app.list_devices().await?);
        }
        DeviceCmd::Code => {
            open(cli, app).await?;
            let c = app.device_verification_code().await?;
            out.value(&serde_json::json!({ "verification_code": c }), || {
                println!("{c}")
            });
        }
        DeviceCmd::Request { vault } => {
            open(cli, app).await?;
            let r = app.request_device_approval(vault.clone()).await?;
            out.value(&r, || {
                println!("request {}", r.request_id);
                println!("verification code: {}", r.verification_code)
            });
        }
        DeviceCmd::Finish { vault } => {
            open(cli, app).await?;
            let done = app.finish_device_approval(vault.clone()).await?;
            if done {
                let _ = app.sync_now().await;
            }
            out.value(&serde_json::json!({ "joined": done }), || {
                if done {
                    println!("device approved; vault joined")
                } else {
                    println!("not approved yet")
                }
            });
            if !done {
                return Err(CliError::Usage("the device is not approved yet".into()));
            }
        }
        DeviceCmd::Approve { request_id, code } => {
            unlocked(cli, app).await?;
            let p = app.start_device_approval(request_id.clone()).await?;
            let matches = match code {
                Some(c) => digits(c) == digits(&p.verification_code),
                None => {
                    eprintln!(
                        "Verification code of '{}': {}",
                        p.device_name, p.verification_code
                    );
                    confirm("Does it match the code shown on the new device?")?
                }
            };
            if !matches {
                return Err(CliError::Usage(
                    "verification codes do not match — NOT approved (possible key substitution)"
                        .into(),
                ));
            }
            app.confirm_device_approval(request_id.clone()).await?;
            out.ok(&format!("device '{}' approved", p.device_name));
        }
        DeviceCmd::Reject { request_id } => {
            open(cli, app).await?;
            app.reject_device_request(request_id.clone()).await?;
            out.ok("request rejected");
        }
        DeviceCmd::Revoke { device_id, reason } => {
            open(cli, app).await?;
            app.revoke_device(device_id.clone(), reason.clone()).await?;
            out.ok("device revoked");
        }
        DeviceCmd::Rename { device_id, name } => {
            open(cli, app).await?;
            let d = app.rename_device(device_id.clone(), name.clone()).await?;
            out.value(&d, || println!("renamed to {}", d.name));
        }
    }
    Ok(())
}

async fn backup(cli: &Cli, app: &AppCore, cmd: &BackupCmd, out: &Out) -> R {
    match cmd {
        BackupCmd::Export { path } => {
            unlocked(cli, app).await?;
            let s = app.export_backup(path.clone()).await?;
            out.value(&s, || {
                println!("backup written to {} ({} objects)", s.path, s.objects)
            });
        }
        BackupCmd::Import {
            path,
            name,
            recovery,
        } => {
            let (unlock, new) = if *recovery {
                (
                    BackupUnlock::RecoveryKey(secret(ENV_RECOVERY_KEY, "Recovery key", false)?),
                    optional_secret(ENV_NEW_PASSPHRASE, "New vault passphrase")?,
                )
            } else {
                (
                    BackupUnlock::Passphrase(secret(ENV_PASSPHRASE, "Vault passphrase", false)?),
                    None,
                )
            };
            let name = name.clone().unwrap_or_else(|| "Restored".into());
            let created = app.import_backup(path.clone(), name, unlock, new).await?;
            out.created(&created);
        }
    }
    Ok(())
}
