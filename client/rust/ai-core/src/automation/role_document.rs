//! Path-derived role mode. Model/user prose cannot turn role tasks into plays.
use super::{MAX_TEXT, Result, invalid};
use serde_json::Value;

fn role_section(path: &str) -> Option<&str> {
    let parts: Vec<_> = path.split('/').collect();
    (parts.len() >= 4
        && parts[0] == "roles"
        && matches!(
            parts[2],
            "tasks" | "handlers" | "defaults" | "vars" | "meta"
        ))
    .then(|| parts[2])
}

pub(super) fn instructions(path: &str) -> &'static str {
    match role_section(path) {
        Some("handlers") => {
            "The requested file is an Ansible role HANDLERS file. Return a YAML sequence of handler tasks, with names for notify/listen. Do not wrap it in a play, hosts, tasks or handlers. Edit only the requested file; do not create a project, playbook or additional files."
        }
        Some("tasks") => {
            "The requested file is an Ansible role TASKS file. Its content is a YAML sequence of tasks. For a connectivity check, the exact shape is:\n- name: Check Ansible connectivity\n  ansible.builtin.ping: {}\nDo not add hosts or a tasks wrapper. The calling playbook selects the hosts. List required role variables explicitly, and use notify only for an existing handler."
        }
        Some("meta") => {
            "The requested file is Ansible role METADATA. Return a YAML mapping of role metadata, not a play or task list. Declare external role dependencies in the proposal's dependencies as well as metadata. Edit only the requested file."
        }
        Some(_) => {
            "The requested file contains Ansible role VARIABLES or DEFAULTS. Return a YAML mapping of variable names to values, not a play or task list. Never include secrets; keep secret values as explicit caller inputs. Edit only the requested file."
        }
        None => {
            "The requested file is an Ansible playbook. Return a YAML sequence of plays with explicit hosts and tasks or roles. Edit only the requested file."
        }
    }
}

/// Structural review, never a substitute for the existing Runner syntax/lint
/// checks or execution approval. Parse errors remain code-only and leak no YAML.
pub(super) fn validate(path: &str, content: &str) -> Result<()> {
    let Some(section) = role_section(path) else {
        return Ok(());
    };
    let options = serde_saphyr::options! {
        with_snippet: false,
        duplicate_keys: serde_saphyr::DuplicateKeyPolicy::Error,
        budget: serde_saphyr::budget! {
            max_documents: 1,
            max_depth: 32,
            max_nodes: 8192,
            max_events: 32768,
            max_aliases: 64,
            max_anchors: 64,
            max_total_scalar_bytes: MAX_TEXT,
        },
        alias_limits: serde_saphyr::alias_limits! {
            max_total_replayed_events: 8192,
            max_replay_stack_depth: 16,
            max_alias_expansions_per_anchor: 64,
        },
    };
    // Consume the entire stream: from_str deliberately reads only its first
    // document, so it cannot reject a trailing hidden play/document.
    let documents: Vec<Value> =
        serde_saphyr::from_multiple_with_options(content, options).map_err(|_| invalid())?;
    let [document] = documents.as_slice() else {
        return Err(invalid());
    };
    if matches!(section, "tasks" | "handlers") {
        task_list(document.as_array().ok_or_else(invalid)?, 0)
    } else if document.is_object() {
        Ok(())
    } else {
        Err(invalid())
    }
}

fn task_list(tasks: &[Value], depth: usize) -> Result<()> {
    if depth > 16 || tasks.len() > 1024 {
        return Err(invalid());
    }
    for task in tasks {
        let task = task.as_object().ok_or_else(invalid)?;
        if task.is_empty()
            || task.keys().any(|key| {
                matches!(
                    key.as_str(),
                    "hosts"
                        | "tasks"
                        | "handlers"
                        | "pre_tasks"
                        | "post_tasks"
                        | "roles"
                        | "gather_facts"
                        | "gather_subset"
                        | "gather_timeout"
                        | "import_playbook"
                        | "vars_files"
                        | "vars_prompt"
                        | "force_handlers"
                        | "serial"
                        | "strategy"
                        | "max_fail_percentage"
                        | "order"
                )
            })
        {
            return Err(invalid());
        }
        let mut actions = 0;
        for (key, value) in task {
            if matches!(key.as_str(), "block" | "rescue" | "always") {
                if !task.contains_key("block") {
                    return Err(invalid());
                }
                task_list(value.as_array().ok_or_else(invalid)?, depth + 1)?;
                if key == "block" {
                    actions += 1;
                }
            } else if !matches!(
                key.as_str(),
                "name"
                    | "args"
                    | "when"
                    | "vars"
                    | "register"
                    | "tags"
                    | "notify"
                    | "listen"
                    | "become"
                    | "become_user"
                    | "become_method"
                    | "become_flags"
                    | "become_exe"
                    | "check_mode"
                    | "diff"
                    | "no_log"
                    | "changed_when"
                    | "failed_when"
                    | "ignore_errors"
                    | "ignore_unreachable"
                    | "any_errors_fatal"
                    | "run_once"
                    | "delegate_to"
                    | "delegate_facts"
                    | "connection"
                    | "environment"
                    | "collections"
                    | "loop"
                    | "loop_control"
                    | "until"
                    | "retries"
                    | "delay"
                    | "async"
                    | "poll"
                    | "throttle"
                    | "timeout"
                    | "debugger"
                    | "module_defaults"
            ) && !key.starts_with("with_")
            {
                actions += 1;
            }
        }
        if actions != 1 {
            return Err(invalid());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn role_paths_select_tasks_and_handlers_without_touching_regular_playbooks() {
        assert!(instructions("roles/web/tasks/main.yml").contains("TASKS file"));
        assert!(instructions("roles/web/tasks/configure/service.yaml").contains("TASKS file"));
        assert!(instructions("roles/web/handlers/main.yml").contains("HANDLERS file"));
        assert!(instructions("roles/web/defaults/main.yml").contains("VARIABLES or DEFAULTS"));
        assert!(instructions("roles/web/meta/main.yml").contains("METADATA"));
        assert!(instructions("playbook.yml").contains("playbook"));
        assert!(instructions("not_roles/web/tasks/main.yml").contains("playbook"));
    }

    #[test]
    fn task_and_handler_documents_accept_real_actions_blocks_and_jinja() {
        for yaml in [
            "---\n- name: Ensure package\n  ansible.builtin.package:\n    name: '{{ package_name }}'\n    state: present\n  become: true\n",
            "- name: Restart web\n  ansible.builtin.service: {name: web, state: restarted}\n  listen: restart web\n",
            "- block:\n    - ansible.builtin.debug: {msg: ok}\n  rescue:\n    - ansible.builtin.fail: {msg: failed}\n  always: []\n",
            "- ansible.builtin.include_tasks: configure.yml\n- ansible.builtin.import_role: {name: common}\n",
            "[]\n",
        ] {
            assert!(validate("roles/web/tasks/main.yml", yaml).is_ok(), "{yaml}");
        }
    }

    #[test]
    fn role_documents_reject_play_wrappers_invalid_yaml_and_ambiguous_actions() {
        for yaml in [
            "- hosts: all\n  tasks: []\n",
            "tasks: []\n",
            "- import_playbook: site.yml\n",
            "- name: Play\n  roles: [web]\n",
            "- scalar\n",
            "- name: Missing action\n",
            "- ansible.builtin.debug: {}\n  ansible.builtin.fail: {}\n",
            "- ansible.builtin.debug: {}\n  ansible.builtin.debug: {}\n",
            "- rescue: []\n",
            "- block: wrong\n",
            "- block:\n    - hosts: all\n      tasks: []\n",
            "[]\n---\n[]\n",
            "- ansible.builtin.debug: [\n",
        ] {
            assert!(
                validate("roles/web/tasks/main.yml", yaml).is_err(),
                "{yaml}"
            );
        }
    }

    #[test]
    fn role_variable_and_meta_documents_are_mappings_not_plays() {
        for path in [
            "roles/web/defaults/main.yml",
            "roles/web/vars/main.yml",
            "roles/web/meta/main.yml",
        ] {
            assert!(validate(path, "service_name: web\n").is_ok());
            assert!(validate(path, "- hosts: all\n  tasks: []\n").is_err());
            assert!(validate(path, "service_name: web\nservice_name: other\n").is_err());
        }
    }
}
