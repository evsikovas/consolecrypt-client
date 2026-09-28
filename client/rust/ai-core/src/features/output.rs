//! Robust parsing of model answers: strict JSON → repaired JSON (fences,
//! surrounding prose, trailing commas) → heuristic fallback.

use super::{CommandSuggestion, ExplainedPart, Explanation, ParseQuality, SuggestedVariable};
use crate::risk::RiskLevel;
use serde_json::Value;

/// Map free-form risk words to a level (`read-only`, `safe`, `dangerous`, …).
pub fn parse_risk(s: &str) -> Option<RiskLevel> {
    let n: String = s
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_lowercase();
    Some(match n.as_str() {
        "readonly" | "read" | "safe" | "low" | "none" | "harmless" | "ro" => RiskLevel::ReadOnly,
        "modifying" | "modify" | "modifies" | "write" | "writes" | "medium" | "moderate"
        | "change" | "changes" => RiskLevel::Modifying,
        "destructive" | "dangerous" | "high" | "critical" | "danger" | "severe" => {
            RiskLevel::Destructive
        }
        "unknown" | "unsure" | "unclear" => RiskLevel::Unknown,
        _ => return None,
    })
}

/// Find the first balanced `{…}` object (string-aware).
fn balanced_object(text: &str) -> Option<&str> {
    let start = text.find('{')?;
    let mut depth = 0i32;
    let mut in_str = false;
    let mut escape = false;
    for (i, c) in text[start..].char_indices() {
        if in_str {
            if escape {
                escape = false;
            } else if c == '\\' {
                escape = true;
            } else if c == '"' {
                in_str = false;
            }
            continue;
        }
        match c {
            '"' => in_str = true,
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&text[start..start + i + 1]);
                }
            }
            _ => {}
        }
    }
    None
}

fn strip_trailing_commas(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let chars: Vec<char> = s.chars().collect();
    let mut in_str = false;
    let mut escape = false;
    for (i, &c) in chars.iter().enumerate() {
        if in_str {
            out.push(c);
            if escape {
                escape = false;
            } else if c == '\\' {
                escape = true;
            } else if c == '"' {
                in_str = false;
            }
            continue;
        }
        if c == '"' {
            in_str = true;
        }
        if c == ',' {
            let next = chars[i + 1..].iter().find(|n| !n.is_whitespace());
            if matches!(next, Some('}' | ']')) {
                continue;
            }
        }
        out.push(c);
    }
    out
}

/// Code fence contents (first ``` block), without the language tag.
pub(crate) fn first_code_block(text: &str) -> Option<String> {
    let start = text.find("```")?;
    let after = &text[start + 3..];
    let nl = after.find('\n')?;
    let body = &after[nl + 1..];
    let end = body.find("```").unwrap_or(body.len());
    Some(body[..end].trim_end().to_owned())
}

/// Extract a JSON object from a model answer.
pub(crate) fn extract_json(text: &str) -> Option<(Value, ParseQuality)> {
    let t = text.trim();
    if let Ok(v @ Value::Object(_)) = serde_json::from_str::<Value>(t) {
        return Some((v, ParseQuality::Json));
    }
    let mut candidates: Vec<String> = Vec::new();
    if let Some(b) = first_code_block(t) {
        candidates.push(b);
    }
    if let Some(o) = balanced_object(t) {
        candidates.push(o.to_owned());
    }
    for c in candidates {
        for attempt in [c.clone(), strip_trailing_commas(&c)] {
            if let Ok(v @ Value::Object(_)) = serde_json::from_str::<Value>(attempt.trim()) {
                return Some((v, ParseQuality::Repaired));
            }
            if let Some(o) = balanced_object(&attempt) {
                if let Ok(v @ Value::Object(_)) =
                    serde_json::from_str::<Value>(&strip_trailing_commas(o))
                {
                    return Some((v, ParseQuality::Repaired));
                }
            }
        }
    }
    None
}

fn get_str(v: &Value, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|k| {
        v.get(*k).and_then(|x| match x {
            Value::String(s) => Some(s.clone()),
            Value::Array(a) => {
                let parts: Vec<String> = a
                    .iter()
                    .filter_map(|e| e.as_str().map(str::to_owned))
                    .collect();
                (!parts.is_empty()).then(|| parts.join("\n"))
            }
            Value::Null => None,
            other => Some(other.to_string()),
        })
    })
}

fn get_strings(v: &Value, keys: &[&str]) -> Vec<String> {
    keys.iter()
        .find_map(|k| v.get(*k))
        .map(|x| match x {
            Value::Array(a) => a
                .iter()
                .filter_map(|e| match e {
                    Value::String(s) => Some(s.clone()),
                    Value::Object(_) => get_str(e, &["command", "text", "value"]),
                    _ => None,
                })
                .collect(),
            Value::String(s) if !s.is_empty() => vec![s.clone()],
            _ => Vec::new(),
        })
        .unwrap_or_default()
}

pub(crate) fn get_variables(v: &Value) -> Vec<SuggestedVariable> {
    let Some(arr) = ["variables", "vars", "params", "parameters", "placeholders"]
        .iter()
        .find_map(|k| v.get(*k))
    else {
        return Vec::new();
    };
    let mut out = Vec::new();
    match arr {
        Value::Array(items) => {
            for it in items {
                match it {
                    Value::String(s) => out.push(SuggestedVariable {
                        name: s.trim_matches(|c| c == '{' || c == '}').trim().to_owned(),
                        description: String::new(),
                        default: None,
                    }),
                    Value::Object(_) => {
                        if let Some(name) = get_str(it, &["name", "var", "variable", "key"]) {
                            out.push(SuggestedVariable {
                                name: name
                                    .trim_matches(|c| c == '{' || c == '}')
                                    .trim()
                                    .to_owned(),
                                description: get_str(it, &["description", "desc", "help"])
                                    .unwrap_or_default(),
                                default: get_str(it, &["default", "example", "value"]),
                            });
                        }
                    }
                    _ => {}
                }
            }
        }
        Value::Object(map) => {
            for (k, val) in map {
                out.push(SuggestedVariable {
                    name: k.clone(),
                    description: val.as_str().unwrap_or_default().to_owned(),
                    default: None,
                });
            }
        }
        _ => {}
    }
    out.retain(|v| !v.name.is_empty());
    out
}

/// Remove prompt markers and wrapping backticks from a one-line command.
pub(crate) fn clean_command(cmd: &str) -> String {
    let t = cmd.trim().trim_matches('`').trim();
    if t.contains('\n') {
        return t.to_owned();
    }
    for p in ["$ ", "# ", "PS> ", "> ", "% "] {
        if let Some(rest) = t.strip_prefix(p) {
            return rest.trim().to_owned();
        }
    }
    t.to_owned()
}

pub(crate) fn parse_command_output(text: &str) -> CommandSuggestion {
    if let Some((v, q)) = extract_json(text) {
        let command = get_str(
            &v,
            &[
                "command",
                "cmd",
                "shell_command",
                "query",
                "request",
                "code",
            ],
        )
        .map(|c| clean_command(&c))
        .unwrap_or_default();
        return CommandSuggestion {
            command,
            explanation: get_str(
                &v,
                &["explanation", "description", "explain", "summary", "notes"],
            )
            .unwrap_or_default(),
            risk_suggestion: get_str(&v, &["risk_suggestion", "risk", "risk_level", "danger"])
                .and_then(|r| parse_risk(&r)),
            variables: get_variables(&v),
            alternatives: get_strings(&v, &["alternatives", "alternative", "other_options"]),
            diagnosis: get_str(&v, &["diagnosis", "cause", "problem", "error_analysis"]),
            sources: v
                .get("sources")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(|s| {
                            s.as_u64().map(|n| n as usize).or_else(|| {
                                s.as_str()
                                    .and_then(|x| x.trim_matches(['[', ']']).parse().ok())
                            })
                        })
                        .collect()
                })
                .unwrap_or_default(),
            parse_quality: q,
        };
    }
    // Fallback: a code block, a `$ command` line or a single line.
    let (command, explanation) = if let Some(start) = text.find("```") {
        let after = &text[start + 3..];
        match after.find('\n') {
            Some(nl) => {
                let body = &text[start + 3 + nl + 1..];
                let close = body.find("```");
                let block = &body[..close.unwrap_or(body.len())];
                let tail = close.map_or("", |c| &body[c + 3..]).trim();
                let mut expl = text[..start].trim().to_owned();
                if !tail.is_empty() {
                    if !expl.is_empty() {
                        expl.push('\n');
                    }
                    expl.push_str(tail);
                }
                (clean_command(block.trim_end()), expl)
            }
            None => (
                clean_command(after.trim_end_matches('`')),
                text[..start].trim().to_owned(),
            ),
        }
    } else if let Some(line) = text.lines().find(|l| l.trim_start().starts_with("$ ")) {
        (
            clean_command(line),
            text.replace(line, "").trim().to_owned(),
        )
    } else if !text.trim().contains('\n') {
        (clean_command(text), String::new())
    } else {
        (String::new(), text.trim().to_owned())
    };
    CommandSuggestion {
        command,
        explanation,
        risk_suggestion: None,
        variables: Vec::new(),
        alternatives: Vec::new(),
        diagnosis: None,
        sources: Vec::new(),
        parse_quality: ParseQuality::Fallback,
    }
}

pub(crate) fn parse_explanation(text: &str) -> Explanation {
    if let Some((v, q)) = extract_json(text) {
        let parts = v
            .get("parts")
            .or_else(|| v.get("breakdown"))
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|p| {
                        Some(ExplainedPart {
                            text: get_str(p, &["text", "part", "token", "arg"])?,
                            meaning: get_str(p, &["meaning", "explanation", "description"])
                                .unwrap_or_default(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        return Explanation {
            summary: get_str(&v, &["summary", "explanation", "description"]).unwrap_or_default(),
            parts,
            risk_suggestion: get_str(&v, &["risk_suggestion", "risk", "risk_level"])
                .and_then(|r| parse_risk(&r)),
            warnings: get_strings(&v, &["warnings", "warning", "caveats"]),
            parse_quality: q,
        };
    }
    Explanation {
        summary: text.trim().to_owned(),
        parts: Vec::new(),
        risk_suggestion: None,
        warnings: Vec::new(),
        parse_quality: ParseQuality::Fallback,
    }
}

/// Raw snippet fields from a model answer.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RawSnippet {
    pub name: String,
    pub description: String,
    pub template: String,
    pub variables: Vec<SuggestedVariable>,
    pub tags: Vec<String>,
    pub risk_suggestion: Option<RiskLevel>,
    pub parse_quality: ParseQuality,
}

pub(crate) fn parse_snippet_output(text: &str) -> RawSnippet {
    if let Some((v, q)) = extract_json(text) {
        return RawSnippet {
            name: get_str(&v, &["name", "title"]).unwrap_or_default(),
            description: get_str(&v, &["description", "explanation", "summary"])
                .unwrap_or_default(),
            template: get_str(&v, &["template", "command", "snippet", "body"])
                .map(|c| clean_command(&c))
                .unwrap_or_default(),
            variables: get_variables(&v),
            tags: get_strings(&v, &["tags", "labels"]),
            risk_suggestion: get_str(&v, &["risk_suggestion", "risk", "risk_level"])
                .and_then(|r| parse_risk(&r)),
            parse_quality: q,
        };
    }
    let c = parse_command_output(text);
    RawSnippet {
        name: String::new(),
        description: c.explanation,
        template: c.command,
        variables: Vec::new(),
        tags: Vec::new(),
        risk_suggestion: None,
        parse_quality: ParseQuality::Fallback,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strict_json() {
        let c = parse_command_output(
            r#"{"command":"kubectl get pods -n {{ns}}","explanation":"lists pods","risk_suggestion":"read_only","variables":[{"name":"ns","description":"namespace","default":"default"}],"alternatives":["kubectl get po"]}"#,
        );
        assert_eq!(c.parse_quality, ParseQuality::Json);
        assert_eq!(c.command, "kubectl get pods -n {{ns}}");
        assert_eq!(c.risk_suggestion, Some(RiskLevel::ReadOnly));
        assert_eq!(c.variables[0].default.as_deref(), Some("default"));
        assert_eq!(c.alternatives, vec!["kubectl get po"]);
    }

    #[test]
    fn repaired_json() {
        let text = "Sure! Here you go:\n```json\n{\n  \"cmd\": \"df -h\",\n  \"risk\": \"Safe\",\n  \"explanation\": \"disk usage\",\n}\n```\nHope it helps {not json}";
        let c = parse_command_output(text);
        assert_eq!(c.parse_quality, ParseQuality::Repaired);
        assert_eq!(c.command, "df -h");
        assert_eq!(c.risk_suggestion, Some(RiskLevel::ReadOnly));
        let prose = "The answer is {\"command\": \"uptime\", \"risk_level\": \"DANGEROUS\"} ok";
        let c = parse_command_output(prose);
        assert_eq!(c.command, "uptime");
        assert_eq!(c.risk_suggestion, Some(RiskLevel::Destructive));
    }

    #[test]
    fn fallbacks() {
        let c = parse_command_output("Use this:\n```bash\n$ ls -la /var/log\n```\nIt lists files.");
        assert_eq!(c.parse_quality, ParseQuality::Fallback);
        assert_eq!(c.command, "ls -la /var/log");
        assert!(c.explanation.contains("Use this:"));
        assert!(c.explanation.contains("It lists files."));
        assert_eq!(parse_command_output("`uptime`").command, "uptime");
        let p = parse_command_output("Run it like:\n$ free -m\nthen check.");
        assert_eq!(p.command, "free -m");
        let none = parse_command_output("I cannot help with that.\nSorry.");
        assert!(none.command.is_empty());
    }

    #[test]
    fn explanation_and_snippet() {
        let e = parse_explanation(
            r#"{"summary":"s","parts":[{"text":"-rf","meaning":"recursive force"}],"risk":"destructive","warnings":"careful"}"#,
        );
        assert_eq!(e.parts[0].text, "-rf");
        assert_eq!(e.risk_suggestion, Some(RiskLevel::Destructive));
        assert_eq!(e.warnings, vec!["careful"]);
        assert_eq!(parse_explanation("plain words").summary, "plain words");
        let s = parse_snippet_output(
            r#"{"name":"Logs","template":"kubectl logs {{pod}}","variables":["{{pod}}"],"tags":["k8s"]}"#,
        );
        assert_eq!(s.name, "Logs");
        assert_eq!(s.variables[0].name, "pod");
    }

    #[test]
    fn risk_words() {
        assert_eq!(parse_risk("read-only"), Some(RiskLevel::ReadOnly));
        assert_eq!(parse_risk("READ_ONLY"), Some(RiskLevel::ReadOnly));
        assert_eq!(parse_risk("Modifying"), Some(RiskLevel::Modifying));
        assert_eq!(parse_risk("potentially bad"), None);
    }
}
