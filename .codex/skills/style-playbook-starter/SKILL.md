---
name: style-playbook-starter
description: "Starter guide for style-topic skills: how to patch (write), enhance (write), and update (read) topic skills consistently."
---

# Style Playbook Starter

Use this starter whenever you maintain a `style-topic-*` skill.

## Patch (write)

- Fix factual errors, broken commands, invalid paths, or stale references.
- Keep behavior changes minimal and explicit.
- If patching changes expected outputs, update the relevant examples.

## Enhance (write)

- Add reusable defaults and decision rules, not project-specific one-offs.
- Prefer additive updates; avoid breaking existing command contracts.
- Add clear guardrails and failure handling where missing.

## Update (read)

- Read latest topic skill + source tag doc before proposing changes.
- Validate that examples still match current files and scripts.
- Call out drift between topic guidance and source references.

## Required workflow in topic skills

- Topic skills must cite this starter in a dedicated section.
- Topic skills should not duplicate starter content verbatim; reference it.
