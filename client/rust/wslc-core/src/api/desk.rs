use crate::{
    models::*,
    runtime::cli::{Runtime, WslcCliAdapter},
    services::ContainerService,
};
use std::sync::{Arc, OnceLock};
use tokio::sync::RwLock;

static CORE: OnceLock<RwLock<Option<Arc<ContainerService<WslcCliAdapter>>>>> = OnceLock::new();

fn core() -> &'static RwLock<Option<Arc<ContainerService<WslcCliAdapter>>>> {
    CORE.get_or_init(|| RwLock::new(None))
}

async fn service() -> AppResult<Arc<ContainerService<WslcCliAdapter>>> {
    core().read().await.clone().ok_or_else(|| {
        AppError::new(
            "not_initialized",
            "api",
            "Сначала проверьте подключение к WSLc",
        )
    })
}

pub async fn probe_runtime(cli_path: Option<String>) -> AppResult<RuntimeInfo> {
    let mut guard = core().write().await;
    *guard = None;
    let adapter = WslcCliAdapter::new(cli_path.as_deref())?;
    let info = adapter.probe().await?;
    if info.readable && info.version_supported {
        *guard = Some(Arc::new(ContainerService::new(adapter)));
    }
    Ok(info)
}

pub async fn list_containers() -> AppResult<Vec<ContainerSummary>> {
    service().await?.runtime.list_containers().await
}
pub async fn inspect_container(
    container_id: String,
    reveal_environment: bool,
) -> AppResult<ContainerDetails> {
    service()
        .await?
        .runtime
        .inspect_container(&container_id, reveal_environment)
        .await
}
pub async fn start_container(container_id: String) -> AppResult<OperationResult> {
    service()
        .await?
        .operate(&container_id, ContainerAction::Start, false)
        .await
}
pub async fn stop_container(container_id: String, confirmed: bool) -> AppResult<OperationResult> {
    service()
        .await?
        .operate(&container_id, ContainerAction::Stop, confirmed)
        .await
}
pub async fn restart_container(
    container_id: String,
    confirmed: bool,
) -> AppResult<OperationResult> {
    service()
        .await?
        .operate(&container_id, ContainerAction::Restart, confirmed)
        .await
}
pub async fn list_images() -> AppResult<Vec<ImageSummary>> {
    service().await?.runtime.list_images().await
}
pub async fn list_volumes() -> AppResult<Vec<VolumeSummary>> {
    service().await?.runtime.list_volumes().await
}
pub async fn inspect_image(image_id: String, reveal_environment: bool) -> AppResult<String> {
    service()
        .await?
        .runtime
        .inspect_readonly("image", &image_id, reveal_environment)
        .await
}
pub async fn inspect_volume(volume_name: String) -> AppResult<String> {
    service()
        .await?
        .runtime
        .inspect_readonly("volume", &volume_name, false)
        .await
}

pub async fn list_networks() -> AppResult<Vec<NetworkSummary>> {
    service().await?.runtime.list_networks().await
}
pub async fn inspect_network(network_name: String) -> AppResult<String> {
    service()
        .await?
        .runtime
        .inspect_network(&network_name)
        .await
}
pub async fn run_container(spec: RunSpec) -> AppResult<String> {
    service().await?.runtime.run_container(&spec).await
}
pub fn preview_run_command(spec: RunSpec) -> AppResult<String> {
    crate::runspec::preview(&spec)
}
pub async fn container_run_spec(container_id: String, include_env: bool) -> AppResult<RunSpec> {
    service()
        .await?
        .runtime
        .container_run_spec(&container_id, include_env)
        .await
}
pub async fn container_env(container_id: String) -> AppResult<Vec<EnvEntry>> {
    service().await?.runtime.container_env(&container_id).await
}
pub fn copy_run_command(spec: RunSpec) -> AppResult<String> {
    crate::runspec::copy_command(&spec)
}
pub async fn pick_folder(title: String) -> AppResult<Option<String>> {
    tokio::task::spawn_blocking(move || {
        rfd::FileDialog::new()
            .set_title(title)
            .pick_folder()
            .map(|p| {
                crate::desktop::simplify_path(&p)
                    .to_string_lossy()
                    .into_owned()
            })
    })
    .await
    .map_err(|_| AppError::new("dialog_failed", "pick_folder", "Ошибка системного диалога"))
}
pub async fn start_pull(image: String) -> AppResult<u64> {
    let service = service().await?;
    crate::jobs::start(&service.runtime.path, crate::runspec::pull_args(&image)?).await
}
pub async fn start_build(spec: BuildSpec) -> AppResult<u64> {
    let service = service().await?;
    crate::jobs::start(&service.runtime.path, crate::runspec::build_args(&spec)?).await
}
pub async fn start_push(image: String) -> AppResult<u64> {
    let service = service().await?;
    crate::jobs::start(&service.runtime.path, crate::runspec::push_args(&image)?).await
}
pub async fn tag_image(source: String, target: String) -> AppResult<()> {
    service().await?.runtime.tag_image(&source, &target).await
}
pub async fn registry_login(
    server: Option<String>,
    username: String,
    password: String,
) -> AppResult<()> {
    let password = zeroize::Zeroizing::new(password);
    service()
        .await?
        .runtime
        .registry_login(server.as_deref(), &username, &password)
        .await
}
pub async fn registry_logout(server: Option<String>) -> AppResult<()> {
    service()
        .await?
        .runtime
        .registry_logout(server.as_deref())
        .await
}
pub async fn start_save(images: Vec<String>, output: String) -> AppResult<u64> {
    let service = service().await?;
    crate::jobs::start(
        &service.runtime.path,
        crate::runspec::save_args(&images, &output)?,
    )
    .await
}
pub async fn start_load(input: String) -> AppResult<u64> {
    let service = service().await?;
    crate::jobs::start(&service.runtime.path, crate::runspec::load_args(&input)?).await
}
pub async fn start_export(container_id: String, output: String) -> AppResult<u64> {
    let service = service().await?;
    crate::jobs::start(
        &service.runtime.path,
        crate::runspec::export_args(&container_id, &output)?,
    )
    .await
}
pub async fn start_import(input: String, image: Option<String>) -> AppResult<u64> {
    let service = service().await?;
    crate::jobs::start(
        &service.runtime.path,
        crate::runspec::import_args(&input, image.as_deref())?,
    )
    .await
}
pub async fn pick_save_file(
    title: String,
    file_name: String,
    extensions: Vec<String>,
) -> AppResult<Option<String>> {
    tokio::task::spawn_blocking(move || {
        rfd::FileDialog::new()
            .set_title(title)
            .set_file_name(file_name)
            .add_filter("Архив", &extensions)
            .save_file()
            .map(|p| {
                crate::desktop::simplify_path(&p)
                    .to_string_lossy()
                    .into_owned()
            })
    })
    .await
    .map_err(|_| {
        AppError::new(
            "dialog_failed",
            "pick_save_file",
            "Ошибка системного диалога",
        )
    })
}
pub async fn start_volume_backup(volume_name: String, output: String) -> AppResult<u64> {
    let service = service().await?;
    crate::jobs::start(
        &service.runtime.path,
        crate::resources::volume_backup_args(&volume_name, &output)?,
    )
    .await
}
pub async fn start_volume_restore(
    volume_name: String,
    input: String,
    clear: bool,
) -> AppResult<u64> {
    let service = service().await?;
    crate::jobs::start(
        &service.runtime.path,
        crate::resources::volume_restore_args(&volume_name, &input, clear)?,
    )
    .await
}
pub async fn take_job(job: u64) -> AppResult<JobBatch> {
    crate::jobs::take(job).await
}
pub async fn cancel_job(job: u64) {
    crate::jobs::cancel(job).await;
}
pub async fn forget_job(job: u64) {
    crate::jobs::forget(job).await;
}
pub async fn container_stats() -> AppResult<Vec<ContainerStats>> {
    service().await?.runtime.container_stats().await
}
pub async fn list_container_files(container_id: String, path: String) -> AppResult<Vec<FileEntry>> {
    service()
        .await?
        .runtime
        .list_files(&container_id, &path)
        .await
}
pub async fn download_container_file(
    container_id: String,
    path: String,
) -> AppResult<Option<String>> {
    let service = service().await?;
    crate::files::download(&service.runtime.path, &container_id, &path).await
}
pub async fn upload_container_file(
    container_id: String,
    local_path: String,
    target_path: String,
) -> AppResult<()> {
    let service = service().await?;
    crate::files::upload(
        &service.runtime.path,
        &container_id,
        &local_path,
        &target_path,
    )
    .await
}
pub async fn pick_file(title: String) -> AppResult<Option<String>> {
    tokio::task::spawn_blocking(move || {
        rfd::FileDialog::new()
            .set_title(title)
            .pick_file()
            .map(|p| {
                crate::desktop::simplify_path(&p)
                    .to_string_lossy()
                    .into_owned()
            })
    })
    .await
    .map_err(|_| AppError::new("dialog_failed", "pick_file", "Ошибка системного диалога"))
}
pub async fn read_container_text(container_id: String, path: String) -> AppResult<String> {
    let service = service().await?;
    let bytes = crate::files::read(
        &service.runtime.path,
        &container_id,
        &path,
        crate::files::TEXT_LIMIT,
    )
    .await
    .map_err(|mut e| {
        if e.code == "too_large" {
            e.message = "Файл больше 2 МБ — его можно только скачать".into();
        }
        e
    })?;
    crate::files::as_text(bytes)
}
pub async fn write_container_text(
    container_id: String,
    path: String,
    content: String,
) -> AppResult<()> {
    let service = service().await?;
    crate::files::write(
        &service.runtime.path,
        &container_id,
        &path,
        content.as_bytes(),
    )
    .await
}
pub async fn remove_container(container_id: String, force: bool) -> AppResult<()> {
    service().await?.remove(&container_id, force).await
}
pub async fn remove_image(image_id: String) -> AppResult<()> {
    service().await?.runtime.remove_image(&image_id).await
}
pub async fn container_links() -> AppResult<Vec<ContainerLinks>> {
    service().await?.runtime.container_links().await
}
pub async fn create_volume(spec: VolumeSpec) -> AppResult<()> {
    let args = crate::resources::volume_create_args(&spec);
    service()
        .await?
        .runtime
        .run_simple(args, "volume_create")
        .await
}
pub async fn create_network(spec: NetworkSpec) -> AppResult<()> {
    let args = crate::resources::network_create_args(&spec);
    service()
        .await?
        .runtime
        .run_simple(args, "network_create")
        .await
}
pub async fn remove_network(network_name: String) -> AppResult<()> {
    let args = crate::resources::network_remove_args(&network_name);
    service()
        .await?
        .runtime
        .run_simple(args, "network_remove")
        .await
}
pub async fn connect_network(
    network_name: String,
    container_id: String,
    alias: Option<String>,
    ip: Option<String>,
) -> AppResult<()> {
    let args = crate::resources::network_connect_args(
        &network_name,
        &container_id,
        &alias.into_iter().collect::<Vec<_>>(),
        ip.as_deref(),
    );
    service()
        .await?
        .runtime
        .run_simple(args, "network_connect")
        .await
}
pub async fn disconnect_network(network_name: String, container_id: String) -> AppResult<()> {
    let args = crate::resources::network_disconnect_args(&network_name, &container_id);
    service()
        .await?
        .runtime
        .run_simple(args, "network_disconnect")
        .await
}
pub async fn remove_volume(volume_name: String) -> AppResult<()> {
    service().await?.runtime.remove_volume(&volume_name).await
}
pub async fn open_terminal_session(
    container_id: String,
    workdir: Option<String>,
    cols: u16,
    rows: u16,
) -> AppResult<u64> {
    let service = service().await?;
    let cli = service.runtime.path.clone();
    tokio::task::spawn_blocking(move || {
        crate::terminal::open(&cli, &container_id, workdir.as_deref(), cols, rows)
    })
    .await
    .map_err(|_| AppError::new("terminal", "open_terminal", "Не удалось открыть терминал"))?
}
pub async fn take_terminal_output(session: u64) -> AppResult<TerminalOutput> {
    crate::terminal::take(session)
}
pub async fn write_terminal(session: u64, data: Vec<u8>) -> AppResult<()> {
    crate::terminal::write(session, &data)
}
pub async fn resize_terminal(session: u64, cols: u16, rows: u16) -> AppResult<()> {
    crate::terminal::resize(session, cols, rows)
}
pub async fn close_terminal_session(session: u64) {
    tokio::task::spawn_blocking(move || crate::terminal::close(session))
        .await
        .ok();
}
pub async fn open_terminal(container_id: String, workdir: Option<String>) -> AppResult<()> {
    let service = service().await?;
    crate::desktop::open_terminal(&service.runtime.path, &container_id, workdir.as_deref())
}
pub async fn open_port_url(host: String, port: u16, https: bool) -> AppResult<()> {
    crate::desktop::open_url(&host, port, https)
}
/// Turns a Compose file into WSLc steps without running anything.
pub async fn parse_compose(
    yaml: String,
    project_dir: Option<String>,
    project_name: Option<String>,
) -> AppResult<ComposePlan> {
    tokio::task::spawn_blocking(move || {
        let dir = project_dir
            .filter(|d| !d.trim().is_empty())
            .map(|d| crate::desktop::host_path(&d))
            .transpose()?;
        if dir.as_ref().is_some_and(|d| !d.is_dir()) {
            return Err(AppError::new(
                "compose",
                "parse_compose",
                "Папка проекта не найдена",
            ));
        }
        let env = crate::compose::load_env(dir.as_deref());
        crate::compose::plan(&yaml, dir.as_deref(), project_name.as_deref(), &env)
    })
    .await
    .map_err(|_| AppError::new("compose", "parse_compose", "Не удалось разобрать файл"))?
}
/// A Compose file picked in the dialog; at most 1 MB.
pub async fn read_compose_file(path: String) -> AppResult<String> {
    tokio::task::spawn_blocking(move || {
        let path = crate::desktop::host_path(&path)?;
        let failed = || AppError::new("compose", "read_compose_file", "Не удалось прочитать файл");
        if std::fs::metadata(&path).map_err(|_| failed())?.len() > 1 << 20 {
            return Err(AppError::new(
                "compose",
                "read_compose_file",
                "Файл больше 1 МБ",
            ));
        }
        std::fs::read_to_string(&path).map_err(|_| failed())
    })
    .await
    .map_err(|_| AppError::new("compose", "read_compose_file", "Не удалось прочитать файл"))?
}
/// Containers labelled with this Compose project.
pub async fn compose_containers(project: String) -> AppResult<Vec<ComposeContainer>> {
    service().await?.runtime.compose_containers(&project).await
}
pub async fn compose_connect(
    network_name: String,
    container_id: String,
    aliases: Vec<String>,
    ip: Option<String>,
) -> AppResult<()> {
    let args = crate::resources::network_connect_args(
        &network_name,
        &container_id,
        &aliases,
        ip.as_deref(),
    );
    service()
        .await?
        .runtime
        .run_simple(args, "network_connect")
        .await
}
/// Missing bind-mount folders, created as Compose does. Absolute Windows paths only.
pub async fn create_host_folders(paths: Vec<String>) -> AppResult<()> {
    tokio::task::spawn_blocking(move || {
        for path in paths {
            let b = path.as_bytes();
            let drive = b.len() > 2 && b[0].is_ascii_alphabetic() && b[1] == b':' && b[2] == b'\\';
            if !(drive || path.starts_with(r"\\"))
                || path.contains(['"', '\0'])
                || path.contains(r"\..")
            {
                return Err(AppError::new(
                    "compose",
                    "create_host_folders",
                    "Некорректный путь папки",
                ));
            }
            std::fs::create_dir_all(&path).map_err(|e| {
                let mut error =
                    AppError::new("compose", "create_host_folders", "Не удалось создать папку");
                error.diagnostics = Some(format!("{path}: {e}"));
                error
            })?;
        }
        Ok(())
    })
    .await
    .map_err(|_| AppError::new("compose", "create_host_folders", "Не удалось создать папку"))?
}
pub async fn load_settings() -> AppResult<DeskSettings> {
    let path = crate::settings::default_path()?;
    tokio::task::spawn_blocking(move || crate::settings::load_from(&path))
        .await
        .map_err(|_| {
            AppError::new(
                "settings",
                "load_settings",
                "Не удалось прочитать настройки",
            )
        })
}
pub async fn save_settings(settings: DeskSettings) -> AppResult<()> {
    let path = crate::settings::default_path()?;
    tokio::task::spawn_blocking(move || crate::settings::save_to(&path, &settings))
        .await
        .map_err(|_| {
            AppError::new(
                "settings",
                "save_settings",
                "Не удалось сохранить настройки",
            )
        })?
}
/// The `Run` entry exists and starts this executable.
pub async fn autostart_enabled() -> AppResult<bool> {
    let exe = current_exe()?;
    Ok(crate::autostart::enabled_in(
        crate::autostart::RUN_KEY,
        &exe,
    ))
}
pub async fn set_autostart(enabled: bool) -> AppResult<()> {
    let exe = current_exe()?;
    crate::autostart::set_in(crate::autostart::RUN_KEY, &exe, enabled)
}
fn current_exe() -> AppResult<std::path::PathBuf> {
    std::env::current_exe().map_err(|e| {
        let mut error = AppError::new(
            "autostart",
            "current_exe",
            "Не удалось определить путь приложения",
        );
        error.diagnostics = Some(e.to_string());
        error
    })
}
pub async fn open_host_folder(path: String) -> AppResult<()> {
    tokio::task::spawn_blocking(move || crate::desktop::open_folder(&path))
        .await
        .map_err(|_| AppError::new("shell", "open_folder", "Не удалось открыть проводник"))?
}
pub async fn subscribe_logs(container_id: String, options: LogOptions) -> AppResult<u64> {
    let service = service().await?;
    service
        .runtime
        .inspect_container(&container_id, false)
        .await?;
    crate::log_readers::subscribe(&service.runtime.path, &container_id, &options).await
}
pub async fn take_log_batch(subscription_id: u64) -> AppResult<LogBatch> {
    crate::log_readers::take(subscription_id).await
}
pub async fn unsubscribe_logs(subscription_id: u64) {
    crate::log_readers::unsubscribe(subscription_id).await;
}
pub async fn shutdown_readers() {
    crate::jobs::shutdown().await;
    crate::terminal::shutdown();
    crate::events::stop(true).await;
    crate::log_readers::shutdown().await;
}

/// ConsoleCrypt ends the backend lease when its vault/profile is closed.
pub async fn disconnect() {
    shutdown_readers().await;
    *core().write().await = None;
}
pub async fn start_events() -> AppResult<()> {
    let service = service().await?;
    crate::events::start(&service.runtime.path).await
}
pub async fn take_events() -> EventBatch {
    crate::events::take().await
}
pub async fn stop_events() {
    crate::events::stop(false).await;
}

pub async fn save_loaded_logs(text: String) -> AppResult<Option<String>> {
    if text.len() > crate::logs::MAX_BYTES {
        return Err(AppError::new(
            "log_limit",
            "save_logs",
            "Буфер логов превышает 10 MiB",
        ));
    }
    tokio::task::spawn_blocking(move || {
        let Some(path) = rfd::FileDialog::new()
            .set_title("Сохранить загруженные логи")
            .set_file_name("container.log")
            .add_filter("Логи", &["log"])
            .save_file()
        else {
            return Ok(None);
        };
        std::fs::write(&path, text).map_err(|_| {
            AppError::new(
                "save_failed",
                "save_logs",
                "Не удалось сохранить файл логов",
            )
        })?;
        Ok(Some(path.to_string_lossy().into()))
    })
    .await
    .map_err(|_| AppError::new("dialog_failed", "save_logs", "Ошибка системного диалога"))?
}
