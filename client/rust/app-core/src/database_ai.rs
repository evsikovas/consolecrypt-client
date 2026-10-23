//! Database assistant: compose, explain, fix, optimize and translate queries
//! with the AI providers configured in ConsoleCrypt. The model never runs
//! anything: its query comes back as a draft for the editor. Schema, selected
//! rows, row samples and read-only probes reach the model only with the
//! consent given for that request; every text passes the sanitizer, and the
//! Strict profile replaces database identifiers with placeholders that are
//! restored in the answer.
use crate::{AppCore, AppError, AppResult};
use cc_ai_core::{
    provider::{ChatRequest, Message, ToolCall, ToolDefinition},
    sanitizer::Category,
    LlmProvider, PrivacyProfile, SanitizerSession,
};
use cc_db_core::{DbCell, DbObjectRef, DbQueryRequest, DbSession};
use cc_models::database::DbEngine;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio_util::sync::CancellationToken;

const REVIEW_TTL: Duration = Duration::from_secs(120);
const MAX_TOOL_ROUNDS: usize = 5;
const MAX_TURNS: usize = 10;
const MAX_TEXT: usize = 64 * 1024;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DbAiRequest {
    pub action: String,
    pub question: String,
    #[serde(default)]
    pub query: Option<String>,
    #[serde(default)]
    pub error_code: Option<String>,
    #[serde(default)]
    pub plan: Option<Value>,
    #[serde(default)]
    pub object: Option<DbObjectRef>,
    #[serde(default)]
    pub target_engine: Option<DbEngine>,
    #[serde(default)]
    pub conversation_id: Option<String>,
    #[serde(default)]
    pub query_id: Option<String>,
    #[serde(default)]
    pub result_index: Option<u32>,
    #[serde(default)]
    pub selected_rows: Vec<u32>,
    #[serde(default = "default_limit")]
    pub context_limit: u32,
}

fn default_limit() -> u32 {
    12
}

#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DbAiConsent {
    #[serde(default)]
    pub schema: bool,
    #[serde(default)]
    pub selected_rows: bool,
    #[serde(default)]
    pub sample_rows: bool,
    #[serde(default)]
    pub run_readonly: bool,
}

#[derive(Debug, Serialize)]
pub struct DbAiProviderDto {
    pub id: String,
    pub name: String,
    /// Sent outside this device and the local network.
    pub external: bool,
}

#[derive(Debug, Serialize)]
pub struct DbAiReviewDto {
    pub ticket: String,
    pub provider: DbAiProviderDto,
    pub connection_name: String,
    pub expires_in_seconds: u64,
    /// Objects whose structure would be sent with schema consent.
    pub schema_objects: Vec<String>,
    pub has_selected_rows: bool,
    pub history_messages: usize,
}

struct Pending {
    session_id: String,
    provider_id: String,
    request: DbAiRequest,
    expires: Instant,
}

/// Events of one running request, drained by polling.
struct Job {
    events: Mutex<VecDeque<Value>>,
    notify: tokio::sync::Notify,
    cancel: CancellationToken,
}

#[derive(Clone)]
struct Turn {
    user: String,
    assistant: String,
}

#[derive(Default)]
pub(crate) struct DbAiRuntime {
    pending: Mutex<HashMap<String, Pending>>,
    jobs: Mutex<HashMap<String, Arc<Job>>>,
    /// Conversations per session: id → earlier turns (native memory only).
    conversations: Mutex<HashMap<String, (String, Vec<Turn>)>>,
}

impl DbAiRuntime {
    pub(crate) fn close(&self) {
        self.pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
        for (_, job) in self.jobs.lock().unwrap_or_else(|e| e.into_inner()).drain() {
            job.cancel.cancel();
        }
        self.conversations
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
    }
}

fn invalid(reason: &'static str) -> AppError {
    AppError::invalid("database_ai", reason)
}

fn dialect(engine: DbEngine) -> &'static str {
    match engine {
        DbEngine::Postgresql => "PostgreSQL SQL",
        DbEngine::Mysql => "MySQL SQL",
        DbEngine::Mariadb => "MariaDB SQL",
        DbEngine::Clickhouse => "ClickHouse SQL",
        DbEngine::Redis => "Redis commands (one command per line, redis-cli syntax)",
        DbEngine::Mongodb => "MongoDB shell syntax (db.collection.find(...) / aggregate([...]))",
        DbEngine::Cassandra => {
            "CQL for Apache Cassandra / ScyllaDB (restrict the full partition key, no joins, \
             avoid ALLOW FILTERING; batches as BEGIN BATCH … APPLY BATCH)"
        }
        DbEngine::Unknown => "SQL",
    }
}

fn fence(engine: DbEngine) -> &'static str {
    match engine {
        DbEngine::Redis => "redis",
        DbEngine::Mongodb => "javascript",
        DbEngine::Cassandra => "cql",
        _ => "sql",
    }
}

const SYSTEM: &str = "You are the database assistant inside ConsoleCrypt. You help write, \
explain, fix and optimize queries. You never execute anything: the user reviews and runs \
your query. When you propose a query, put exactly one fenced code block with it, using the \
requested dialect, and keep the explanation short. Use only tables and columns you were \
shown or found with tools; if something is unknown, say so instead of guessing. Placeholders \
like <DB_3> stand for real names: keep them exactly as written. Answer in the language of \
the user's question.";

fn tools(consent: DbAiConsent) -> Vec<ToolDefinition> {
    let mut tools = vec![
        ToolDefinition {
            name: "find_tables".into(),
            description: "Tables whose name contains the text.".into(),
            parameters: json!({"type":"object","properties":{"query":{"type":"string"}},"required":["query"]}),
        },
        ToolDefinition {
            name: "describe_table".into(),
            description: "Columns and types of one table (no data).".into(),
            parameters: json!({"type":"object","properties":{"name":{"type":"string"}},"required":["name"]}),
        },
        ToolDefinition {
            name: "explain".into(),
            description: "Query plan without executing the query (checks syntax and cost).".into(),
            parameters: json!({"type":"object","properties":{"sql":{"type":"string"}},"required":["sql"]}),
        },
    ];
    if consent.sample_rows {
        tools.push(ToolDefinition {
            name: "sample_rows".into(),
            description: "Up to 20 rows of a table.".into(),
            parameters: json!({"type":"object","properties":{"table":{"type":"string"},"n":{"type":"integer","maximum":20}},"required":["table"]}),
        });
    }
    if consent.run_readonly {
        tools.push(ToolDefinition {
            name: "run_readonly".into(),
            description: "Run one read-only query and see up to 50 rows (10 s limit).".into(),
            parameters: json!({"type":"object","properties":{"sql":{"type":"string"},"limit":{"type":"integer","maximum":50}},"required":["sql"]}),
        });
    }
    tools
}

/// Rows as a small Markdown table for the model.
fn markdown(columns: &[cc_db_core::DbColumn], rows: &[Vec<DbCell>]) -> String {
    let cell = |cell: &DbCell| match cell {
        DbCell::Null => "NULL".to_owned(),
        DbCell::Bool(value) => value.to_string(),
        DbCell::Int(v)
        | DbCell::Uint(v)
        | DbCell::Decimal(v)
        | DbCell::Float(v)
        | DbCell::Text(v)
        | DbCell::Date(v)
        | DbCell::Time(v)
        | DbCell::Timestamp(v)
        | DbCell::Binary(v)
        | DbCell::Json(v) => v
            .chars()
            .take(200)
            .collect::<String>()
            .replace('|', "\\|")
            .replace('\n', " "),
    };
    let mut out = format!(
        "| {} |\n|{}\n",
        columns
            .iter()
            .map(|c| c.name.as_str())
            .collect::<Vec<_>>()
            .join(" | "),
        columns.iter().map(|_| " --- |").collect::<String>()
    );
    for row in rows {
        // Sensitive columns (passwords, tokens…) never leave the device.
        let cells: Vec<String> = row
            .iter()
            .zip(columns)
            .map(|(value, column)| {
                if column.sensitive {
                    "[hidden]".into()
                } else {
                    cell(value)
                }
            })
            .collect();
        out.push_str(&format!("| {} |\n", cells.join(" | ")));
    }
    out
}

/// The first fenced code block and the text around it.
fn split_answer(answer: &str) -> (Option<String>, String) {
    let Some(start) = answer.find("```") else {
        return (None, answer.trim().to_owned());
    };
    let after = &answer[start + 3..];
    let body_start = after.find('\n').map_or(0, |i| i + 1);
    let Some(end) = after[body_start..].find("```") else {
        return (None, answer.trim().to_owned());
    };
    let code = after[body_start..body_start + end].trim().to_owned();
    let rest = format!("{}{}", &answer[..start], &after[body_start + end + 3..]);
    ((!code.is_empty()).then_some(code), rest.trim().to_owned())
}

fn is_ddl(text: &str) -> bool {
    let head = text.trim_start().to_ascii_uppercase();
    [
        "CREATE ",
        "ALTER ",
        "DROP ",
        "TRUNCATE ",
        "RENAME ",
        "COMMENT ON",
    ]
    .iter()
    .any(|keyword| head.starts_with(keyword))
}

struct Context<'a> {
    session: Arc<DbSession>,
    provider: Arc<dyn LlmProvider>,
    sanitizer: SanitizerSession,
    consent: DbAiConsent,
    job: &'a Job,
    engine: DbEngine,
}

impl Context<'_> {
    fn emit(&self, event: Value) {
        self.job
            .events
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push_back(event);
        self.job.notify.notify_one();
    }

    fn strict(&self) -> bool {
        self.sanitizer.profile() == PrivacyProfile::Strict
    }

    /// Registers identifiers so Strict replaces them with placeholders.
    fn pseudonymize(&mut self, names: impl IntoIterator<Item = String>) {
        if self.strict() {
            for name in names {
                self.sanitizer.add_literal(Category::Database, &name);
            }
        }
    }

    async fn tool(&mut self, call: &ToolCall) -> String {
        let text = |key: &str| {
            call.arguments
                .get(key)
                .and_then(Value::as_str)
                .unwrap_or_default()
                .chars()
                .take(MAX_TEXT)
                .collect::<String>()
        };
        let restore =
            |sanitizer: &SanitizerSession, value: String| sanitizer.rehydrate(&value).text;
        let shown = match call.name.as_str() {
            "find_tables" | "describe_table" => text(if call.name == "find_tables" {
                "query"
            } else {
                "name"
            }),
            "sample_rows" => text("table"),
            _ => String::new(),
        };
        self.emit(
            json!({"kind": "tool", "tool": call.name, "content": restore(&self.sanitizer, shown)}),
        );
        let result: Result<String, cc_db_core::DbError> = async {
            match call.name.as_str() {
                "find_tables" => {
                    let query = restore(&self.sanitizer, text("query"));
                    Ok(self.session.find_tables(&query, 50).await?.join("\n"))
                }
                "describe_table" => {
                    let name = restore(&self.sanitizer, text("name"));
                    let short = name.rsplit('.').next().unwrap_or(&name).to_owned();
                    let lines = self.session.schema_context(&short, 1).await?;
                    Ok(lines
                        .into_iter()
                        .find(|line| line.contains(&format!(".{short} (")))
                        .unwrap_or_else(|| "not found".into()))
                }
                "explain" => {
                    let sql = restore(&self.sanitizer, text("sql"));
                    let plan = self
                        .session
                        .explain(
                            &DbQueryRequest {
                                text: sql,
                                parameters: Vec::new(),
                                approval_ticket: None,
                            },
                            false,
                        )
                        .await?;
                    Ok(plan["plan"].to_string().chars().take(16 * 1024).collect())
                }
                "sample_rows" if self.consent.sample_rows => {
                    let table = restore(&self.sanitizer, text("table"));
                    let n = call
                        .arguments
                        .get("n")
                        .and_then(Value::as_u64)
                        .unwrap_or(10)
                        .clamp(1, 20);
                    let reference = self
                        .session
                        .find_tables(table.rsplit('.').next().unwrap_or(&table), 1)
                        .await?
                        .into_iter()
                        .next()
                        .ok_or(cc_db_core::DbError::InvalidRequest)?;
                    let (container, name) = reference.split_once('.').unwrap_or(("", &reference));
                    let object = DbObjectRef {
                        database: container.to_owned(),
                        schema: Some(container.to_owned()),
                        kind: cc_db_core::DbObjectKind::Table,
                        name: name.to_owned(),
                        parent_name: None,
                    };
                    let sql = self.session.object_query(&object, n as u32).await?;
                    let (columns, rows) = tokio::time::timeout(
                        Duration::from_secs(10),
                        self.session.read_rows(
                            &DbQueryRequest {
                                text: sql,
                                parameters: Vec::new(),
                                approval_ticket: None,
                            },
                            n as usize,
                        ),
                    )
                    .await
                    .map_err(|_| cc_db_core::DbError::Timeout)??;
                    Ok(markdown(&columns, &rows))
                }
                "run_readonly" if self.consent.run_readonly => {
                    let sql = restore(&self.sanitizer, text("sql"));
                    let limit = call
                        .arguments
                        .get("limit")
                        .and_then(Value::as_u64)
                        .unwrap_or(20)
                        .clamp(1, 50);
                    let (columns, rows) = tokio::time::timeout(
                        Duration::from_secs(10),
                        self.session.read_rows(
                            &DbQueryRequest {
                                text: sql,
                                parameters: Vec::new(),
                                approval_ticket: None,
                            },
                            limit as usize,
                        ),
                    )
                    .await
                    .map_err(|_| cc_db_core::DbError::Timeout)??;
                    Ok(markdown(&columns, &rows))
                }
                _ => Err(cc_db_core::DbError::PolicyDenied),
            }
        }
        .await;
        match result {
            Ok(output) => output,
            Err(cc_db_core::DbError::PolicyDenied) => {
                "refused: not allowed for this connection or consent".into()
            }
            Err(_) => "failed".into(),
        }
    }
}

impl AppCore {
    /// Providers configured in ConsoleCrypt (default first).
    pub async fn db_ai_providers(&self) -> AppResult<Vec<DbAiProviderDto>> {
        let (_, unlocked) = self.unlocked().await?;
        let ai = unlocked.ai.clone();
        let mut configs = ai.working().ai_providers();
        configs.sort_by_key(|config| !config.is_default);
        if let Ok(local) = ai.resolve_config(Some(crate::ai::DEVICE_LOCAL_PROVIDER_ID)) {
            configs.insert(0, local);
        }
        Ok(configs
            .into_iter()
            .map(|config| DbAiProviderDto {
                external: cc_ai_core::ProviderSettings::from_config(&config)
                    .map(|settings| settings.locality() == cc_ai_core::provider::Locality::Public)
                    .unwrap_or(true),
                id: config.id.to_string(),
                name: config.name,
            })
            .collect())
    }

    /// Stages a request and lists what consent would send.
    pub async fn db_ai_prepare(
        &self,
        session_id: String,
        provider_id: String,
        request: DbAiRequest,
    ) -> AppResult<DbAiReviewDto> {
        let (_, unlocked) = self.unlocked().await?;
        let session = unlocked.db.session(&session_id)?;
        crate::teams::check_ai_allowed(unlocked.working(), &session.info().connection_id)?;
        if request.question.len() > MAX_TEXT
            || request.query.as_ref().is_some_and(|q| q.len() > MAX_TEXT)
        {
            return Err(invalid("request_too_large"));
        }
        let config = unlocked.ai.resolve_config(Some(&provider_id))?;
        let external = cc_ai_core::ProviderSettings::from_config(&config)
            .map(|settings| settings.locality() == cc_ai_core::provider::Locality::Public)
            .unwrap_or(true);
        let hints = format!(
            "{} {}",
            request.question,
            request.query.clone().unwrap_or_default()
        );
        let schema_objects = session
            .schema_context(&hints, request.context_limit as usize)
            .await
            .unwrap_or_default()
            .into_iter()
            .filter_map(|line| {
                line.split(' ')
                    .nth(1)
                    .map(|name| name.split('(').next().unwrap_or(name).to_owned())
            })
            .collect();
        let connection_id = crate::dto::parse_id("connection_id", &session.info().connection_id)?;
        let connection_name = self
            .db_saved_connection(connection_id)
            .await?
            .map(|connection| connection.name)
            .unwrap_or_default();
        let history_messages = request
            .conversation_id
            .as_ref()
            .and_then(|id| {
                unlocked
                    .db_ai
                    .conversations
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .get(id)
                    .filter(|(owner, _)| owner == &session_id)
                    .map(|(_, turns)| turns.len() * 2)
            })
            .unwrap_or(0);
        let ticket = uuid::Uuid::new_v4().to_string();
        let has_selected_rows = request.query_id.is_some() && !request.selected_rows.is_empty();
        let mut pending = unlocked
            .db_ai
            .pending
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let now = Instant::now();
        pending.retain(|_, item| item.expires > now);
        if pending.len() >= 16 {
            return Err(invalid("too_many_requests"));
        }
        pending.insert(
            ticket.clone(),
            Pending {
                session_id,
                provider_id: provider_id.clone(),
                request,
                expires: now + REVIEW_TTL,
            },
        );
        Ok(DbAiReviewDto {
            ticket,
            provider: DbAiProviderDto {
                id: provider_id,
                name: config.name,
                external,
            },
            connection_name,
            expires_in_seconds: REVIEW_TTL.as_secs(),
            schema_objects,
            has_selected_rows,
            history_messages,
        })
    }

    /// Starts the staged request with the consent the user gave for it.
    pub async fn db_ai_ask(
        &self,
        session_id: String,
        ticket: String,
        consent: DbAiConsent,
    ) -> AppResult<()> {
        let (_, unlocked) = self.unlocked().await?;
        let pending = unlocked
            .db_ai
            .pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&ticket)
            .filter(|pending| pending.session_id == session_id && pending.expires > Instant::now())
            .ok_or_else(|| invalid("review_expired"))?;
        let session = unlocked.db.session(&session_id)?;
        crate::teams::check_ai_allowed(unlocked.working(), &session.info().connection_id)?;
        let config = unlocked.ai.resolve_config(Some(&pending.provider_id))?;
        let provider = unlocked.ai.provider_for(&config).await?;
        // Rows chosen in the grid travel only with their own consent.
        let selected = match (&pending.request.query_id, consent.selected_rows) {
            (Some(query_id), true) if !pending.request.selected_rows.is_empty() => self
                .db_copy_result(
                    query_id.clone(),
                    pending.request.result_index.unwrap_or(0),
                    Some(
                        pending
                            .request
                            .selected_rows
                            .iter()
                            .copied()
                            .take(50)
                            .collect(),
                    ),
                    None,
                    serde_json::from_value(json!({"format": "markdown"}))
                        .map_err(|_| invalid("invalid_request"))?,
                )
                .await
                .ok(),
            _ => None,
        };
        let history = pending
            .request
            .conversation_id
            .as_ref()
            .and_then(|id| {
                unlocked
                    .db_ai
                    .conversations
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .get(id)
                    .filter(|(owner, _)| owner == &session_id)
                    .map(|(_, turns)| turns.clone())
            })
            .unwrap_or_default();
        let conversation_id = pending
            .request
            .conversation_id
            .clone()
            .filter(|_| !history.is_empty())
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let job = Arc::new(Job {
            events: Mutex::new(VecDeque::new()),
            notify: tokio::sync::Notify::new(),
            cancel: unlocked.db.cancellation().child_token(),
        });
        unlocked
            .db_ai
            .jobs
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(ticket.clone(), job.clone());
        let runtime = unlocked.clone();
        let profile = provider.privacy_profile();
        tokio::spawn(async move {
            let cancel = job.cancel.clone();
            let mut context = Context {
                engine: session.info().engine,
                session,
                provider,
                sanitizer: SanitizerSession::new(profile),
                consent,
                job: &job,
            };
            let run = run(&mut context, &pending, selected, &history);
            let outcome = tokio::select! {
                biased;
                _ = cancel.cancelled() => Err("cancelled"),
                outcome = run => outcome,
            };
            match outcome {
                Ok((question, answer)) => {
                    let mut conversations = runtime
                        .db_ai
                        .conversations
                        .lock()
                        .unwrap_or_else(|e| e.into_inner());
                    let entry = conversations
                        .entry(conversation_id.clone())
                        .or_insert_with(|| (pending.session_id.clone(), Vec::new()));
                    entry.1.push(Turn {
                        user: question,
                        assistant: answer,
                    });
                    if entry.1.len() > MAX_TURNS {
                        entry.1.remove(0);
                    }
                    drop(conversations);
                    context
                        .emit(json!({"kind": "conversation", "conversation_id": conversation_id}));
                    context.emit(json!({"kind": "done"}));
                }
                Err(code) => context.emit(json!({"kind": "error", "code": code})),
            }
        });
        Ok(())
    }

    /// Next events of a running request; an empty list after `wait_ms`.
    pub async fn db_ai_poll(&self, ticket: String, wait_ms: u64) -> AppResult<Vec<Value>> {
        let (_, unlocked) = self.unlocked().await?;
        let job = unlocked
            .db_ai
            .jobs
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&ticket)
            .cloned()
            .ok_or_else(|| invalid("request_closed"))?;
        let take = || -> Vec<Value> {
            job.events
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .drain(..)
                .collect()
        };
        let mut events = take();
        if events.is_empty() {
            let _ = tokio::time::timeout(
                Duration::from_millis(wait_ms.min(2000)),
                job.notify.notified(),
            )
            .await;
            events = take();
        }
        if events
            .iter()
            .any(|event| matches!(event["kind"].as_str(), Some("done" | "error")))
        {
            unlocked
                .db_ai
                .jobs
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(&ticket);
        }
        Ok(events)
    }

    pub async fn db_ai_cancel(&self, ticket: String) -> AppResult<()> {
        let (_, unlocked) = self.unlocked().await?;
        unlocked
            .db_ai
            .pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&ticket);
        if let Some(job) = unlocked
            .db_ai
            .jobs
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&ticket)
        {
            job.cancel.cancel();
        }
        Ok(())
    }

    /// Forgets this session's conversations and stops its running requests.
    pub async fn db_ai_revoke(&self, session_id: String) -> AppResult<()> {
        let (_, unlocked) = self.unlocked().await?;
        unlocked
            .db_ai
            .conversations
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .retain(|_, (owner, _)| owner != &session_id);
        unlocked
            .db_ai
            .pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .retain(|_, pending| pending.session_id != session_id);
        Ok(())
    }
}

/// The instruction for one action, in plain words for the model.
fn instruction(request: &DbAiRequest, engine: DbEngine) -> String {
    let target = request.target_engine.map_or(dialect(engine), dialect);
    match request.action.as_str() {
        "explain" => {
            "Explain what this query does in simple words: tables, filters, result, risks.".into()
        }
        "fix" => format!(
            "This query failed{}. Find the cause and return the corrected query.",
            request
                .error_code
                .as_deref()
                .map(|code| format!(" with error code {code}"))
                .unwrap_or_default()
        ),
        "optimize" => {
            "Make this query faster using the plan if given: rewrite it and suggest indexes \
            as separate statements."
                .into()
        }
        "translate" => format!("Translate this query to {target}, keeping its meaning."),
        "snippet" => {
            "Turn this query into a reusable snippet: replace literal values that a user would \
            change with {{name}} variables."
                .into()
        }
        "examples" => {
            "Suggest typical useful queries for this object: top rows, duplicates, missing \
            values, value distributions. Put all of them in one code block."
                .into()
        }
        "result" => "Explain what the selected result rows show.".into(),
        _ => "Write a query for the request.".into(),
    }
}

async fn run(
    context: &mut Context<'_>,
    pending: &Pending,
    selected: Option<String>,
    history: &[Turn],
) -> Result<(String, String), &'static str> {
    let request = &pending.request;
    let engine = context.engine;
    let production = context.session.info().production;
    // What the model may see, gathered first so Strict can pseudonymize it.
    let mut schema = Vec::new();
    if context.consent.schema {
        let hints = format!(
            "{} {}",
            request.question,
            request.query.clone().unwrap_or_default()
        );
        schema = context
            .session
            .schema_context(&hints, request.context_limit.clamp(1, 100) as usize)
            .await
            .unwrap_or_default();
        let objects: Vec<String> = schema
            .iter()
            .filter_map(|line| {
                line.split(' ')
                    .nth(1)
                    .map(|name| name.split('(').next().unwrap_or(name).to_owned())
            })
            .collect();
        context.emit(json!({"kind": "context", "content": objects.join(", ")}));
        // Identifiers of the shown tables and columns become placeholders under Strict.
        let mut names = Vec::new();
        for line in &schema {
            if let Some((head, columns)) = line.split_once(" (") {
                if let Some(qualified) = head.split(' ').nth(1) {
                    names.extend(qualified.split('.').map(str::to_owned));
                }
                names.extend(
                    columns
                        .trim_end_matches(')')
                        .split(", ")
                        .filter_map(|column| column.split(' ').next().map(str::to_owned)),
                );
            }
        }
        names.retain(|name| name.len() > 2);
        context.pseudonymize(names);
    }
    let mut prompt = format!(
        "Dialect: {}. Connection: {}.\nTask: {}\n",
        dialect(engine),
        if production {
            "production (read-only; changes are never run there)"
        } else {
            "development"
        },
        instruction(request, engine)
    );
    if let Some(query) = request
        .query
        .as_deref()
        .filter(|query| !query.trim().is_empty())
    {
        prompt.push_str(&format!(
            "\nCurrent query:\n```{}\n{}\n```\n",
            fence(engine),
            query.trim()
        ));
    }
    if let Some(plan) = &request.plan {
        prompt.push_str(&format!(
            "\nQuery plan:\n{}\n",
            plan.to_string().chars().take(16 * 1024).collect::<String>()
        ));
    }
    if !schema.is_empty() {
        prompt.push_str("\nSchema (no data):\n");
        for line in &schema {
            prompt.push_str(line);
            prompt.push('\n');
        }
    }
    if let Some(rows) = &selected {
        prompt.push_str(&format!("\nSelected result rows:\n{rows}\n"));
    }
    prompt.push_str(&format!("\nRequest: {}\n", request.question.trim()));
    let mut messages = vec![Message::system(context.sanitizer.sanitize(SYSTEM))];
    for turn in history {
        messages.push(Message::user(context.sanitizer.sanitize(&turn.user)));
        messages.push(Message::assistant(
            context.sanitizer.sanitize(&turn.assistant),
        ));
    }
    messages.push(Message::user(context.sanitizer.sanitize(&prompt)));
    let use_tools = context.consent.schema && context.provider.capabilities().tool_calling;
    let mut answer = String::new();
    for round in 0..=MAX_TOOL_ROUNDS {
        let mut chat = ChatRequest::new(messages.clone());
        chat.temperature = Some(0.2);
        if use_tools && round < MAX_TOOL_ROUNDS {
            chat.tools = tools(context.consent);
        }
        let response = context
            .provider
            .chat(&chat)
            .await
            .map_err(|_| "provider_failed")?;
        if response.tool_calls.is_empty() || !use_tools {
            answer = response.content;
            break;
        }
        messages.push(Message::assistant_tool_calls(
            context.sanitizer.sanitize(&response.content),
            response.tool_calls.clone(),
        ));
        for call in response.tool_calls.iter().take(8) {
            let output = context.tool(call).await;
            messages.push(Message::tool_result(
                call.id.clone(),
                context.sanitizer.sanitize(&output),
            ));
        }
    }
    let answer = context.sanitizer.rehydrate(&answer).text;
    let (draft, explanation) = split_answer(&answer);
    if let Some(draft) = draft {
        context.emit(json!({"kind": "draft", "content": draft, "is_ddl": is_ddl(&draft)}));
    }
    if !explanation.is_empty() {
        context.emit(json!({"kind": "explanation", "content": explanation}));
    }
    Ok((
        format!(
            "{}\n{}",
            request.question,
            request.query.clone().unwrap_or_default()
        ),
        answer,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn answers_split_into_draft_and_explanation() {
        let (draft, rest) = split_answer("Here:\n```sql\nSELECT 1;\n```\nDone.");
        assert_eq!(draft.as_deref(), Some("SELECT 1;"));
        assert_eq!(rest, "Here:\n\nDone.");
        assert_eq!(split_answer("No code").0, None);
        assert!(is_ddl("  create index i on t (a)"));
        assert!(!is_ddl("SELECT 1"));
    }

    /// Scripted model: asks for one tool, then answers with placeholders it saw.
    #[derive(Debug)]
    struct Scripted {
        settings: cc_ai_core::ProviderSettings,
        seen: Mutex<Vec<String>>,
        tool: Option<(&'static str, Value)>,
    }

    #[async_trait::async_trait]
    impl LlmProvider for Scripted {
        fn settings(&self) -> &cc_ai_core::ProviderSettings {
            &self.settings
        }
        fn capabilities(&self) -> cc_ai_core::Capabilities {
            cc_ai_core::Capabilities {
                chat: true,
                responses: false,
                embeddings: false,
                streaming: false,
                tool_calling: true,
                structured_output: cc_ai_core::provider::StructuredOutput::None,
            }
        }
        async fn list_models(&self) -> cc_ai_core::Result<Vec<cc_ai_core::provider::ModelInfo>> {
            Ok(Vec::new())
        }
        async fn chat(
            &self,
            req: &ChatRequest,
        ) -> cc_ai_core::Result<cc_ai_core::provider::ChatResponse> {
            let text: Vec<String> = req
                .messages
                .iter()
                .map(|m| m.content().as_str().to_owned())
                .collect();
            let all = text.join("\n");
            self.seen.lock().unwrap().push(all.clone());
            let first_call = !req
                .messages
                .iter()
                .any(|m| m.role == cc_ai_core::provider::Role::Tool);
            if let (Some((name, arguments)), true) = (&self.tool, first_call) {
                return Ok(cc_ai_core::provider::ChatResponse {
                    tool_calls: vec![ToolCall {
                        id: "1".into(),
                        name: (*name).into(),
                        arguments: arguments.clone(),
                    }],
                    ..Default::default()
                });
            }
            // Echo the placeholder the model was given for the table name.
            let table = all
                .lines()
                .find_map(|line| line.strip_prefix("table "))
                .and_then(|rest| rest.split(" (").next())
                .and_then(|qualified| qualified.rsplit('.').next())
                .unwrap_or("cc_ai_orders")
                .to_owned();
            Ok(cc_ai_core::provider::ChatResponse {
                content: format!(
                    "Use this:\n```sql\nSELECT count(*) FROM {table};\n```\nCounts rows."
                ),
                ..Default::default()
            })
        }
        async fn chat_stream(
            &self,
            _: &ChatRequest,
        ) -> cc_ai_core::Result<cc_ai_core::provider::ChatStream> {
            Err(cc_ai_core::AiError::Unsupported("stream"))
        }
        async fn embed(
            &self,
            _: &cc_ai_core::provider::EmbeddingRequest,
        ) -> cc_ai_core::Result<cc_ai_core::provider::EmbeddingResponse> {
            Err(cc_ai_core::AiError::Unsupported("embed"))
        }
    }

    fn scripted(tool: Option<(&'static str, Value)>) -> Arc<Scripted> {
        let now = chrono::Utc::now();
        let config = cc_models::ai::AiProviderConfig {
            id: cc_models::ObjectId::new(),
            name: "mock".into(),
            provider: cc_models::ai::AiProviderKind::OpenaiCompatible,
            base_url: "https://llm.example.com/v1".into(),
            api_key_secret_id: None,
            chat_model: "m".into(),
            embedding_model: None,
            timeout_secs: 10,
            streaming: false,
            tool_support: true,
            privacy_profile: PrivacyProfile::Strict,
            is_default: true,
            created_at: now,
            updated_at: now,
        };
        Arc::new(Scripted {
            settings: cc_ai_core::ProviderSettings::from_config(&config).unwrap(),
            seen: Mutex::new(Vec::new()),
            tool,
        })
    }

    /// Opt-in: CC_DB_LIVE_PG=host:port:user:password:database.
    async fn live_session(production: bool) -> Option<Arc<DbSession>> {
        let spec = std::env::var("CC_DB_LIVE_PG").ok()?;
        let parts: Vec<&str> = spec.splitn(5, ':').collect();
        let mut connection =
            cc_models::database::DbConnection::new("ai", parts[0], DbEngine::Postgresql);
        connection.port = parts[1].parse().unwrap();
        connection.username = parts[2].into();
        connection.database = parts[4].into();
        connection.production = production;
        connection.tls.mode = cc_models::database::DbTlsMode::Disable;
        let credentials = cc_db_core::transport::DbCredentials {
            password: Some(secrecy::SecretString::from(parts[3].to_owned())),
            client_key_pem: None,
        };
        Some(Arc::new(
            DbSession::open(
                "ai".into(),
                &connection,
                &credentials,
                Arc::new(cc_db_core::transport::DirectConnector),
                false,
            )
            .await
            .unwrap(),
        ))
    }

    async fn ask(
        session: Arc<DbSession>,
        provider: Arc<Scripted>,
        consent: DbAiConsent,
    ) -> Vec<Value> {
        let job = Job {
            events: Mutex::new(VecDeque::new()),
            notify: tokio::sync::Notify::new(),
            cancel: CancellationToken::new(),
        };
        let profile = provider.privacy_profile();
        let mut context = Context {
            engine: DbEngine::Postgresql,
            session,
            provider,
            sanitizer: SanitizerSession::new(profile),
            consent,
            job: &job,
        };
        let pending = Pending {
            session_id: "ai".into(),
            provider_id: String::new(),
            request: serde_json::from_value(
                json!({"action": "compose", "question": "how many cc_ai_orders are there?"}),
            )
            .unwrap(),
            expires: Instant::now(),
        };
        run(&mut context, &pending, None, &[]).await.unwrap();
        let events: Vec<Value> = job.events.lock().unwrap().drain(..).collect();
        events
    }

    #[tokio::test]
    async fn assistant_sees_pseudonymized_schema_only_with_consent_and_never_runs_drafts() {
        let Some(session) = live_session(false).await else {
            eprintln!("CC_DB_LIVE_PG not set; skipped");
            return;
        };
        let setup = |sql: &str| DbQueryRequest {
            text: sql.into(),
            parameters: Vec::new(),
            approval_ticket: None,
        };
        for sql in [
            "DROP TABLE IF EXISTS cc_ai_orders",
            "CREATE TABLE cc_ai_orders (id int primary key, customer_secret_note text)",
            "INSERT INTO cc_ai_orders VALUES (1, 'top-secret-value')",
        ] {
            let mut request = setup(sql);
            let review = session.guard().review(&request).unwrap();
            session.guard().approve(&review.ticket).unwrap();
            request.approval_ticket = Some(review.ticket.clone());
            let mut stream = session.execute(&request).unwrap();
            while let Some(event) = stream.events.recv().await {
                if matches!(
                    event,
                    cc_db_core::DbQueryEvent::Done | cc_db_core::DbQueryEvent::Error { .. }
                ) {
                    break;
                }
            }
        }
        // Without consent: no schema, no tools, nothing about the table.
        let provider = scripted(None);
        let events = ask(session.clone(), provider.clone(), DbAiConsent::default()).await;
        let seen = provider.seen.lock().unwrap().join("\n");
        assert!(!seen.contains("customer_secret_note"));
        assert!(events.iter().any(|e| e["kind"] == "draft"));
        // Schema consent under Strict: names become placeholders, data never leaves.
        let provider = scripted(Some((
            "sample_rows",
            json!({"table": "cc_ai_orders", "n": 5}),
        )));
        let events = ask(
            session.clone(),
            provider.clone(),
            DbAiConsent {
                schema: true,
                ..Default::default()
            },
        )
        .await;
        let seen = provider.seen.lock().unwrap().join("\n");
        assert!(
            !seen.contains("cc_ai_orders") && !seen.contains("customer_secret_note"),
            "{seen}"
        );
        assert!(!seen.contains("top-secret-value"));
        assert!(
            seen.contains("refused"),
            "sample_rows needs its own consent"
        );
        let draft = events.iter().find(|e| e["kind"] == "draft").unwrap()["content"]
            .as_str()
            .unwrap()
            .to_owned();
        assert_eq!(
            draft, "SELECT count(*) FROM cc_ai_orders;",
            "placeholders restored"
        );
        // The draft was not executed: the table still has its one row.
        let (_, rows) = session
            .read_rows(&setup("SELECT count(*) FROM cc_ai_orders"), 1)
            .await
            .unwrap();
        assert_eq!(rows[0][0], DbCell::Int("1".into()));
        // A read-only probe consent still refuses anything but a read.
        let provider = scripted(Some((
            "run_readonly",
            json!({"sql": "DELETE FROM cc_ai_orders"}),
        )));
        ask(
            session.clone(),
            provider.clone(),
            DbAiConsent {
                schema: true,
                run_readonly: true,
                ..Default::default()
            },
        )
        .await;
        let seen = provider.seen.lock().unwrap().join("\n");
        assert!(seen.contains("refused"), "run_readonly refuses writes");
        let (_, rows) = session
            .read_rows(&setup("SELECT count(*) FROM cc_ai_orders"), 1)
            .await
            .unwrap();
        assert_eq!(rows[0][0], DbCell::Int("1".into()));
        let mut drop = setup("DROP TABLE cc_ai_orders");
        let review = session.guard().review(&drop).unwrap();
        session.guard().approve(&review.ticket).unwrap();
        drop.approval_ticket = Some(review.ticket.clone());
        let mut stream = session.execute(&drop).unwrap();
        while stream.events.recv().await.is_some() {}
    }

    #[test]
    fn sensitive_columns_never_reach_the_model() {
        let columns = vec![
            cc_db_core::DbColumn {
                name: "id".into(),
                type_name: String::new(),
                nullable: false,
                sensitive: false,
                primary_key: true,
            },
            cc_db_core::DbColumn {
                name: "password".into(),
                type_name: String::new(),
                nullable: false,
                sensitive: true,
                primary_key: false,
            },
        ];
        let table = markdown(
            &columns,
            &[vec![
                DbCell::Int("1".into()),
                DbCell::Text("hunter2".into()),
            ]],
        );
        assert!(table.contains("[hidden]") && !table.contains("hunter2"));
    }
}
