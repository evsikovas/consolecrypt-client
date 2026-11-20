# Reviewed requirements from AI drafts

Implemented after the immutable Windows r2 preview; r2 does not include this
change. This is E1 work, not a complete dependency installer or a release.

An AI answer remains an unsaved editable playbook. It also retains declared
required variable names and exact role/collection versions. Saving a draft with
requirements first displays a separate diff for
`.consolecrypt-requirements.json`. Cancel writes neither file. Accept saves the
requirements with the existing file SHA (or exclusive creation), then saves the
current, possibly manually edited playbook with its original SHA. Lock/profile
generation changes invalidate the dialog and pending actions.

The two writes are deliberately **not** advertised as a multi-file transaction.
If saving the playbook conflicts, requirements may already be on disk. The UI
states this and keeps the editable draft. It never rolls back over another
editor. Retrying reads and reviews the current requirements again. Existing
manual constraints are unioned, not removed by regeneration; incompatible pins,
unknown schema, duplicate JSON keys (including escaped spellings), or excessive
budgets require a manual file edit. Requirements of other playbooks are retained.

The separate contract avoids rewriting `.consolecrypt-inputs.json` or claiming
that an AI-generated dependency description establishes artifact provenance.
Both existing manual inputs and the new requirements apply conjunctively.

```json
{
  "version": 1,
  "playbooks": {
    "playbook.yml": {
      "required_extra_vars": ["service_port"],
      "required_dependencies": [
        {"kind": "role", "name": "demo", "version": "1.2.3"},
        {"kind": "collection", "name": "example.demo", "version": "2.0.0"}
      ]
    }
  }
}
```

Budgets: 64 KiB per file, 128 playbooks, 64 variable names and 32 dependencies per
playbook. Unknown/duplicate fields, duplicate dependency identities, future
versions, traversal, case variants of the contract filename, mutable version
expressions, and `ansible_`/`consolecrypt_` authority variables fail closed.
`false` and `0` are supplied values; absent, null and blank strings are missing.
Only the selected playbook's declared inputs are required. No Jinja introspection
or automatic inference of all variables in arbitrary playbooks is promised.

The AI proposal source syntax is `role:role_name` or
`collection:namespace.name` with an exact version. It contains no artifact URL,
digest, credential, install or execution authority. `ansible.builtin` is supplied
by the pinned runtime and must not be invented as an external collection.

Before syntax checking, the isolated worker first verifies every locked
dependency tree against the parent-verified snapshot, then checks declared input
contracts and required dependency identities/versions. A matching name/version
alone does not verify installed code: it must also be in the strict
`.consolecrypt-dependencies.json` lock whose source and tree digests were
independently reviewed. Nothing is downloaded or globally installed here.

The whole contract is part of the source revision/digest, so an edit invalidates
prior native approval and validation. Native admission requires the negotiated
`project_requirements_v1` helper capability whenever this root file is present,
including wrong-case variants. Older helpers must be updated rather than
silently ignoring requirements. Wire manifest v1 is unchanged; no new sync-server
object or server access to decrypted project data is introduced.

Coverage includes preservation of manual edits/other playbooks, duplicate keys,
pin conflicts, cancel and first/second CAS failures in the UI; native old-helper
compatibility; real isolated Ansible syntax/lint against a locked role; missing
inputs, different versions and edited role trees; and signed AppCore execution
with the contract included in the approved snapshot. Exact test outcomes belong
in the workspace status log, not inferred from this design description.
