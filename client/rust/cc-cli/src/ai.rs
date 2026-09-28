//! `consolecrypt ai search|ask|gen|explain` — the AI features of the facade
//! (providers are configured in the app and synced with the vault).
//!
//! AI output never runs by itself: `ai gen --run --host H` passes the
//! command through `approve_run` (the local risk gate; modifying /
//! destructive / unknown commands need `--yes` or an interactive "y") and
//! only then `exec_approved`.

use crate::input::{confirm, stdin_is_tty};
use crate::output::Out;
use crate::{prompt_responder, unlocked, Cli, CliError, R};
use cc_app_core::*;
use clap::Subcommand;
use serde::de::DeserializeOwned;
use std::io::Write;

#[derive(Subcommand, Debug)]
pub enum AiCmd {
    /// Search the local knowledge base (snippets, notes, hosts, history).
    Search {
        #[arg(required = true)]
        query: Vec<String>,
        /// Restrict to kinds: snippet, note, host, history (repeatable).
        #[arg(long)]
        kind: Vec<String>,
        /// Involve the LLM: never (default), auto (no exact hit), always.
        #[arg(long, default_value = "never")]
        llm: String,
        #[arg(long, default_value_t = 10)]
        limit: u32,
        /// Provider id (default: the vault's default provider).
        #[arg(long)]
        provider: Option<String>,
    },
    /// Ask AI (the answer is streamed).
    Ask {
        #[arg(required = true)]
        question: Vec<String>,
        /// Host the question is about (name or id).
        #[arg(long)]
        host: Option<String>,
        #[arg(long)]
        provider: Option<String>,
        /// Continue a conversation (id printed after an answer).
        #[arg(long)]
        conversation: Option<String>,
    },
    /// Generate a command; `--run` executes it on `--host` after the risk gate.
    Gen {
        #[arg(required = true)]
        request: Vec<String>,
        /// Host (name or id): context for the model and target of `--run`.
        #[arg(long)]
        host: Option<String>,
        /// Dialect: shell, bash, zsh, power_shell, cmd, sql, postgre_sql,
        /// kubectl, helm, docker, terraform, ansible, redis_cli, cql,
        /// open_search_dsl, curl (default: from the host's shell).
        #[arg(long)]
        target: Option<String>,
        #[arg(long)]
        provider: Option<String>,
        /// Run the command on `--host` (asks for confirmation when needed).
        #[arg(long, requires = "host")]
        run: bool,
        /// Confirm modifying / destructive / unknown commands without asking.
        #[arg(long, requires = "run")]
        yes: bool,
        /// Trust an unknown host key on `--run`.
        #[arg(long, requires = "run")]
        accept_host_key: bool,
    },
    /// Explain a command.
    Explain {
        #[arg(required = true)]
        command: Vec<String>,
        #[arg(long)]
        host: Option<String>,
        #[arg(long)]
        target: Option<String>,
        #[arg(long)]
        provider: Option<String>,
    },
}

/// Parse a snake_case enum value (`--kind`, `--llm`, `--target`).
fn parse_enum<T: DeserializeOwned>(what: &str, v: &str) -> R<T> {
    serde_json::from_value(serde_json::Value::String(v.trim().to_ascii_lowercase()))
        .map_err(|_| CliError::Usage(format!("unknown {what} {v:?}")))
}

async fn host_id(app: &AppCore, host: &Option<String>) -> R<Option<String>> {
    Ok(match host {
        Some(h) => Some(app.find_host(h.clone()).await?.id),
        None => None,
    })
}

fn print_proposal(p: &CommandProposalDto) {
    println!("{}", p.command);
    if !p.explanation.is_empty() {
        println!("  # {}", p.explanation);
    }
    if let Some(d) = &p.diagnosis {
        println!("  # diagnosis: {d}");
    }
    let confirm = if p.run.requires_confirmation {
        ", needs confirmation"
    } else {
        ""
    };
    println!("  risk: {:?}{confirm}", p.run.risk);
    for r in &p.run.reasons {
        println!("    - {}: {}", r.rule, r.detail);
    }
    if !p.run.unresolved_placeholders.is_empty() {
        println!(
            "  fill in first: {}",
            p.run.unresolved_placeholders.join(", ")
        );
    }
    if p.redactions.total > 0 {
        println!(
            "  ({} value(s) redacted before sending, profile {:?})",
            p.redactions.total, p.privacy_profile
        );
    }
}

pub async fn ai(cli: &Cli, app: &AppCore, cmd: &AiCmd, out: &Out) -> R<i32> {
    unlocked(cli, app).await?;
    match cmd {
        AiCmd::Search {
            query,
            kind,
            llm,
            limit,
            provider,
        } => {
            let kinds = kind
                .iter()
                .map(|k| parse_enum::<DocKind>("kind", k))
                .collect::<R<Vec<_>>>()?;
            let r = app
                .ai_search(
                    query.join(" "),
                    AiSearchOptionsDto {
                        limit: *limit,
                        kinds,
                        llm: parse_enum("llm mode", llm)?,
                        provider_id: provider.clone(),
                        ..Default::default()
                    },
                )
                .await?;
            out.value(&r, || {
                if r.hits.is_empty() {
                    println!("no local results");
                }
                for h in &r.hits {
                    println!(
                        "{:<8} {}  [{:?}]",
                        format!("{:?}", h.kind),
                        h.title,
                        h.origin
                    );
                    if let Some(line) = h.body.lines().find(|l| !l.trim().is_empty()) {
                        println!("         {line}");
                    }
                }
                if let Some(a) = &r.answer {
                    println!("\nAI ({:?}):", a.origin);
                    print_proposal(&a.proposal);
                }
            });
        }
        AiCmd::Ask {
            question,
            host,
            provider,
            conversation,
        } => {
            let mut s = app
                .ai_ask(
                    provider.clone(),
                    AiAskRequestDto {
                        conversation_id: conversation.clone(),
                        question: question.join(" "),
                    },
                    AiContextOptionsDto {
                        host_id: host_id(app, host).await?,
                        ..Default::default()
                    },
                )
                .await?;
            let mut answer = String::new();
            let mut summary = None;
            while let Some(chunk) = s.chunks.recv().await {
                match chunk {
                    AiAskChunk::Delta { text } => {
                        if !cli.json {
                            print!("{text}");
                            let _ = std::io::stdout().flush();
                        }
                        answer.push_str(&text);
                    }
                    AiAskChunk::Done(d) => summary = Some(d),
                    AiAskChunk::Error { code, message } => {
                        if !cli.json {
                            println!();
                        }
                        return Err(CliError::App(match code.as_str() {
                            "ai_auth_failed" => AppError::AiAuthFailed(message),
                            "ai_unavailable" => AppError::AiUnavailable(message),
                            "cancelled" => AppError::Cancelled,
                            "rate_limited" => AppError::RateLimited {
                                retry_after_secs: None,
                            },
                            _ => AppError::AiProvider(message),
                        }));
                    }
                }
            }
            out.value(
                &serde_json::json!({
                    "conversation_id": s.conversation_id,
                    "answer": answer,
                    "summary": summary,
                }),
                || {
                    println!();
                    eprintln!("(conversation {})", s.conversation_id);
                },
            );
        }
        AiCmd::Gen {
            request,
            host,
            target,
            provider,
            run,
            yes,
            accept_host_key,
        } => {
            let target = match target {
                Some(t) => Some(parse_enum::<CommandTarget>("target", t)?),
                None => None,
            };
            let host_id = host_id(app, host).await?;
            let p = app
                .ai_generate_command(
                    provider.clone(),
                    request.join(" "),
                    target,
                    AiContextOptionsDto {
                        host_id: host_id.clone(),
                        ..Default::default()
                    },
                )
                .await?;
            if !*run {
                out.value(&p, || print_proposal(&p));
                return Ok(0);
            }
            if !cli.json {
                print_proposal(&p);
            }
            let mut run = p.run.clone();
            run.host_id = host_id;
            let confirmed = *yes
                || (run.requires_confirmation
                    && stdin_is_tty()
                    && confirm(&format!(
                        "Run this {:?} command on {}?",
                        run.risk,
                        run.host_display.clone().unwrap_or_default()
                    ))?);
            let approved = app.approve_run(run, confirmed).await?;
            let responder = prompt_responder(app, *accept_host_key);
            let r = app.exec_approved(approved.token).await;
            responder.abort();
            let r = r?;
            out.exec(&r);
            return Ok(r.exit_status.map(|c| c as i32).unwrap_or(255));
        }
        AiCmd::Explain {
            command,
            host,
            target,
            provider,
        } => {
            let target = match target {
                Some(t) => Some(parse_enum::<CommandTarget>("target", t)?),
                None => None,
            };
            let e = app
                .ai_explain_command(
                    provider.clone(),
                    command.join(" "),
                    target,
                    AiContextOptionsDto {
                        host_id: host_id(app, host).await?,
                        ..Default::default()
                    },
                )
                .await?;
            out.value(&e, || {
                println!("{}", e.summary);
                for p in &e.parts {
                    println!("  {:<24} {}", p.text, p.meaning);
                }
                for w in &e.warnings {
                    println!("  ! {w}");
                }
                println!(
                    "  risk: {:?} (local rules: {:?})",
                    e.effective_risk, e.local_risk.level
                );
            });
        }
    }
    Ok(0)
}
