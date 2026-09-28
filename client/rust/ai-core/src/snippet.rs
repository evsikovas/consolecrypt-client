//! Snippet engine: `{{variable}}` templates, the value form model and
//! rendering with per-dialect quoting so user values can never break out of
//! their slot (CLIENT_SPEC §11.2).
//!
//! Quoting is **context-aware**: the template is lexed with the target
//! dialect's rules, so a placeholder that already sits inside quotes in the
//! template (`echo '{{msg}}'`) is escaped for that context instead of being
//! quoted twice.

use crate::sanitizer::{rules_classify_secret_key, Category};
use crate::shell::{is_ps_double_quote, is_ps_single_quote, ShellDialect};
use cc_models::snippet::{template_variables, Snippet, SnippetType, SnippetVariable};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Quoting rules used when substituting values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RenderDialect {
    /// sh / bash / zsh.
    #[default]
    Posix,
    PowerShell,
    Cmd,
    /// Generic SQL: `'…'` literals with `''`; backslashes rejected inside
    /// literals (MySQL treats them as escapes).
    Sql,
    /// PostgreSQL (standard_conforming_strings): `''` only.
    Postgres,
    /// Cassandra CQL: `''` only.
    Cql,
    /// JSON (OpenSearch DSL bodies).
    Json,
}

impl RenderDialect {
    /// Dialect for a snippet type; `shell` refines shell-executed types
    /// (e.g. a kubectl snippet meant for PowerShell).
    pub fn for_snippet(t: SnippetType, shell: Option<&str>) -> Self {
        let from_shell = shell
            .and_then(ShellDialect::from_shell_name)
            .map(Self::from);
        match t {
            SnippetType::Powershell => Self::PowerShell,
            SnippetType::Cmd => Self::Cmd,
            SnippetType::Sql => Self::Sql,
            SnippetType::Postgresql => Self::Postgres,
            SnippetType::Cql => Self::Cql,
            SnippetType::OpensearchDsl => Self::Json,
            _ => from_shell.unwrap_or(Self::Posix),
        }
    }
}

impl From<ShellDialect> for RenderDialect {
    fn from(d: ShellDialect) -> Self {
        match d {
            ShellDialect::Posix => Self::Posix,
            ShellDialect::PowerShell => Self::PowerShell,
            ShellDialect::Cmd => Self::Cmd,
        }
    }
}

/// A parsed template piece.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Segment {
    Literal(String),
    Var(String),
}

/// Parsed `{{variable}}` template. Parsing mirrors
/// [`cc_models::snippet::template_variables`] exactly (invalid placeholders
/// such as `{{1bad}}` stay literal text).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Template {
    segments: Vec<Segment>,
}

fn valid_name(name: &str) -> bool {
    name.chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'))
}

impl Template {
    pub fn parse(src: &str) -> Self {
        let mut segments = Vec::new();
        let mut lit = String::new();
        let mut rest = src;
        while let Some(start) = rest.find("{{") {
            let after = &rest[start + 2..];
            let Some(end) = after.find("}}") else { break };
            lit.push_str(&rest[..start]);
            let name = after[..end].trim();
            if valid_name(name) {
                if !lit.is_empty() {
                    segments.push(Segment::Literal(std::mem::take(&mut lit)));
                }
                segments.push(Segment::Var(name.to_owned()));
            } else {
                lit.push_str(&rest[start..start + 2 + end + 2]);
            }
            rest = &after[end + 2..];
        }
        lit.push_str(rest);
        if !lit.is_empty() {
            segments.push(Segment::Literal(lit));
        }
        Self { segments }
    }

    pub fn segments(&self) -> &[Segment] {
        &self.segments
    }

    /// Variable names in order of first appearance (same as
    /// `template_variables`).
    pub fn variables(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for s in &self.segments {
            if let Segment::Var(n) = s {
                if !out.contains(n) {
                    out.push(n.clone());
                }
            }
        }
        out
    }

    /// Substitute `values`, quoting each value for its lexical context.
    pub fn render(
        &self,
        values: &HashMap<String, String>,
        dialect: RenderDialect,
    ) -> Result<String, RenderError> {
        let mut out = String::new();
        let mut ctx = Lexer::new(dialect);
        for s in &self.segments {
            match s {
                Segment::Literal(l) => {
                    ctx.feed(l);
                    out.push_str(l);
                }
                Segment::Var(name) => {
                    let v = values
                        .get(name)
                        .ok_or_else(|| RenderError::MissingValue { name: name.clone() })?;
                    let q = quote(v, ctx.state, dialect).map_err(|reason| match reason {
                        Reject::Control => RenderError::ControlCharacter { name: name.clone() },
                        Reject::Unsafe(r) => RenderError::UnsafeValue {
                            name: name.clone(),
                            reason: r,
                        },
                    })?;
                    out.push_str(&q);
                }
            }
        }
        Ok(out)
    }
}

/// Rendering failures.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, Serialize, Deserialize)]
pub enum RenderError {
    #[error("no value for variable `{name}`")]
    MissingValue { name: String },
    #[error("value for `{name}` contains control characters (newline, tab, …)")]
    ControlCharacter { name: String },
    #[error("value for `{name}` cannot be inserted safely: {reason}")]
    UnsafeValue { name: String, reason: String },
}

/// Lexical context at a placeholder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Quote {
    None,
    Single,
    Double,
    Backtick,
}

struct Lexer {
    dialect: RenderDialect,
    state: Quote,
    escape_next: bool,
}

impl Lexer {
    fn new(dialect: RenderDialect) -> Self {
        Self {
            dialect,
            state: Quote::None,
            escape_next: false,
        }
    }

    fn feed(&mut self, s: &str) {
        for c in s.chars() {
            if self.escape_next {
                self.escape_next = false;
                continue;
            }
            match self.dialect {
                RenderDialect::Posix => match (self.state, c) {
                    (Quote::None, '\\') | (Quote::Double, '\\') => self.escape_next = true,
                    (Quote::None, '\'') => self.state = Quote::Single,
                    (Quote::None, '"') => self.state = Quote::Double,
                    (Quote::Single, '\'') | (Quote::Double, '"') => self.state = Quote::None,
                    _ => {}
                },
                RenderDialect::PowerShell => match self.state {
                    Quote::None if c == '`' => self.escape_next = true,
                    Quote::None if is_ps_single_quote(c) => self.state = Quote::Single,
                    Quote::None if is_ps_double_quote(c) => self.state = Quote::Double,
                    Quote::Single if is_ps_single_quote(c) => self.state = Quote::None,
                    Quote::Double if c == '`' => self.escape_next = true,
                    Quote::Double if is_ps_double_quote(c) => self.state = Quote::None,
                    _ => {}
                },
                RenderDialect::Cmd => match (self.state, c) {
                    (Quote::None, '^') => self.escape_next = true,
                    (Quote::None, '"') => self.state = Quote::Double,
                    (Quote::Double, '"') => self.state = Quote::None,
                    _ => {}
                },
                RenderDialect::Sql | RenderDialect::Postgres | RenderDialect::Cql => {
                    match (self.state, c) {
                        (Quote::None, '\'') => self.state = Quote::Single,
                        (Quote::None, '"') => self.state = Quote::Double,
                        (Quote::None, '`') => self.state = Quote::Backtick,
                        (Quote::Single, '\'') | (Quote::Double, '"') | (Quote::Backtick, '`') => {
                            self.state = Quote::None;
                        }
                        _ => {}
                    }
                }
                RenderDialect::Json => match (self.state, c) {
                    (Quote::Double, '\\') => self.escape_next = true,
                    (Quote::None, '"') => self.state = Quote::Double,
                    (Quote::Double, '"') => self.state = Quote::None,
                    _ => {}
                },
            }
        }
    }
}

enum Reject {
    Control,
    Unsafe(String),
}

fn has_control(v: &str) -> bool {
    v.chars().any(|c| c.is_control())
}

fn posix_single(v: &str) -> String {
    format!("'{}'", v.replace('\'', r"'\''"))
}

fn is_posix_safe(v: &str) -> bool {
    !v.is_empty()
        && v.chars().all(|c| {
            c.is_ascii_alphanumeric()
                || matches!(c, '_' | '@' | '%' | '+' | '=' | ':' | ',' | '.' | '/' | '-')
        })
}

fn ps_escape_single(v: &str) -> String {
    let mut out = String::with_capacity(v.len());
    for c in v.chars() {
        out.push(c);
        if is_ps_single_quote(c) {
            out.push(c);
        }
    }
    out
}

fn ps_escape_double(v: &str) -> String {
    let mut out = String::with_capacity(v.len());
    for c in v.chars() {
        if c == '`' || c == '$' || is_ps_double_quote(c) {
            out.push('`');
        }
        out.push(c);
    }
    out
}

fn is_ps_safe(v: &str) -> bool {
    !v.is_empty()
        && !v.starts_with('-')
        && v.chars().all(|c| {
            c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | ',' | '/' | ':' | '\\' | '-')
        })
}

fn quote(v: &str, ctx: Quote, dialect: RenderDialect) -> Result<String, Reject> {
    match dialect {
        RenderDialect::Posix => {
            if has_control(v) {
                return Err(Reject::Control);
            }
            Ok(match ctx {
                Quote::None | Quote::Backtick => {
                    if is_posix_safe(v) {
                        v.to_owned()
                    } else {
                        posix_single(v)
                    }
                }
                Quote::Single => v.replace('\'', r"'\''"),
                // Close the double quote, insert a single-quoted literal,
                // reopen: "pre"'value'"post" is one word in POSIX shells.
                Quote::Double => {
                    if is_posix_safe(v) {
                        v.to_owned()
                    } else {
                        format!("\"{}\"", posix_single(v))
                    }
                }
            })
        }
        RenderDialect::PowerShell => {
            if has_control(v) {
                return Err(Reject::Control);
            }
            Ok(match ctx {
                Quote::None | Quote::Backtick => {
                    if is_ps_safe(v) {
                        v.to_owned()
                    } else {
                        format!("'{}'", ps_escape_single(v))
                    }
                }
                Quote::Single => ps_escape_single(v),
                Quote::Double => ps_escape_double(v),
            })
        }
        RenderDialect::Cmd => {
            if has_control(v) {
                return Err(Reject::Control);
            }
            if let Some(c) = v.chars().find(|c| matches!(c, '"' | '%' | '!')) {
                return Err(Reject::Unsafe(format!(
                    "`{c}` cannot be escaped reliably in cmd.exe"
                )));
            }
            let safe = !v.is_empty()
                && v.chars().all(|c| {
                    c.is_ascii_alphanumeric()
                        || matches!(
                            c,
                            '_' | '.' | ',' | ':' | '\\' | '/' | '@' | '+' | '=' | '-'
                        )
                });
            Ok(match ctx {
                Quote::Double => v.to_owned(),
                _ if safe => v.to_owned(),
                _ => format!("\"{v}\""),
            })
        }
        RenderDialect::Sql | RenderDialect::Postgres | RenderDialect::Cql => {
            if v.contains('\0') {
                return Err(Reject::Control);
            }
            match ctx {
                Quote::Single => {
                    if dialect == RenderDialect::Sql && v.contains('\\') {
                        return Err(Reject::Unsafe(
                            "backslash inside a SQL string literal is dialect-dependent; use the PostgreSQL snippet type or remove it".into(),
                        ));
                    }
                    Ok(v.replace('\'', "''"))
                }
                Quote::Double => Ok(v.replace('"', "\"\"")),
                Quote::Backtick => Ok(v.replace('`', "``")),
                Quote::None => {
                    let ident = !v.is_empty()
                        && v.chars()
                            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.')
                        && !v.starts_with('.');
                    let number = v.parse::<f64>().is_ok()
                        && v.chars().all(|c| {
                            c.is_ascii_digit() || matches!(c, '.' | '-' | 'e' | 'E' | '+')
                        });
                    if ident || number {
                        Ok(v.to_owned())
                    } else {
                        Err(Reject::Unsafe(
                            "only identifiers/numbers can be inserted unquoted in SQL; put the placeholder in quotes ('{{name}}')".into(),
                        ))
                    }
                }
            }
        }
        RenderDialect::Json => {
            let encoded = serde_json::to_string(v).map_err(|_| Reject::Control)?;
            Ok(match ctx {
                Quote::Double | Quote::Single | Quote::Backtick => {
                    encoded[1..encoded.len() - 1].to_owned()
                }
                Quote::None => {
                    let literal = matches!(v, "true" | "false" | "null")
                        || serde_json::from_str::<serde_json::Number>(v).is_ok();
                    if literal {
                        v.to_owned()
                    } else {
                        encoded
                    }
                }
            })
        }
    }
}

/// One input in the "fill in values" form shown before running a snippet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FormField {
    pub name: String,
    /// Human label derived from the name (`db_host` → "Db host").
    pub label: String,
    pub description: String,
    pub default: Option<String>,
    pub required: bool,
    /// The name suggests a secret (password, token, …): the UI should mask
    /// the input and never store the value in history.
    pub secret: bool,
}

/// Field-level validation error.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FieldError {
    pub name: String,
    pub message: String,
}

/// The value form for a template.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VariableForm {
    pub fields: Vec<FormField>,
}

fn label_for(name: &str) -> String {
    let words: Vec<&str> = name
        .split(['_', '-', '.'])
        .filter(|w| !w.is_empty())
        .collect();
    let mut s = words.join(" ");
    if let Some(first) = s.get(..1) {
        s = first.to_uppercase() + &s[1..];
    }
    s
}

impl VariableForm {
    /// Fields in order of appearance in the template, enriched with the
    /// snippet's variable metadata (description/default/required).
    pub fn from_template(template: &str, meta: &[SnippetVariable]) -> Self {
        let fields = template_variables(template)
            .into_iter()
            .map(|name| {
                let m = meta.iter().find(|m| m.name == name);
                FormField {
                    label: label_for(&name),
                    description: m.map(|m| m.description.clone()).unwrap_or_default(),
                    default: m.and_then(|m| m.default.clone()),
                    required: m.is_none_or(|m| m.required),
                    secret: matches!(
                        rules_classify_secret_key(&name),
                        Some(Category::Password | Category::Secret)
                    ),
                    name,
                }
            })
            .collect();
        Self { fields }
    }

    pub fn for_snippet(s: &Snippet) -> Self {
        Self::from_template(&s.template, &s.variables)
    }

    /// Merge user input with defaults and check required fields. Optional
    /// fields without value/default render as empty strings.
    pub fn resolve(
        &self,
        input: &HashMap<String, String>,
    ) -> Result<HashMap<String, String>, Vec<FieldError>> {
        let mut out = HashMap::new();
        let mut errors = Vec::new();
        for f in &self.fields {
            let v = input
                .get(&f.name)
                .filter(|v| !v.is_empty())
                .cloned()
                .or_else(|| f.default.clone());
            match v {
                Some(v) => {
                    out.insert(f.name.clone(), v);
                }
                None if f.required => errors.push(FieldError {
                    name: f.name.clone(),
                    message: "a value is required".into(),
                }),
                None => {
                    out.insert(f.name.clone(), String::new());
                }
            }
        }
        if errors.is_empty() {
            Ok(out)
        } else {
            Err(errors)
        }
    }
}

/// Resolve the form and render a snippet for its dialect.
pub fn render_snippet(
    s: &Snippet,
    input: &HashMap<String, String>,
) -> Result<String, SnippetRenderError> {
    let form = VariableForm::for_snippet(s);
    let values = form.resolve(input).map_err(SnippetRenderError::Form)?;
    let dialect = RenderDialect::for_snippet(s.snippet_type, s.shell.as_deref());
    Template::parse(&s.template)
        .render(&values, dialect)
        .map_err(SnippetRenderError::Render)
}

/// Errors from [`render_snippet`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, Serialize, Deserialize)]
pub enum SnippetRenderError {
    #[error("form has invalid fields")]
    Form(Vec<FieldError>),
    #[error(transparent)]
    Render(RenderError),
}

/// Convenience: build a value map from pairs.
pub fn values_from<I, K, V>(pairs: I) -> HashMap<String, String>
where
    I: IntoIterator<Item = (K, V)>,
    K: Into<String>,
    V: Into<String>,
{
    pairs
        .into_iter()
        .map(|(k, v)| (k.into(), v.into()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_matches_model_extraction() {
        for t in [
            "kubectl logs -n {{namespace}} {{ pod }} --tail={{lines}} {{pod}}",
            "echo {{}} {{1bad}} {{ unterminated",
            "{{a}}{{b}}x{{a.b-c}}",
            "no vars",
            "{{ a }} }} {{",
        ] {
            assert_eq!(Template::parse(t).variables(), template_variables(t), "{t}");
        }
        let t = Template::parse("a {{1bad}} {{x}}");
        assert_eq!(
            t.segments(),
            &[
                Segment::Literal("a {{1bad}} ".into()),
                Segment::Var("x".into())
            ]
        );
    }

    #[test]
    fn posix_quoting_contexts() {
        let v = values_from([("x", "it's $(reboot)")]);
        let r = |t: &str| Template::parse(t).render(&v, RenderDialect::Posix).unwrap();
        assert_eq!(r("echo {{x}}"), r#"echo 'it'\''s $(reboot)'"#);
        assert_eq!(r("echo '{{x}}'"), r#"echo 'it'\''s $(reboot)'"#);
        assert_eq!(
            r("echo \"a {{x}} b\""),
            r#"echo "a "'it'\''s $(reboot)'" b""#
        );
        let safe = values_from([("x", "prod-1.example:80")]);
        assert_eq!(
            Template::parse("ssh {{x}}")
                .render(&safe, RenderDialect::Posix)
                .unwrap(),
            "ssh prod-1.example:80"
        );
        let nl = values_from([("x", "a\nrm -rf /")]);
        assert!(matches!(
            Template::parse("echo {{x}}").render(&nl, RenderDialect::Posix),
            Err(RenderError::ControlCharacter { .. })
        ));
        let empty = values_from([("x", "")]);
        assert_eq!(
            Template::parse("echo {{x}}")
                .render(&empty, RenderDialect::Posix)
                .unwrap(),
            "echo ''"
        );
    }

    #[test]
    fn form_model() {
        let meta = vec![SnippetVariable {
            name: "lines".into(),
            description: "How many".into(),
            default: Some("100".into()),
            required: true,
        }];
        let f = VariableForm::from_template(
            "kubectl logs {{pod}} --tail={{lines}} -p {{db_password}}",
            &meta,
        );
        assert_eq!(f.fields.len(), 3);
        assert_eq!(f.fields[1].default.as_deref(), Some("100"));
        assert_eq!(f.fields[1].label, "Lines");
        assert!(f.fields[2].secret);
        assert!(!f.fields[0].secret);
        let err = f.resolve(&HashMap::new()).unwrap_err();
        assert_eq!(err.len(), 2);
        let ok = f
            .resolve(&values_from([("pod", "api-1"), ("db_password", "x")]))
            .unwrap();
        assert_eq!(ok["lines"], "100");
    }
}
