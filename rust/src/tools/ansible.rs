use crate::loaders::ansible::{load_ansible_project_quads, load_inventory_quads};
use oxigraph::store::Store;
use rmcp::model::CallToolResult;

fn tool_error(msg: impl std::fmt::Display) -> CallToolResult {
    CallToolResult::error(vec![rmcp::model::Content::text(msg.to_string())])
}

pub fn load_inventory(store: &Store, path: &str) -> CallToolResult {
    let path = std::path::Path::new(path);
    if !path.exists() {
        return tool_error(format!("Path does not exist: {}", path.display()));
    }

    let result = match load_inventory_quads(path) {
        Ok(r) => r,
        Err(e) => return tool_error(e),
    };

    let quad_count = result.quads.len();
    for quad in &result.quads {
        if let Err(e) = store.insert(quad) {
            return tool_error(format!("Store insert error: {e}"));
        }
    }

    let summary = format!(
        "Loaded inventory: {} hosts, {} groups, {} variables ({} triples).",
        result.host_count, result.group_count, result.var_count, quad_count
    );
    CallToolResult::success(vec![rmcp::model::Content::text(summary)])
}

pub fn load_ansible(store: &Store, path: &str, inventory_path: Option<&str>) -> CallToolResult {
    let project_dir = std::path::Path::new(path);
    if !project_dir.exists() {
        return tool_error(format!("Path does not exist: {}", project_dir.display()));
    }
    if !project_dir.is_dir() {
        return tool_error("Path must be a directory for load_ansible");
    }

    let inv_path = inventory_path.map(std::path::Path::new);
    let result = match load_ansible_project_quads(project_dir, inv_path) {
        Ok(r) => r,
        Err(e) => return tool_error(e),
    };

    let quad_count = result.quads.len();
    for quad in &result.quads {
        if let Err(e) = store.insert(quad) {
            return tool_error(format!("Store insert error: {e}"));
        }
    }

    let mut summary = format!(
        "Loaded Ansible project: {} hosts, {} groups, {} variables",
        result.host_count, result.group_count, result.var_count
    );
    if result.playbook_count > 0 {
        summary.push_str(&format!(
            ", {} playbooks, {} plays, {} tasks",
            result.playbook_count, result.play_count, result.task_count
        ));
    }
    if result.role_count > 0 {
        summary.push_str(&format!(", {} roles", result.role_count));
    }
    if result.handler_count > 0 {
        summary.push_str(&format!(", {} handlers", result.handler_count));
    }
    if result.template_count > 0 {
        summary.push_str(&format!(", {} templates", result.template_count));
    }
    summary.push_str(&format!(" ({} triples).", quad_count));

    CallToolResult::success(vec![rmcp::model::Content::text(summary)])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::sparql::sparql_query;
    use std::fs;
    use tempfile::TempDir;

    fn result_text(result: &CallToolResult) -> &str {
        match &result.content[0].raw {
            rmcp::model::RawContent::Text(t) => &t.text,
            _ => panic!("Expected text content"),
        }
    }

    fn is_error(result: &CallToolResult) -> bool {
        result.is_error == Some(true)
    }

    fn query_results(store: &Store, sparql: &str) -> String {
        let result = sparql_query(store, sparql, None);
        assert!(!is_error(&result), "Query failed: {}", result_text(&result));
        result_text(&result).to_string()
    }

    fn qq(store: &Store, select: &str, body: &str) -> String {
        let sparql = format!(
            "PREFIX ans: <https://ds-labs.org/ansible#>\nPREFIX host: <http://www.invincea.com/ontologies/icas/1.0/host#>\n{select} WHERE {{ {body} }}"
        );
        query_results(store, &sparql)
    }

    #[test]
    fn test_load_inventory_tool_ini() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("hosts"),
            "[webservers]\nweb01 ansible_host=10.0.0.1\nweb02 ansible_host=10.0.0.2\n\n[dbservers]\ndb01\n",
        )
        .unwrap();

        let store = Store::new().unwrap();
        let result = load_inventory(&store, dir.path().join("hosts").to_str().unwrap());
        assert!(!is_error(&result), "Failed: {}", result_text(&result));
        assert!(
            result_text(&result).contains("2 hosts") || result_text(&result).contains("3 hosts"),
            "Unexpected: {}",
            result_text(&result)
        );

        // Verify ICAS host:Host type
        let json = qq(
            &store,
            "SELECT ?name",
            "?h a host:Host ; host:hostName ?name .",
        );
        assert!(json.contains("web01"), "web01 not found: {json}");
        assert!(json.contains("db01"), "db01 not found: {json}");

        // Verify ansible_host
        let json = qq(
            &store,
            "SELECT ?addr",
            "?h a host:Host ; host:hostName \"web01\" ; ans:ansibleHost ?addr .",
        );
        assert!(json.contains("10.0.0.1"), "ansible_host not found: {json}");

        // Verify group membership
        let json = qq(
            &store,
            "SELECT ?group",
            "?h a host:Host ; host:hostName \"web01\" ; ans:memberOf ?g . ?g ans:name ?group .",
        );
        assert!(json.contains("webservers"), "group not found: {json}");
    }

    #[test]
    fn test_load_inventory_tool_yaml() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("inventory.yml"),
            r#"---
all:
  children:
    webservers:
      hosts:
        web01:
          ansible_host: 10.0.0.1
        web02:
          ansible_host: 10.0.0.2
      vars:
        http_port: "80"
"#,
        )
        .unwrap();

        let store = Store::new().unwrap();
        let result = load_inventory(&store, dir.path().join("inventory.yml").to_str().unwrap());
        assert!(!is_error(&result), "Failed: {}", result_text(&result));

        // Verify group variable
        let json = qq(
            &store,
            "SELECT ?val",
            "?g a ans:HostGroup ; ans:name \"webservers\" ; ans:hasVariable ?v . ?v ans:variableName \"http_port\" ; ans:variableValue ?val .",
        );
        assert!(json.contains("80"), "group var not found: {json}");
    }

    #[test]
    fn test_load_ansible_tool_full_project() {
        let dir = TempDir::new().unwrap();

        // Inventory
        let inv = dir.path().join("inventory");
        fs::create_dir_all(&inv).unwrap();
        fs::write(inv.join("hosts"), "[web]\nweb01\n").unwrap();

        // Playbook
        fs::write(
            dir.path().join("site.yml"),
            r#"---
- name: Deploy web
  hosts: web
  tasks:
    - name: Install nginx
      apt:
        name: nginx
  roles:
    - nginx
"#,
        )
        .unwrap();

        // Role
        let role_dir = dir.path().join("roles/nginx");
        fs::create_dir_all(role_dir.join("tasks")).unwrap();
        fs::write(
            role_dir.join("tasks/main.yml"),
            "---\n- name: Copy config\n  template:\n    src: nginx.conf.j2\n    dest: /etc/nginx/nginx.conf\n",
        )
        .unwrap();

        let store = Store::new().unwrap();
        let result = load_ansible(&store, dir.path().to_str().unwrap(), None);
        assert!(!is_error(&result), "Failed: {}", result_text(&result));
        assert!(
            result_text(&result).contains("playbook"),
            "No playbooks: {}",
            result_text(&result)
        );

        // Verify playbook
        let json = qq(
            &store,
            "SELECT ?name",
            "?pb a ans:Playbook ; ans:name ?name .",
        );
        assert!(json.contains("site.yml"), "playbook not found: {json}");

        // Verify play target hosts
        let json = qq(
            &store,
            "SELECT ?hosts",
            "?p a ans:Play ; ans:targetHosts ?hosts .",
        );
        assert!(json.contains("web"), "target hosts not found: {json}");

        // Verify task module
        let json = qq(&store, "SELECT ?mod", "?t a ans:Task ; ans:module ?mod .");
        assert!(json.contains("apt"), "apt module not found: {json}");
        assert!(
            json.contains("template"),
            "template module not found: {json}"
        );

        // Verify role
        let json = qq(&store, "SELECT ?name", "?r a ans:Role ; ans:name ?name .");
        assert!(json.contains("nginx"), "nginx role not found: {json}");

        // Verify uses_role link
        let json = qq(
            &store,
            "SELECT ?role",
            "?p a ans:Play ; ans:usesRole ?r . ?r ans:name ?role .",
        );
        assert!(json.contains("nginx"), "usesRole not found: {json}");
    }

    #[test]
    fn test_load_inventory_nonexistent() {
        let store = Store::new().unwrap();
        let result = load_inventory(&store, "/nonexistent/path");
        assert!(is_error(&result));
        assert!(result_text(&result).contains("does not exist"));
    }

    #[test]
    fn test_load_ansible_nonexistent() {
        let store = Store::new().unwrap();
        let result = load_ansible(&store, "/nonexistent/path", None);
        assert!(is_error(&result));
        assert!(result_text(&result).contains("does not exist"));
    }
}
