# Plan: Ansible & Inventory Loader for Oxigraph MCP

## Context

The Oxigraph MCP server currently loads code (Rust, TypeScript) and git history into an RDF knowledge graph. We need to extend it to load **Ansible infrastructure-as-code** artifacts — inventory files, playbooks, roles, and variable files — so that infrastructure topology, deployment automation, and host configuration can be queried via SPARQL alongside code and architecture data.

The ontology mapping uses **ICAS `host:Host`** for inventory hosts (enabling joins with C4 deployment nodes, monitoring probes, and hardening reports from the ds-reporting ontology stack) plus a new **`ans:` namespace** for Ansible-specific concepts (playbooks, roles, tasks, groups).

## Architecture

Standalone tool pattern (like `tools/git.rs` + `loaders/git.rs`), NOT a LanguageLoader.

**New files:**
- `rust/src/loaders/ansible.rs` — Parsing logic, quad generation
- `rust/src/tools/ansible.rs` — Tool functions (`load_inventory`, `load_ansible`)

**Modified files:**
- `rust/src/loaders/mod.rs` — Add `pub mod ansible;`
- `rust/src/tools/mod.rs` — Add `pub mod ansible;`
- `rust/src/main.rs` — Add param structs + 2 async tool handlers
- `rust/Cargo.toml` — Add `serde_yaml = "0.9"`
- `SPECIFICATIONS.md` — Document new ontology & tools

## Namespaces

| Prefix | URI | Source |
|--------|-----|--------|
| `ans:` | `https://ds-labs.org/ansible#` | **NEW** — Ansible-specific classes/properties |
| `host:` | `http://www.invincea.com/ontologies/icas/1.0/host#` | Existing ICAS — shared host identity |

All triples go to the **default graph** (consistent with all other loaders).

## Ontology

### Classes

| Class | Description |
|-------|-------------|
| `host:Host` | Inventory host (ICAS, enables cross-ontology joins) |
| `ans:HostGroup` | Group of hosts (`[webservers]`) |
| `ans:Inventory` | Inventory file/directory |
| `ans:Variable` | Key-value variable |
| `ans:Playbook` | Playbook YAML file |
| `ans:Play` | Play within a playbook (`- hosts:` block) |
| `ans:Task` | Task within a play or role |
| `ans:Role` | Ansible role |
| `ans:Handler` | Notified handler |
| `ans:Template` | Jinja2 template file |

### Key Properties

| Property | Domain → Range | Description |
|----------|----------------|-------------|
| `host:hostName` | Host → string | Hostname from inventory |
| `ans:ansibleHost` | Host → string | `ansible_host` connection address |
| `ans:memberOf` | Host → HostGroup | Host-to-group membership |
| `ans:hasHost` | HostGroup → Host | Group contains host |
| `ans:childGroup` | HostGroup → HostGroup | Group hierarchy |
| `ans:hasVariable` | Host/Group → Variable | Variable attachment |
| `ans:variableName` / `ans:variableValue` | Variable → string | Key-value pair |
| `ans:hasPlay` | Playbook → Play | Play containment |
| `ans:targetHosts` | Play → string | Hosts pattern |
| `ans:hasTask` | Play/Role → Task | Task containment |
| `ans:module` | Task → string | Ansible module name |
| `ans:usesRole` | Play → Role | Role inclusion |
| `ans:dependsOn` | Role → Role | Role dependency |
| `ans:name` | any → string | Entity name |
| `ans:sourceFile` | any → string | Source file path |

### URI Patterns

| Entity | Pattern | Example |
|--------|---------|---------|
| Host | `ans:host/<hostname>` | `ans:host/web01` |
| Group | `ans:group/<name>` | `ans:group/webservers` |
| Variable | `ans:var/<owner>/<key>` | `ans:var/host/web01/http_port` |
| Playbook | `ans:playbook/<rel_path>` | `ans:playbook/site.yml` |
| Play | `ans:play/<playbook>/<idx>` | `ans:play/site.yml/0` |
| Task | `ans:task/<ctx>/<idx>` | `ans:task/site.yml/0/3` |
| Role | `ans:role/<name>` | `ans:role/nginx` |
| Handler | `ans:handler/<ctx>/<slug>` | `ans:handler/nginx/restart_nginx` |

## Two MCP Tools

### `load_inventory`
- **Input**: `path` (file or directory)
- **Parses**: INI inventory, YAML inventory, `host_vars/`, `group_vars/`
- **Output**: `host:Host`, `ans:HostGroup`, `ans:Variable` triples

### `load_ansible`
- **Input**: `path` (project dir), optional `inventory_path`
- **Parses**: inventory + playbooks + roles
- **Output**: All of the above plus `ans:Playbook`, `ans:Play`, `ans:Task`, `ans:Role`, `ans:Handler`, `ans:Template`

## Implementation Steps

1. Add `serde_yaml = "0.9"` to Cargo.toml
2. Create `loaders/ansible.rs` — namespace helpers, error types, result structs, intermediate data model
3. INI inventory parser + quad generation
4. YAML inventory parser
5. `host_vars/` and `group_vars/` directory parsing
6. Create `tools/ansible.rs` — `load_inventory()` function
7. Wire `load_inventory` tool handler in `main.rs` (+ mod.rs updates)
8. Tests: INI parsing, YAML parsing, range expansion, host/group vars, ICAS bridge
9. Playbook parser (plays, tasks)
10. Role parser (tasks, handlers, defaults, templates, meta/dependencies)
11. `load_ansible_project_quads()` orchestrator
12. `load_ansible()` tool function + handler in main.rs
13. Tests: playbook, role, full project
14. Update SPECIFICATIONS.md
