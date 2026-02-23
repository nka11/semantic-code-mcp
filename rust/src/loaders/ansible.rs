use oxigraph::model::{GraphName, Literal, NamedNode, Quad, Term};
use std::collections::HashMap;
use std::fmt;
use std::path::Path;

// --- Namespaces ---

pub const ANS_NS: &str = "https://ds-labs.org/ansible#";
pub const HOST_NS: &str = "http://www.invincea.com/ontologies/icas/1.0/host#";
const RDF_TYPE: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";

fn sanitize(s: &str) -> String {
    super::sanitize_iri_local(s)
}

pub fn ans_node(local: &str) -> NamedNode {
    NamedNode::new(format!("{ANS_NS}{}", sanitize(local))).unwrap()
}

fn host_node(local: &str) -> NamedNode {
    NamedNode::new(format!("{HOST_NS}{}", sanitize(local))).unwrap()
}

fn rdf_type_node() -> NamedNode {
    NamedNode::new(RDF_TYPE).unwrap()
}

fn lit(value: &str) -> Term {
    Term::Literal(Literal::new_simple_literal(value))
}

fn dg() -> GraphName {
    GraphName::DefaultGraph
}

fn q(subject: &NamedNode, predicate: &NamedNode, object: Term) -> Quad {
    Quad::new(subject.clone(), predicate.clone(), object, dg())
}

fn q_type(subject: &NamedNode, class: &NamedNode) -> Quad {
    Quad::new(
        subject.clone(),
        rdf_type_node(),
        Term::NamedNode(class.clone()),
        dg(),
    )
}

fn q_link(subject: &NamedNode, predicate: &NamedNode, object: &NamedNode) -> Quad {
    Quad::new(
        subject.clone(),
        predicate.clone(),
        Term::NamedNode(object.clone()),
        dg(),
    )
}

// --- Error type ---

#[derive(Debug)]
pub enum AnsibleLoadError {
    Io(std::io::Error),
    Yaml(String),
    Parse(String),
}

impl fmt::Display for AnsibleLoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AnsibleLoadError::Io(e) => write!(f, "I/O error: {e}"),
            AnsibleLoadError::Yaml(e) => write!(f, "YAML parse error: {e}"),
            AnsibleLoadError::Parse(e) => write!(f, "Parse error: {e}"),
        }
    }
}

impl std::error::Error for AnsibleLoadError {}

impl From<std::io::Error> for AnsibleLoadError {
    fn from(e: std::io::Error) -> Self {
        AnsibleLoadError::Io(e)
    }
}

impl From<serde_yaml::Error> for AnsibleLoadError {
    fn from(e: serde_yaml::Error) -> Self {
        AnsibleLoadError::Yaml(e.to_string())
    }
}

// --- Result types ---

pub struct InventoryResult {
    pub quads: Vec<Quad>,
    pub host_count: u32,
    pub group_count: u32,
    pub var_count: u32,
}

pub struct AnsibleProjectResult {
    pub quads: Vec<Quad>,
    pub host_count: u32,
    pub group_count: u32,
    pub var_count: u32,
    pub playbook_count: u32,
    pub play_count: u32,
    pub task_count: u32,
    pub role_count: u32,
    pub handler_count: u32,
    pub template_count: u32,
}

// --- Intermediate data model ---

#[derive(Debug, Clone, Default)]
struct InventoryData {
    hosts: HashMap<String, HostData>,
    groups: HashMap<String, GroupData>,
}

#[derive(Debug, Clone, Default)]
struct HostData {
    vars: HashMap<String, String>,
}

#[derive(Debug, Clone, Default)]
struct GroupData {
    hosts: Vec<String>,
    children: Vec<String>,
    vars: HashMap<String, String>,
}

impl InventoryData {
    fn ensure_host(&mut self, name: &str) {
        self.hosts.entry(name.to_string()).or_default();
    }

    fn ensure_group(&mut self, name: &str) {
        self.groups.entry(name.to_string()).or_default();
    }

    fn add_host_to_group(&mut self, host: &str, group: &str) {
        self.ensure_host(host);
        self.ensure_group(group);
        let g = self.groups.get_mut(group).unwrap();
        if !g.hosts.contains(&host.to_string()) {
            g.hosts.push(host.to_string());
        }
    }

    fn set_host_var(&mut self, host: &str, key: &str, value: &str) {
        self.ensure_host(host);
        self.hosts
            .get_mut(host)
            .unwrap()
            .vars
            .insert(key.to_string(), value.to_string());
    }

    fn set_group_var(&mut self, group: &str, key: &str, value: &str) {
        self.ensure_group(group);
        self.groups
            .get_mut(group)
            .unwrap()
            .vars
            .insert(key.to_string(), value.to_string());
    }

    fn add_child_group(&mut self, parent: &str, child: &str) {
        self.ensure_group(parent);
        self.ensure_group(child);
        let g = self.groups.get_mut(parent).unwrap();
        if !g.children.contains(&child.to_string()) {
            g.children.push(child.to_string());
        }
    }
}

// --- Range expansion ---

/// Expand Ansible host range patterns like `web[01:05]` → `web01, web02, ..., web05`
fn expand_host_pattern(pattern: &str) -> Vec<String> {
    if let Some(bracket_start) = pattern.find('[') {
        if let Some(bracket_end) = pattern[bracket_start..].find(']') {
            let bracket_end = bracket_start + bracket_end;
            let prefix = &pattern[..bracket_start];
            let suffix = &pattern[bracket_end + 1..];
            let range_str = &pattern[bracket_start + 1..bracket_end];

            if let Some(colon_pos) = range_str.find(':') {
                let start_str = &range_str[..colon_pos];
                let end_str = &range_str[colon_pos + 1..];

                // Determine padding width from the start string
                let width = start_str.len();

                if let (Ok(start), Ok(end)) = (start_str.parse::<u32>(), end_str.parse::<u32>()) {
                    let mut results = Vec::new();
                    for i in start..=end {
                        let expanded = format!("{prefix}{i:0>width$}{suffix}");
                        // Recursively expand in case of multiple ranges
                        results.extend(expand_host_pattern(&expanded));
                    }
                    return results;
                }

                // Alphabetic range (a:z)
                if start_str.len() == 1 && end_str.len() == 1 {
                    let start_ch = start_str.chars().next().unwrap();
                    let end_ch = end_str.chars().next().unwrap();
                    if start_ch.is_ascii_alphabetic() && end_ch.is_ascii_alphabetic() {
                        let mut results = Vec::new();
                        for ch in start_ch..=end_ch {
                            let expanded = format!("{prefix}{ch}{suffix}");
                            results.extend(expand_host_pattern(&expanded));
                        }
                        return results;
                    }
                }
            }
        }
    }
    vec![pattern.to_string()]
}

// --- INI inventory parser ---

fn parse_ini_inventory(content: &str, data: &mut InventoryData) {
    enum Section {
        None,
        Group(String),
        Children(String),
        Vars(String),
    }

    let mut section = Section::None;

    // All standalone hosts (outside any group) go into the implicit "all" group
    // but we handle them specially below.

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }

        // Section header: [groupname], [groupname:children], [groupname:vars]
        if line.starts_with('[') && line.ends_with(']') {
            let inner = &line[1..line.len() - 1];
            if let Some(colon) = inner.find(':') {
                let group_name = &inner[..colon];
                let qualifier = &inner[colon + 1..];
                match qualifier {
                    "children" => {
                        data.ensure_group(group_name);
                        section = Section::Children(group_name.to_string());
                    }
                    "vars" => {
                        data.ensure_group(group_name);
                        section = Section::Vars(group_name.to_string());
                    }
                    _ => {
                        data.ensure_group(inner);
                        section = Section::Group(inner.to_string());
                    }
                }
            } else {
                data.ensure_group(inner);
                section = Section::Group(inner.to_string());
            }
            continue;
        }

        match &section {
            Section::None => {
                // Ungrouped host line
                parse_ini_host_line(line, data, None);
            }
            Section::Group(group) => {
                let group = group.clone();
                parse_ini_host_line(line, data, Some(&group));
            }
            Section::Children(parent) => {
                let child = line.split_whitespace().next().unwrap_or(line);
                let parent = parent.clone();
                data.add_child_group(&parent, child);
            }
            Section::Vars(group) => {
                let group = group.clone();
                if let Some(eq) = line.find('=') {
                    let key = line[..eq].trim();
                    let value = line[eq + 1..].trim();
                    data.set_group_var(&group, key, value);
                }
            }
        }
    }
}

fn parse_ini_host_line(line: &str, data: &mut InventoryData, group: Option<&str>) {
    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.is_empty() {
        return;
    }

    let host_pattern = parts[0];
    let hosts = expand_host_pattern(host_pattern);

    for host in &hosts {
        data.ensure_host(host);
        if let Some(g) = group {
            data.add_host_to_group(host, g);
        }

        // Parse inline key=value pairs
        for part in &parts[1..] {
            if let Some(eq) = part.find('=') {
                let key = &part[..eq];
                let value = &part[eq + 1..];
                data.set_host_var(host, key, value);
            }
        }
    }
}

// --- YAML inventory parser ---

fn parse_yaml_inventory(
    value: &serde_yaml::Value,
    data: &mut InventoryData,
) -> Result<(), AnsibleLoadError> {
    let map = value
        .as_mapping()
        .ok_or_else(|| AnsibleLoadError::Parse("YAML inventory must be a mapping".into()))?;

    for (key, val) in map {
        let group_name = key
            .as_str()
            .ok_or_else(|| AnsibleLoadError::Parse("Group name must be a string".into()))?;
        data.ensure_group(group_name);
        parse_yaml_group(group_name, val, data)?;
    }
    Ok(())
}

fn parse_yaml_group(
    group_name: &str,
    value: &serde_yaml::Value,
    data: &mut InventoryData,
) -> Result<(), AnsibleLoadError> {
    if value.is_null() {
        return Ok(());
    }
    let map = match value.as_mapping() {
        Some(m) => m,
        None => return Ok(()),
    };

    // hosts:
    if let Some(hosts_val) = map.get(serde_yaml::Value::String("hosts".into())) {
        if let Some(hosts_map) = hosts_val.as_mapping() {
            for (hk, hv) in hosts_map {
                if let Some(hostname) = hk.as_str() {
                    let expanded = expand_host_pattern(hostname);
                    for host in &expanded {
                        data.add_host_to_group(host, group_name);
                        // Host vars from YAML
                        if let Some(vars_map) = hv.as_mapping() {
                            for (vk, vv) in vars_map {
                                if let Some(k) = vk.as_str() {
                                    let v = yaml_value_to_string(vv);
                                    data.set_host_var(host, k, &v);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // vars:
    if let Some(vars_val) = map.get(serde_yaml::Value::String("vars".into())) {
        if let Some(vars_map) = vars_val.as_mapping() {
            for (vk, vv) in vars_map {
                if let Some(k) = vk.as_str() {
                    let v = yaml_value_to_string(vv);
                    data.set_group_var(group_name, k, &v);
                }
            }
        }
    }

    // children:
    if let Some(children_val) = map.get(serde_yaml::Value::String("children".into())) {
        if let Some(children_map) = children_val.as_mapping() {
            for (ck, cv) in children_map {
                if let Some(child_name) = ck.as_str() {
                    data.add_child_group(group_name, child_name);
                    parse_yaml_group(child_name, cv, data)?;
                }
            }
        }
    }

    Ok(())
}

fn yaml_value_to_string(value: &serde_yaml::Value) -> String {
    match value {
        serde_yaml::Value::String(s) => s.clone(),
        serde_yaml::Value::Bool(b) => b.to_string(),
        serde_yaml::Value::Number(n) => n.to_string(),
        serde_yaml::Value::Null => "null".to_string(),
        // Complex values serialized as JSON
        other => serde_json::to_string(other).unwrap_or_else(|_| format!("{other:?}")),
    }
}

// --- host_vars / group_vars directory parsing ---

fn load_host_vars_dir(dir: &Path, data: &mut InventoryData) -> Result<(), AnsibleLoadError> {
    if !dir.is_dir() {
        return Ok(());
    }
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_string();
        if name.is_empty() {
            continue;
        }

        if path.is_file() && is_yaml_file(&path) {
            let content = std::fs::read_to_string(&path)?;
            let value: serde_yaml::Value = serde_yaml::from_str(&content)?;
            if let Some(map) = value.as_mapping() {
                data.ensure_host(&name);
                for (k, v) in map {
                    if let Some(key) = k.as_str() {
                        data.set_host_var(&name, key, &yaml_value_to_string(v));
                    }
                }
            }
        } else if path.is_dir() {
            // Directory-style: host_vars/hostname/*.yml
            data.ensure_host(&name);
            load_vars_from_dir(&path, |k, v| data.set_host_var(&name, k, v))?;
        }
    }
    Ok(())
}

fn load_group_vars_dir(dir: &Path, data: &mut InventoryData) -> Result<(), AnsibleLoadError> {
    if !dir.is_dir() {
        return Ok(());
    }
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_string();
        if name.is_empty() {
            continue;
        }

        if path.is_file() && is_yaml_file(&path) {
            let content = std::fs::read_to_string(&path)?;
            let value: serde_yaml::Value = serde_yaml::from_str(&content)?;
            if let Some(map) = value.as_mapping() {
                data.ensure_group(&name);
                for (k, v) in map {
                    if let Some(key) = k.as_str() {
                        data.set_group_var(&name, key, &yaml_value_to_string(v));
                    }
                }
            }
        } else if path.is_dir() {
            data.ensure_group(&name);
            load_vars_from_dir(&path, |k, v| data.set_group_var(&name, k, v))?;
        }
    }
    Ok(())
}

fn load_vars_from_dir(
    dir: &Path,
    mut set_var: impl FnMut(&str, &str),
) -> Result<(), AnsibleLoadError> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() && is_yaml_file(&path) {
            let content = std::fs::read_to_string(&path)?;
            let value: serde_yaml::Value = serde_yaml::from_str(&content)?;
            if let Some(map) = value.as_mapping() {
                for (k, v) in map {
                    if let Some(key) = k.as_str() {
                        set_var(key, &yaml_value_to_string(v));
                    }
                }
            }
        }
    }
    Ok(())
}

fn is_yaml_file(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()),
        Some("yml" | "yaml")
    )
}

// --- Format detection ---

fn is_yaml_content(content: &str) -> bool {
    let trimmed = content.trim_start();
    trimmed.starts_with("---")
        || trimmed.starts_with("all:")
        || trimmed.starts_with("ungrouped:")
        || (trimmed.contains(':') && !trimmed.starts_with('['))
}

// --- Inventory quad generation ---

fn inventory_data_to_quads(data: &InventoryData) -> (Vec<Quad>, u32, u32, u32) {
    let mut quads = Vec::new();
    let host_type = host_node("Host");
    let host_name_pred = host_node("hostName");
    let group_type = ans_node("HostGroup");
    let var_type = ans_node("Variable");
    let member_of_pred = ans_node("memberOf");
    let has_host_pred = ans_node("hasHost");
    let child_group_pred = ans_node("childGroup");
    let has_variable_pred = ans_node("hasVariable");
    let var_name_pred = ans_node("variableName");
    let var_value_pred = ans_node("variableValue");
    let ansible_host_pred = ans_node("ansibleHost");
    let name_pred = ans_node("name");

    let mut host_count = 0u32;
    let mut group_count = 0u32;
    let mut var_count = 0u32;

    // Hosts
    for (hostname, host_data) in &data.hosts {
        let host_uri = ans_node(&format!("host/{hostname}"));
        quads.push(q_type(&host_uri, &host_type));
        quads.push(q(&host_uri, &host_name_pred, lit(hostname)));
        quads.push(q(&host_uri, &name_pred, lit(hostname)));
        host_count += 1;

        // Host variables
        for (key, value) in &host_data.vars {
            if key == "ansible_host" {
                quads.push(q(&host_uri, &ansible_host_pred, lit(value)));
            }
            let var_uri = ans_node(&format!("var/host/{hostname}/{key}"));
            quads.push(q_type(&var_uri, &var_type));
            quads.push(q(&var_uri, &var_name_pred, lit(key)));
            quads.push(q(&var_uri, &var_value_pred, lit(value)));
            quads.push(q_link(&host_uri, &has_variable_pred, &var_uri));
            var_count += 1;
        }
    }

    // Groups
    for (group_name, group_data) in &data.groups {
        let group_uri = ans_node(&format!("group/{group_name}"));
        quads.push(q_type(&group_uri, &group_type));
        quads.push(q(&group_uri, &name_pred, lit(group_name)));
        group_count += 1;

        // Group → host membership
        for hostname in &group_data.hosts {
            let host_uri = ans_node(&format!("host/{hostname}"));
            quads.push(q_link(&group_uri, &has_host_pred, &host_uri));
            quads.push(q_link(&host_uri, &member_of_pred, &group_uri));
        }

        // Child groups
        for child in &group_data.children {
            let child_uri = ans_node(&format!("group/{child}"));
            quads.push(q_link(&group_uri, &child_group_pred, &child_uri));
        }

        // Group variables
        for (key, value) in &group_data.vars {
            let var_uri = ans_node(&format!("var/group/{group_name}/{key}"));
            quads.push(q_type(&var_uri, &var_type));
            quads.push(q(&var_uri, &var_name_pred, lit(key)));
            quads.push(q(&var_uri, &var_value_pred, lit(value)));
            quads.push(q_link(&group_uri, &has_variable_pred, &var_uri));
            var_count += 1;
        }
    }

    (quads, host_count, group_count, var_count)
}

// --- Public API: load inventory ---

pub fn load_inventory_quads(path: &Path) -> Result<InventoryResult, AnsibleLoadError> {
    let mut data = InventoryData::default();

    if path.is_file() {
        load_inventory_file(path, &mut data)?;
    } else if path.is_dir() {
        // Directory inventory: look for hosts file + host_vars/ + group_vars/
        let hosts_file = path.join("hosts");
        let hosts_yml = path.join("hosts.yml");
        let hosts_yaml = path.join("hosts.yaml");

        if hosts_file.is_file() {
            load_inventory_file(&hosts_file, &mut data)?;
        } else if hosts_yml.is_file() {
            load_inventory_file(&hosts_yml, &mut data)?;
        } else if hosts_yaml.is_file() {
            load_inventory_file(&hosts_yaml, &mut data)?;
        }

        // Also load any YAML/INI files directly in the inventory dir (besides hosts)
        // This handles inventory dirs with multiple files
        load_host_vars_dir(&path.join("host_vars"), &mut data)?;
        load_group_vars_dir(&path.join("group_vars"), &mut data)?;
    } else {
        return Err(AnsibleLoadError::Parse(format!(
            "Path does not exist: {}",
            path.display()
        )));
    }

    let (quads, host_count, group_count, var_count) = inventory_data_to_quads(&data);

    Ok(InventoryResult {
        quads,
        host_count,
        group_count,
        var_count,
    })
}

fn load_inventory_file(path: &Path, data: &mut InventoryData) -> Result<(), AnsibleLoadError> {
    let content = std::fs::read_to_string(path)?;
    if is_yaml_content(&content) {
        let value: serde_yaml::Value = serde_yaml::from_str(&content)?;
        parse_yaml_inventory(&value, data)?;
    } else {
        parse_ini_inventory(&content, data);
    }
    Ok(())
}

// --- Playbook parsing ---

/// Known task-level keys that are NOT module names
const TASK_KEYWORDS: &[&str] = &[
    "name",
    "when",
    "register",
    "notify",
    "tags",
    "become",
    "become_user",
    "become_method",
    "ignore_errors",
    "changed_when",
    "failed_when",
    "loop",
    "with_items",
    "with_dict",
    "with_fileglob",
    "with_first_found",
    "with_together",
    "with_flattened",
    "with_indexed_items",
    "with_nested",
    "with_random_choice",
    "with_sequence",
    "with_subelements",
    "until",
    "retries",
    "delay",
    "no_log",
    "environment",
    "vars",
    "delegate_to",
    "delegate_facts",
    "any_errors_fatal",
    "run_once",
    "serial",
    "throttle",
    "timeout",
    "check_mode",
    "diff",
    "connection",
    "collections",
    "module_defaults",
    "listen",
    "block",
    "rescue",
    "always",
    "args",
    "async",
    "poll",
    "local_action",
    "action",
];

fn detect_module(task: &serde_yaml::Mapping) -> Option<String> {
    for (k, _v) in task {
        if let Some(key) = k.as_str() {
            if !TASK_KEYWORDS.contains(&key) {
                return Some(key.to_string());
            }
        }
    }
    None
}

fn slugify(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' || c == '-' {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect()
}

struct PlaybookParseResult {
    quads: Vec<Quad>,
    play_count: u32,
    task_count: u32,
    role_refs: Vec<String>,
}

fn parse_playbook(rel_path: &str, content: &str) -> Result<PlaybookParseResult, AnsibleLoadError> {
    let docs: Vec<serde_yaml::Value> = serde_yaml::from_str(content)?;

    // A playbook is a list of plays
    let plays = match docs.as_slice() {
        [serde_yaml::Value::Sequence(seq)] => seq.clone(),
        _ => {
            // Some YAML parsers return the sequence directly
            let val: serde_yaml::Value = serde_yaml::from_str(content)?;
            match val {
                serde_yaml::Value::Sequence(seq) => seq,
                _ => {
                    return Err(AnsibleLoadError::Parse(
                        "Playbook must be a YAML list of plays".into(),
                    ))
                }
            }
        }
    };

    let playbook_uri = ans_node(&format!("playbook/{rel_path}"));
    let playbook_type = ans_node("Playbook");
    let play_type = ans_node("Play");
    let task_type = ans_node("Task");
    let has_play_pred = ans_node("hasPlay");
    let target_hosts_pred = ans_node("targetHosts");
    let has_task_pred = ans_node("hasTask");
    let module_pred = ans_node("module");
    let name_pred = ans_node("name");
    let source_file_pred = ans_node("sourceFile");
    let uses_role_pred = ans_node("usesRole");

    let mut quads = Vec::new();
    let mut play_count = 0u32;
    let mut task_count = 0u32;
    let mut role_refs = Vec::new();

    quads.push(q_type(&playbook_uri, &playbook_type));
    quads.push(q(&playbook_uri, &name_pred, lit(rel_path)));
    quads.push(q(&playbook_uri, &source_file_pred, lit(rel_path)));

    for (play_idx, play_val) in plays.iter().enumerate() {
        let play_map = match play_val.as_mapping() {
            Some(m) => m,
            None => continue,
        };

        let play_uri = ans_node(&format!("play/{rel_path}/{play_idx}"));
        quads.push(q_type(&play_uri, &play_type));
        quads.push(q_link(&playbook_uri, &has_play_pred, &play_uri));
        quads.push(q(&play_uri, &source_file_pred, lit(rel_path)));

        if let Some(name_val) = play_map.get(serde_yaml::Value::String("name".into())) {
            if let Some(name) = name_val.as_str() {
                quads.push(q(&play_uri, &name_pred, lit(name)));
            }
        }

        if let Some(hosts_val) = play_map.get(serde_yaml::Value::String("hosts".into())) {
            let hosts_str = yaml_value_to_string(hosts_val);
            quads.push(q(&play_uri, &target_hosts_pred, lit(&hosts_str)));
        }

        play_count += 1;

        // Tasks
        if let Some(tasks_val) = play_map.get(serde_yaml::Value::String("tasks".into())) {
            if let Some(tasks) = tasks_val.as_sequence() {
                for (task_idx, task_val) in tasks.iter().enumerate() {
                    if let Some(task_map) = task_val.as_mapping() {
                        let task_uri = ans_node(&format!("task/{rel_path}/{play_idx}/{task_idx}"));
                        quads.push(q_type(&task_uri, &task_type));
                        quads.push(q_link(&play_uri, &has_task_pred, &task_uri));
                        quads.push(q(&task_uri, &source_file_pred, lit(rel_path)));

                        if let Some(name_val) =
                            task_map.get(serde_yaml::Value::String("name".into()))
                        {
                            if let Some(name) = name_val.as_str() {
                                quads.push(q(&task_uri, &name_pred, lit(name)));
                            }
                        }

                        if let Some(module_name) = detect_module(task_map) {
                            quads.push(q(&task_uri, &module_pred, lit(&module_name)));
                        }

                        task_count += 1;
                    }
                }
            }
        }

        // Handlers in playbook
        if let Some(handlers_val) = play_map.get(serde_yaml::Value::String("handlers".into())) {
            if let Some(handlers) = handlers_val.as_sequence() {
                for (handler_idx, handler_val) in handlers.iter().enumerate() {
                    if let Some(handler_map) = handler_val.as_mapping() {
                        let handler_name = handler_map
                            .get(serde_yaml::Value::String("name".into()))
                            .and_then(|v| v.as_str())
                            .unwrap_or("unnamed");
                        let slug = slugify(handler_name);
                        let handler_uri =
                            ans_node(&format!("handler/{rel_path}/{play_idx}/{slug}"));
                        let handler_type = ans_node("Handler");
                        quads.push(q_type(&handler_uri, &handler_type));
                        quads.push(q(&handler_uri, &name_pred, lit(handler_name)));
                        quads.push(q(&handler_uri, &source_file_pred, lit(rel_path)));

                        if let Some(module_name) = detect_module(handler_map) {
                            quads.push(q(&handler_uri, &module_pred, lit(&module_name)));
                        }

                        // Link handler to play
                        let has_handler_pred = ans_node("hasHandler");
                        quads.push(q_link(&play_uri, &has_handler_pred, &handler_uri));

                        // Count handlers as tasks for simplicity in the task counter
                        let _ = handler_idx;
                    }
                }
            }
        }

        // Roles
        if let Some(roles_val) = play_map.get(serde_yaml::Value::String("roles".into())) {
            if let Some(roles) = roles_val.as_sequence() {
                for role_val in roles {
                    let role_name = match role_val {
                        serde_yaml::Value::String(s) => s.clone(),
                        serde_yaml::Value::Mapping(m) => m
                            .get(serde_yaml::Value::String("role".into()))
                            .or_else(|| m.get(serde_yaml::Value::String("name".into())))
                            .and_then(|v| v.as_str())
                            .unwrap_or_default()
                            .to_string(),
                        _ => continue,
                    };
                    if role_name.is_empty() {
                        continue;
                    }
                    let role_uri = ans_node(&format!("role/{role_name}"));
                    quads.push(q_link(&play_uri, &uses_role_pred, &role_uri));
                    if !role_refs.contains(&role_name) {
                        role_refs.push(role_name);
                    }
                }
            }
        }
    }

    Ok(PlaybookParseResult {
        quads,
        play_count,
        task_count,
        role_refs,
    })
}

// --- Role parsing ---

struct RoleParsResult {
    quads: Vec<Quad>,
    task_count: u32,
    handler_count: u32,
    template_count: u32,
}

fn parse_role(role_name: &str, role_dir: &Path) -> Result<RoleParsResult, AnsibleLoadError> {
    let role_uri = ans_node(&format!("role/{role_name}"));
    let role_type_node = ans_node("Role");
    let name_pred = ans_node("name");
    let source_file_pred = ans_node("sourceFile");
    let has_task_pred = ans_node("hasTask");
    let task_type = ans_node("Task");
    let module_pred = ans_node("module");
    let handler_type = ans_node("Handler");
    let has_handler_pred = ans_node("hasHandler");
    let template_type = ans_node("Template");
    let has_template_pred = ans_node("hasTemplate");
    let depends_on_pred = ans_node("dependsOn");
    let has_variable_pred = ans_node("hasVariable");
    let var_type = ans_node("Variable");
    let var_name_pred = ans_node("variableName");
    let var_value_pred = ans_node("variableValue");

    let mut quads = Vec::new();
    let mut task_count = 0u32;
    let mut handler_count = 0u32;
    let mut template_count = 0u32;

    quads.push(q_type(&role_uri, &role_type_node));
    quads.push(q(&role_uri, &name_pred, lit(role_name)));
    quads.push(q(
        &role_uri,
        &source_file_pred,
        lit(&format!("roles/{role_name}")),
    ));

    // Tasks: roles/<name>/tasks/main.yml
    let tasks_dir = role_dir.join("tasks");
    if tasks_dir.is_dir() {
        for entry in std::fs::read_dir(&tasks_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_file() && is_yaml_file(&path) {
                let content = std::fs::read_to_string(&path)?;
                let tasks = parse_yaml_task_list(&content)?;
                let file_name = path.file_name().unwrap().to_string_lossy();
                for (idx, task_map) in tasks.iter().enumerate() {
                    let task_uri =
                        ans_node(&format!("task/roles/{role_name}/tasks/{file_name}/{idx}"));
                    quads.push(q_type(&task_uri, &task_type));
                    quads.push(q_link(&role_uri, &has_task_pred, &task_uri));
                    quads.push(q(
                        &task_uri,
                        &source_file_pred,
                        lit(&format!("roles/{role_name}/tasks/{file_name}")),
                    ));
                    if let Some(name_val) = task_map.get(serde_yaml::Value::String("name".into())) {
                        if let Some(name) = name_val.as_str() {
                            quads.push(q(&task_uri, &name_pred, lit(name)));
                        }
                    }
                    if let Some(module_name) = detect_module(task_map) {
                        quads.push(q(&task_uri, &module_pred, lit(&module_name)));
                    }
                    task_count += 1;
                }
            }
        }
    }

    // Handlers: roles/<name>/handlers/main.yml
    let handlers_dir = role_dir.join("handlers");
    if handlers_dir.is_dir() {
        for entry in std::fs::read_dir(&handlers_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_file() && is_yaml_file(&path) {
                let content = std::fs::read_to_string(&path)?;
                let handlers = parse_yaml_task_list(&content)?;
                for handler_map in &handlers {
                    let handler_name = handler_map
                        .get(serde_yaml::Value::String("name".into()))
                        .and_then(|v| v.as_str())
                        .unwrap_or("unnamed");
                    let slug = slugify(handler_name);
                    let handler_uri = ans_node(&format!("handler/{role_name}/{slug}"));
                    quads.push(q_type(&handler_uri, &handler_type));
                    quads.push(q(&handler_uri, &name_pred, lit(handler_name)));
                    quads.push(q(
                        &handler_uri,
                        &source_file_pred,
                        lit(&format!("roles/{role_name}/handlers/main.yml")),
                    ));
                    if let Some(module_name) = detect_module(handler_map) {
                        quads.push(q(&handler_uri, &module_pred, lit(&module_name)));
                    }
                    quads.push(q_link(&role_uri, &has_handler_pred, &handler_uri));
                    handler_count += 1;
                }
            }
        }
    }

    // Templates: roles/<name>/templates/
    let templates_dir = role_dir.join("templates");
    if templates_dir.is_dir() {
        for entry in std::fs::read_dir(&templates_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_file() {
                let file_name = path.file_name().unwrap().to_string_lossy();
                let tmpl_uri = ans_node(&format!("template/{role_name}/{file_name}"));
                quads.push(q_type(&tmpl_uri, &template_type));
                quads.push(q(&tmpl_uri, &name_pred, lit(&file_name)));
                quads.push(q(
                    &tmpl_uri,
                    &source_file_pred,
                    lit(&format!("roles/{role_name}/templates/{file_name}")),
                ));
                quads.push(q_link(&role_uri, &has_template_pred, &tmpl_uri));
                template_count += 1;
            }
        }
    }

    // Defaults: roles/<name>/defaults/main.yml
    let defaults_main = role_dir.join("defaults/main.yml");
    if defaults_main.is_file() {
        let content = std::fs::read_to_string(&defaults_main)?;
        let value: serde_yaml::Value = serde_yaml::from_str(&content)?;
        if let Some(map) = value.as_mapping() {
            for (k, v) in map {
                if let Some(key) = k.as_str() {
                    let var_uri = ans_node(&format!("var/role/{role_name}/{key}"));
                    quads.push(q_type(&var_uri, &var_type));
                    quads.push(q(&var_uri, &var_name_pred, lit(key)));
                    quads.push(q(&var_uri, &var_value_pred, lit(&yaml_value_to_string(v))));
                    quads.push(q_link(&role_uri, &has_variable_pred, &var_uri));
                }
            }
        }
    }

    // Meta/dependencies: roles/<name>/meta/main.yml
    let meta_main = role_dir.join("meta/main.yml");
    if meta_main.is_file() {
        let content = std::fs::read_to_string(&meta_main)?;
        let value: serde_yaml::Value = serde_yaml::from_str(&content)?;
        if let Some(map) = value.as_mapping() {
            if let Some(deps) = map.get(serde_yaml::Value::String("dependencies".into())) {
                if let Some(deps_seq) = deps.as_sequence() {
                    for dep in deps_seq {
                        let dep_name = match dep {
                            serde_yaml::Value::String(s) => s.clone(),
                            serde_yaml::Value::Mapping(m) => m
                                .get(serde_yaml::Value::String("role".into()))
                                .or_else(|| m.get(serde_yaml::Value::String("name".into())))
                                .and_then(|v| v.as_str())
                                .unwrap_or_default()
                                .to_string(),
                            _ => continue,
                        };
                        if !dep_name.is_empty() {
                            let dep_uri = ans_node(&format!("role/{dep_name}"));
                            quads.push(q_link(&role_uri, &depends_on_pred, &dep_uri));
                        }
                    }
                }
            }
        }
    }

    Ok(RoleParsResult {
        quads,
        task_count,
        handler_count,
        template_count,
    })
}

fn parse_yaml_task_list(content: &str) -> Result<Vec<serde_yaml::Mapping>, AnsibleLoadError> {
    let value: serde_yaml::Value = serde_yaml::from_str(content)?;
    match value {
        serde_yaml::Value::Sequence(seq) => Ok(seq
            .into_iter()
            .filter_map(|v| match v {
                serde_yaml::Value::Mapping(m) => Some(m),
                _ => None,
            })
            .collect()),
        serde_yaml::Value::Null => Ok(Vec::new()),
        _ => Err(AnsibleLoadError::Parse(
            "Task file must be a YAML list".into(),
        )),
    }
}

// --- Public API: load full Ansible project ---

pub fn load_ansible_project_quads(
    project_dir: &Path,
    inventory_path: Option<&Path>,
) -> Result<AnsibleProjectResult, AnsibleLoadError> {
    let mut all_quads = Vec::new();
    let mut host_count = 0u32;
    let mut group_count = 0u32;
    let mut var_count = 0u32;
    let mut playbook_count = 0u32;
    let mut play_count = 0u32;
    let mut task_count = 0u32;
    let mut role_count = 0u32;
    let mut handler_count = 0u32;
    let mut template_count = 0u32;

    // 1. Load inventory
    let default_inv = project_dir.join("inventory");
    let inv_path = inventory_path.unwrap_or(&default_inv);
    // Try common inventory locations
    let inv_candidates = if inv_path.exists() {
        vec![inv_path.to_path_buf()]
    } else {
        let mut candidates = Vec::new();
        for name in &[
            "inventory",
            "inventory.ini",
            "inventory.yml",
            "hosts",
            "hosts.yml",
        ] {
            let p = project_dir.join(name);
            if p.exists() {
                candidates.push(p);
                break;
            }
        }
        candidates
    };

    for inv in &inv_candidates {
        let result = load_inventory_quads(inv)?;
        host_count += result.host_count;
        group_count += result.group_count;
        var_count += result.var_count;
        all_quads.extend(result.quads);
    }

    // Also check for host_vars/group_vars at project root
    let mut inv_data = InventoryData::default();
    // Reconstruct hosts from already-loaded quads so host_vars can reference them
    load_host_vars_dir(&project_dir.join("host_vars"), &mut inv_data)?;
    load_group_vars_dir(&project_dir.join("group_vars"), &mut inv_data)?;
    if !inv_data.hosts.is_empty() || !inv_data.groups.is_empty() {
        let (extra_quads, h, g, v) = inventory_data_to_quads(&inv_data);
        host_count += h;
        group_count += g;
        var_count += v;
        all_quads.extend(extra_quads);
    }

    // 2. Load playbooks (*.yml at project root)
    let mut all_role_refs = Vec::new();
    for entry in std::fs::read_dir(project_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() && is_yaml_file(&path) {
            let file_name = path.file_name().unwrap().to_string_lossy().to_string();
            // Skip known non-playbook files
            if file_name == "ansible.cfg"
                || file_name.starts_with('.')
                || file_name == "requirements.yml"
            {
                continue;
            }
            let content = std::fs::read_to_string(&path)?;
            // Only parse as playbook if it looks like a list of plays
            let trimmed = content.trim();
            if !trimmed.starts_with('-') && !trimmed.starts_with("---") {
                continue;
            }
            match parse_playbook(&file_name, &content) {
                Ok(result) => {
                    playbook_count += 1;
                    play_count += result.play_count;
                    task_count += result.task_count;
                    all_quads.extend(result.quads);
                    for r in result.role_refs {
                        if !all_role_refs.contains(&r) {
                            all_role_refs.push(r);
                        }
                    }
                }
                Err(_) => {
                    // Not a playbook, skip
                    continue;
                }
            }
        }
    }

    // 3. Load roles
    let roles_dir = project_dir.join("roles");
    if roles_dir.is_dir() {
        for entry in std::fs::read_dir(&roles_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                let role_name = path.file_name().unwrap().to_string_lossy().to_string();
                let result = parse_role(&role_name, &path)?;
                role_count += 1;
                task_count += result.task_count;
                handler_count += result.handler_count;
                template_count += result.template_count;
                all_quads.extend(result.quads);
            }
        }
    }

    // Create role stubs for referenced but not-found roles
    let role_type_node = ans_node("Role");
    let name_pred = ans_node("name");
    for role_name in &all_role_refs {
        let role_uri = ans_node(&format!("role/{role_name}"));
        // Check if we already have this role
        let already_exists = all_quads.iter().any(|q| {
            q.subject == role_uri.clone().into()
                && q.predicate == rdf_type_node()
                && q.object == Term::NamedNode(role_type_node.clone())
        });
        if !already_exists {
            all_quads.push(q_type(&role_uri, &role_type_node));
            all_quads.push(q(&role_uri, &name_pred, lit(role_name)));
            role_count += 1;
        }
    }

    Ok(AnsibleProjectResult {
        quads: all_quads,
        host_count,
        group_count,
        var_count,
        playbook_count,
        play_count,
        task_count,
        role_count,
        handler_count,
        template_count,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- Range expansion tests ---

    #[test]
    fn test_expand_numeric_range() {
        let result = expand_host_pattern("web[01:03]");
        assert_eq!(result, vec!["web01", "web02", "web03"]);
    }

    #[test]
    fn test_expand_no_range() {
        let result = expand_host_pattern("web01");
        assert_eq!(result, vec!["web01"]);
    }

    #[test]
    fn test_expand_alpha_range() {
        let result = expand_host_pattern("db-[a:c]");
        assert_eq!(result, vec!["db-a", "db-b", "db-c"]);
    }

    #[test]
    fn test_expand_with_suffix() {
        let result = expand_host_pattern("web[1:3].example.com");
        assert_eq!(
            result,
            vec!["web1.example.com", "web2.example.com", "web3.example.com"]
        );
    }

    // --- INI parser tests ---

    #[test]
    fn test_ini_basic_groups() {
        let ini = r#"
[webservers]
web01 ansible_host=10.0.0.1
web02 ansible_host=10.0.0.2

[dbservers]
db01

[all:children]
webservers
dbservers

[webservers:vars]
http_port=80
"#;
        let mut data = InventoryData::default();
        parse_ini_inventory(ini, &mut data);

        assert!(data.hosts.contains_key("web01"));
        assert!(data.hosts.contains_key("web02"));
        assert!(data.hosts.contains_key("db01"));
        assert_eq!(
            data.hosts["web01"].vars.get("ansible_host").unwrap(),
            "10.0.0.1"
        );
        assert!(data.groups["webservers"].hosts.contains(&"web01".into()));
        assert!(data.groups["webservers"].hosts.contains(&"web02".into()));
        assert!(data.groups["dbservers"].hosts.contains(&"db01".into()));
        assert!(data.groups["all"].children.contains(&"webservers".into()));
        assert!(data.groups["all"].children.contains(&"dbservers".into()));
        assert_eq!(
            data.groups["webservers"].vars.get("http_port").unwrap(),
            "80"
        );
    }

    #[test]
    fn test_ini_range_expansion() {
        let ini = "[web]\nweb[01:03]\n";
        let mut data = InventoryData::default();
        parse_ini_inventory(ini, &mut data);

        assert!(data.hosts.contains_key("web01"));
        assert!(data.hosts.contains_key("web02"));
        assert!(data.hosts.contains_key("web03"));
        assert_eq!(data.groups["web"].hosts.len(), 3);
    }

    // --- YAML inventory tests ---

    #[test]
    fn test_yaml_inventory() {
        let yaml = r#"
all:
  hosts:
    web01:
      ansible_host: 10.0.0.1
    web02:
      ansible_host: 10.0.0.2
  children:
    webservers:
      hosts:
        web01:
        web02:
    dbservers:
      hosts:
        db01:
          ansible_host: 10.0.1.1
      vars:
        db_port: "5432"
"#;
        let value: serde_yaml::Value = serde_yaml::from_str(yaml).unwrap();
        let mut data = InventoryData::default();
        parse_yaml_inventory(&value, &mut data).unwrap();

        assert!(data.hosts.contains_key("web01"));
        assert!(data.hosts.contains_key("db01"));
        assert_eq!(
            data.hosts["web01"].vars.get("ansible_host").unwrap(),
            "10.0.0.1"
        );
        assert!(data.groups["webservers"].hosts.contains(&"web01".into()));
        assert!(data.groups["dbservers"].hosts.contains(&"db01".into()));
        assert_eq!(
            data.groups["dbservers"].vars.get("db_port").unwrap(),
            "5432"
        );
        assert!(data.groups["all"].children.contains(&"webservers".into()));
    }

    // --- Quad generation tests ---

    #[test]
    fn test_inventory_quad_generation() {
        let mut data = InventoryData::default();
        data.add_host_to_group("web01", "webservers");
        data.set_host_var("web01", "ansible_host", "10.0.0.1");
        data.set_group_var("webservers", "http_port", "80");

        let (quads, host_count, group_count, var_count) = inventory_data_to_quads(&data);

        assert_eq!(host_count, 1);
        assert_eq!(group_count, 1);
        assert_eq!(var_count, 2); // one host var + one group var

        // Verify host:Host type triple (ICAS)
        let host_type = host_node("Host");
        let host_uri = ans_node("host/web01");
        assert!(quads.iter().any(|q| q.subject == host_uri.clone().into()
            && q.predicate == rdf_type_node()
            && q.object == Term::NamedNode(host_type.clone())));

        // Verify ansible_host property
        let ansible_host_pred = ans_node("ansibleHost");
        assert!(quads.iter().any(|q| q.subject == host_uri.clone().into()
            && q.predicate == ansible_host_pred
            && q.object == lit("10.0.0.1")));

        // Verify group membership
        let group_uri = ans_node("group/webservers");
        let member_of = ans_node("memberOf");
        assert!(quads.iter().any(|q| q.subject == host_uri.clone().into()
            && q.predicate == member_of
            && q.object == Term::NamedNode(group_uri.clone())));
    }

    // --- Playbook parsing tests ---

    #[test]
    fn test_playbook_parsing() {
        let content = r#"
---
- name: Configure webservers
  hosts: webservers
  tasks:
    - name: Install nginx
      apt:
        name: nginx
        state: present
    - name: Start nginx
      service:
        name: nginx
        state: started
      notify: restart nginx
  handlers:
    - name: restart nginx
      service:
        name: nginx
        state: restarted
  roles:
    - nginx
    - { role: common, tags: ['common'] }
"#;
        let result = parse_playbook("site.yml", content).unwrap();
        assert_eq!(result.play_count, 1);
        assert_eq!(result.task_count, 2);
        assert!(result.role_refs.contains(&"nginx".to_string()));
        assert!(result.role_refs.contains(&"common".to_string()));

        // Check playbook URI
        let playbook_uri = ans_node("playbook/site.yml");
        assert!(result
            .quads
            .iter()
            .any(|q| q.subject == playbook_uri.clone().into()
                && q.predicate == rdf_type_node()
                && q.object == Term::NamedNode(ans_node("Playbook"))));

        // Check task module detection
        let module_pred = ans_node("module");
        assert!(result
            .quads
            .iter()
            .any(|q| q.predicate == module_pred && q.object == lit("apt")));
        assert!(result
            .quads
            .iter()
            .any(|q| q.predicate == module_pred && q.object == lit("service")));

        // Check handler
        let handler_type = ans_node("Handler");
        assert!(result
            .quads
            .iter()
            .any(|q| q.predicate == rdf_type_node()
                && q.object == Term::NamedNode(handler_type.clone())));
    }

    // --- Role parsing tests ---

    #[test]
    fn test_role_parsing() {
        let dir = tempfile::TempDir::new().unwrap();
        let role_dir = dir.path().join("nginx");
        std::fs::create_dir_all(role_dir.join("tasks")).unwrap();
        std::fs::create_dir_all(role_dir.join("handlers")).unwrap();
        std::fs::create_dir_all(role_dir.join("templates")).unwrap();
        std::fs::create_dir_all(role_dir.join("defaults")).unwrap();
        std::fs::create_dir_all(role_dir.join("meta")).unwrap();

        std::fs::write(
            role_dir.join("tasks/main.yml"),
            r#"---
- name: Install nginx
  apt:
    name: nginx
- name: Copy config
  template:
    src: nginx.conf.j2
    dest: /etc/nginx/nginx.conf
"#,
        )
        .unwrap();

        std::fs::write(
            role_dir.join("handlers/main.yml"),
            r#"---
- name: restart nginx
  service:
    name: nginx
    state: restarted
"#,
        )
        .unwrap();

        std::fs::write(role_dir.join("templates/nginx.conf.j2"), "server { }").unwrap();

        std::fs::write(
            role_dir.join("defaults/main.yml"),
            "---\nnginx_port: 80\nnginx_worker: 4\n",
        )
        .unwrap();

        std::fs::write(
            role_dir.join("meta/main.yml"),
            "---\ndependencies:\n  - common\n  - { role: ssl }\n",
        )
        .unwrap();

        let result = parse_role("nginx", &role_dir).unwrap();
        assert_eq!(result.task_count, 2);
        assert_eq!(result.handler_count, 1);
        assert_eq!(result.template_count, 1);

        // Verify role type
        let role_uri = ans_node("role/nginx");
        assert!(result
            .quads
            .iter()
            .any(|q| q.subject == role_uri.clone().into()
                && q.predicate == rdf_type_node()
                && q.object == Term::NamedNode(ans_node("Role"))));

        // Verify dependencies
        let depends_on = ans_node("dependsOn");
        assert!(result
            .quads
            .iter()
            .any(|q| q.subject == role_uri.clone().into()
                && q.predicate == depends_on
                && q.object == Term::NamedNode(ans_node("role/common"))));
        assert!(result
            .quads
            .iter()
            .any(|q| q.subject == role_uri.clone().into()
                && q.predicate == ans_node("dependsOn")
                && q.object == Term::NamedNode(ans_node("role/ssl"))));

        // Verify default variables
        let var_uri = ans_node("var/role/nginx/nginx_port");
        let var_value_pred = ans_node("variableValue");
        assert!(result
            .quads
            .iter()
            .any(|q| q.subject == var_uri.clone().into()
                && q.predicate == var_value_pred
                && q.object == lit("80")));

        // Verify template
        let tmpl_uri = ans_node("template/nginx/nginx.conf.j2");
        assert!(result
            .quads
            .iter()
            .any(|q| q.subject == tmpl_uri.clone().into()
                && q.predicate == rdf_type_node()
                && q.object == Term::NamedNode(ans_node("Template"))));
    }

    // --- host_vars / group_vars tests ---

    #[test]
    fn test_host_vars_dir() {
        let dir = tempfile::TempDir::new().unwrap();
        let host_vars = dir.path().join("host_vars");
        std::fs::create_dir_all(&host_vars).unwrap();
        std::fs::write(
            host_vars.join("web01.yml"),
            "---\nhttp_port: 8080\nmax_conn: 100\n",
        )
        .unwrap();

        let mut data = InventoryData::default();
        load_host_vars_dir(&host_vars, &mut data).unwrap();

        assert!(data.hosts.contains_key("web01"));
        assert_eq!(data.hosts["web01"].vars.get("http_port").unwrap(), "8080");
        assert_eq!(data.hosts["web01"].vars.get("max_conn").unwrap(), "100");
    }

    #[test]
    fn test_group_vars_dir() {
        let dir = tempfile::TempDir::new().unwrap();
        let group_vars = dir.path().join("group_vars");
        std::fs::create_dir_all(&group_vars).unwrap();
        std::fs::write(
            group_vars.join("webservers.yml"),
            "---\nhttp_port: 80\nproxy: true\n",
        )
        .unwrap();

        let mut data = InventoryData::default();
        load_group_vars_dir(&group_vars, &mut data).unwrap();

        assert!(data.groups.contains_key("webservers"));
        assert_eq!(
            data.groups["webservers"].vars.get("http_port").unwrap(),
            "80"
        );
        assert_eq!(data.groups["webservers"].vars.get("proxy").unwrap(), "true");
    }

    // --- Full inventory loading tests ---

    #[test]
    fn test_load_inventory_ini_file() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(
            dir.path().join("hosts"),
            "[web]\nweb01 ansible_host=10.0.0.1\nweb02\n",
        )
        .unwrap();

        let result = load_inventory_quads(&dir.path().join("hosts")).unwrap();
        assert_eq!(result.host_count, 2);
        assert_eq!(result.group_count, 1);
    }

    #[test]
    fn test_load_inventory_directory() {
        let dir = tempfile::TempDir::new().unwrap();
        let inv_dir = dir.path().join("inventory");
        std::fs::create_dir_all(inv_dir.join("host_vars")).unwrap();
        std::fs::create_dir_all(inv_dir.join("group_vars")).unwrap();
        std::fs::write(inv_dir.join("hosts"), "[web]\nweb01\n[db]\ndb01\n").unwrap();
        std::fs::write(
            inv_dir.join("host_vars/web01.yml"),
            "---\nhttp_port: 8080\n",
        )
        .unwrap();
        std::fs::write(inv_dir.join("group_vars/web.yml"), "---\nenv: production\n").unwrap();

        let result = load_inventory_quads(&inv_dir).unwrap();
        assert!(result.host_count >= 2);
        assert!(result.var_count >= 2);
    }

    // --- Full project tests ---

    #[test]
    fn test_load_ansible_project() {
        let dir = tempfile::TempDir::new().unwrap();

        // Create inventory
        let inv_dir = dir.path().join("inventory");
        std::fs::create_dir_all(&inv_dir).unwrap();
        std::fs::write(inv_dir.join("hosts"), "[web]\nweb01\n[db]\ndb01\n").unwrap();

        // Create playbook
        std::fs::write(
            dir.path().join("site.yml"),
            r#"---
- name: Configure web
  hosts: web
  tasks:
    - name: Install pkg
      apt:
        name: nginx
  roles:
    - nginx
"#,
        )
        .unwrap();

        // Create role
        let role_dir = dir.path().join("roles/nginx");
        std::fs::create_dir_all(role_dir.join("tasks")).unwrap();
        std::fs::write(
            role_dir.join("tasks/main.yml"),
            "---\n- name: Do stuff\n  debug:\n    msg: hello\n",
        )
        .unwrap();

        let result = load_ansible_project_quads(dir.path(), None).unwrap();
        assert!(result.host_count >= 2, "hosts: {}", result.host_count);
        assert!(
            result.playbook_count >= 1,
            "playbooks: {}",
            result.playbook_count
        );
        assert!(result.play_count >= 1, "plays: {}", result.play_count);
        assert!(result.task_count >= 2, "tasks: {}", result.task_count); // 1 in playbook + 1 in role
        assert!(result.role_count >= 1, "roles: {}", result.role_count);
    }
}
