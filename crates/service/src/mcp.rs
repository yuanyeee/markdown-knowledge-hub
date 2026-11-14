//! Local stdio MCP server with two tools: search, then read.
//! Tool definitions stay short. The initialize instructions tell the model how to spend tokens.

use anyhow::Result;
use hub_core::Hub;
use serde_json::{Value, json};
use std::{
    io::{BufRead, Write},
    path::{Path, PathBuf},
};

const READ_CAP: usize = 8192;
const PROTOCOL: &str = "2024-11-05";

const INSTRUCTIONS: &str = "\
ローカルのMarkdown索引を検索する。原本は変更できず、このPCの外へは出ない。\
プロジェクトが不明なら project を省略し、返ったIDを使う。\
質問は search の抜粋で判断し、足りない1件だけ read する。\
長い文書は heading を指定するか、応答の next_offset を offset に渡して続きを読む。\
全文の繰り返し読み、リンク図、管理操作はしない。";

pub fn desktop_data_dir() -> PathBuf {
    #[cfg(windows)]
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        return PathBuf::from(local).join("com.yuanyeee.markdownknowledgehub");
    }
    #[cfg(target_os = "macos")]
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home)
            .join("Library/Application Support/com.yuanyeee.markdownknowledgehub");
    }
    if let Some(xdg) = std::env::var_os("XDG_DATA_HOME").filter(|value| !value.is_empty()) {
        return PathBuf::from(xdg).join("com.yuanyeee.markdownknowledgehub");
    }
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home)
            .join(".local/share")
            .join("com.yuanyeee.markdownknowledgehub");
    }
    PathBuf::from(".hub")
}

pub fn mcp_config_text(command: &Path, data_flag: &str, data: &Path) -> Result<String> {
    let entry = json!({
        "command": command,
        "args": ["mcp", data_flag, data],
    });
    let config = json!({ "mcpServers": { "markdown-knowledge-hub": entry } });
    let text = serde_json::to_string_pretty(&config)?;
    Ok(format!(
        "\
Claude Code の .mcp.json、Claude Desktop の claude_desktop_config.json、Gemini CLI の ~/.gemini/settings.json に追加します。
Antigravity は ~/.gemini/config/mcp_config.json（全体）または .agents/mcp_config.json（この作業フォルダー）です。

{text}

検索は抜粋だけを返し、本文は read の1件だけをモデルへ渡します。索引は {} を使い、デスクトップアプリと同じ登録を見ます。",
        data.display()
    ))
}

pub fn print_mcp_config(command: &Path, data_flag: &str, data: &Path) -> Result<()> {
    println!("{}", mcp_config_text(command, data_flag, data)?);
    Ok(())
}

pub fn serve_mcp(data: &Path) -> Result<()> {
    let hub = Hub::open(data)?;
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        if let Some(response) = respond(&hub, &line) {
            serde_json::to_writer(&mut stdout, &response)?;
            stdout.write_all(b"\n")?;
            stdout.flush()?;
        }
    }
    Ok(())
}

fn respond(hub: &Hub, line: &str) -> Option<Value> {
    let request: Value = match serde_json::from_str::<Value>(line) {
        Ok(value) if value.is_object() => value,
        _ => {
            return Some(rpc_error(
                Value::Null,
                -32700,
                "JSON-RPCのメッセージを読み取れません",
            ));
        }
    };
    let method = request.get("method").and_then(Value::as_str).unwrap_or("");
    let id = request.get("id").cloned()?;
    let params = request.get("params").cloned().unwrap_or_else(|| json!({}));
    Some(match method {
        "initialize" => {
            let version = params
                .get("protocolVersion")
                .and_then(Value::as_str)
                .filter(|version| version.starts_with("202"))
                .unwrap_or(PROTOCOL);
            json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "protocolVersion": version,
                    "capabilities": { "tools": {} },
                    "serverInfo": { "name": "markdown-knowledge-hub", "version": env!("CARGO_PKG_VERSION") },
                    "instructions": INSTRUCTIONS
                }
            })
        }
        "ping" => json!({"jsonrpc": "2.0", "id": id, "result": {}}),
        "tools/list" => json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": { "tools": tools() }
        }),
        "resources/list" => json!({"jsonrpc": "2.0", "id": id, "result": {"resources": []}}),
        "resource/templates/list" | "resources/templates/list" => {
            json!({"jsonrpc": "2.0", "id": id, "result": {"resourceTemplates": []}})
        }
        "prompts/list" => json!({"jsonrpc": "2.0", "id": id, "result": {"prompts": []}}),
        "tools/call" => {
            let name = params.get("name").and_then(Value::as_str).unwrap_or("");
            let arguments = params
                .get("arguments")
                .cloned()
                .unwrap_or_else(|| json!({}));
            match call_tool(hub, name, &arguments) {
                Ok(text) => tool_result(id, &text, false),
                Err(message) => tool_result(id, &message, true),
            }
        }
        _ => rpc_error(id, -32601, "未対応のMCPメソッドです"),
    })
}

fn tools() -> Value {
    json!([
        {
            "name": "search",
            "description": "索引を検索し、path・title・短い抜粋だけ返す。本文が必要なら、その1件を read する。",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "project": { "type": "string", "description": "プロジェクトID。省略時は登録が1件ならそれを使う。" },
                    "query": { "type": "string", "description": "空白区切りはAND。" },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 5, "description": "既定5。" }
                },
                "required": ["query"]
            }
        },
        {
            "name": "read",
            "description": "1文書を読む。既定は先頭約8KB。節は heading、続きは前回の next_offset を offset に渡す。",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "project": { "type": "string", "description": "プロジェクトID。省略時は登録が1件ならそれを使う。" },
                    "path": { "type": "string", "description": "search が返した path。" },
                    "heading": { "type": "string", "description": "読む見出し。省略すると文書の先頭。" },
                    "offset": { "type": "integer", "minimum": 0, "description": "続きを読むバイト位置。前回の next_offset。" }
                },
                "required": ["path"]
            }
        }
    ])
}

fn call_tool(hub: &Hub, name: &str, arguments: &Value) -> std::result::Result<String, String> {
    if !arguments.is_object() {
        return Err("引数はオブジェクトで指定してください".to_string());
    }
    match name {
        "search" => search(hub, arguments),
        "read" => read(hub, arguments),
        _ => Err(
            "使えるのは search と read です。まず search し、必要な1件を read してください"
                .to_string(),
        ),
    }
}

fn search(hub: &Hub, arguments: &Value) -> std::result::Result<String, String> {
    let project = resolve_project(hub, optional_string(arguments, "project"))?;
    let query = optional_string(arguments, "query")
        .ok_or_else(|| format!("query を指定してください。プロジェクトは {project} です"))?;
    if query.chars().count() > 1024 {
        return Err("検索語が長すぎます".to_string());
    }
    let limit = match optional_usize(arguments, "limit")? {
        None => 5,
        Some(limit) if (1..=5).contains(&limit) => limit,
        Some(_) => {
            return Err(
                "limit は1～5です。件数を増やす代わりに語を足して絞ってください".to_string(),
            );
        }
    };
    let page = hub
        .search_page(&project, &query, 0, limit)
        .map_err(|error| error.to_string())?;
    let mut text = format!(
        "project: {project}\nquery: {query}\n{}件\n",
        page.items.len()
    );
    for (index, hit) in page.items.iter().enumerate() {
        text.push_str(&format!(
            "{}. {}\npath: {}\n",
            index + 1,
            hit.title,
            hit.path
        ));
        if let Some(snippet) = hit.snippet.as_deref() {
            text.push_str(&short(snippet, 160));
            text.push('\n');
        }
    }
    if page.next_offset.is_some() {
        text.push_str("他にも一致があります。語を足して絞り、必要な1件を read する。\n");
    } else if page.items.is_empty() {
        text.push_str("語を短くするか、別の語で search する。\n");
    } else {
        text.push_str("抜粋で足りなければ、その path を1件だけ read する。\n");
    }
    Ok(text)
}

fn read(hub: &Hub, arguments: &Value) -> std::result::Result<String, String> {
    let project = resolve_project(hub, optional_string(arguments, "project"))?;
    let path = optional_string(arguments, "path")
        .ok_or_else(|| "path は search の結果を指定してください".to_string())?;
    let document = hub.document(&project, &path).map_err(|_| {
        format!("文書を読めません: {path}。path は search の結果を指定してください")
    })?;
    let (body, scope) = if let Some(heading) = optional_string(arguments, "heading") {
        (
            section(&document.markdown, &heading)?,
            format!("heading: {heading}\n"),
        )
    } else {
        (document.markdown, String::new())
    };
    let offset = optional_usize(arguments, "offset")?.unwrap_or(0);
    let (slice, truncated, next_offset) = take_capped(&body, offset)?;
    let mut text = format!(
        "project: {project}\npath: {path}\ntitle: {}\n{scope}offset: {offset}\n",
        document.title
    );
    if truncated {
        text.push_str(&format!("next_offset: {next_offset}\n"));
    }
    text.push('\n');
    text.push_str(&slice);
    if !slice.ends_with('\n') {
        text.push('\n');
    }
    if truncated {
        text.push_str(
            "続きあり。next_offset を offset に渡すか、heading で節を指定する。全文をまとめて読まない。\n",
        );
    }
    Ok(text)
}

fn resolve_project(hub: &Hub, requested: Option<String>) -> std::result::Result<String, String> {
    let projects = hub.projects().map_err(|error| error.to_string())?;
    if let Some(id) = requested {
        if projects.iter().any(|project| project.id == id) {
            Ok(id)
        } else {
            Err(format!(
                "プロジェクト {id} はありません。\n{}",
                project_list(&projects)
            ))
        }
    } else if let [project] = projects.as_slice() {
        Ok(project.id.clone())
    } else if projects.is_empty() {
        Err(
            "登録されたプロジェクトがありません。Markdown Knowledge Hub でプロジェクトを作成してから検索してください。"
                .to_string(),
        )
    } else {
        Err(format!(
            "プロジェクトが複数あります。search と read の project にIDを指定してください。\n{}",
            project_list(&projects)
        ))
    }
}

fn project_list(projects: &[hub_core::Project]) -> String {
    projects
        .iter()
        .map(|project| format!("- {}: {}", project.id, project.name))
        .collect::<Vec<_>>()
        .join("\n")
}

fn optional_string(arguments: &Value, key: &str) -> Option<String> {
    arguments
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn optional_usize(arguments: &Value, key: &str) -> std::result::Result<Option<usize>, String> {
    match arguments.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(number)) => number
            .as_u64()
            .and_then(|value| usize::try_from(value).ok())
            .map(Some)
            .ok_or_else(|| format!("{key}は0以上の整数です")),
        _ => Err(format!("{key}は整数です")),
    }
}

struct HeadingSpan {
    level: usize,
    line_start: usize,
    content_start: usize,
    text: String,
}

fn headings(markdown: &str) -> Vec<HeadingSpan> {
    let mut found = Vec::new();
    let mut offset = 0;
    for line in markdown.split_inclusive('\n') {
        let trimmed = line.trim_end_matches(['\n', '\r']);
        let hashes = trimmed.bytes().take_while(|byte| *byte == b'#').count();
        if (1..=6).contains(&hashes)
            && trimmed
                .as_bytes()
                .get(hashes)
                .is_some_and(|byte| byte.is_ascii_whitespace())
        {
            let text = trimmed[hashes..].trim();
            if !text.is_empty() {
                found.push(HeadingSpan {
                    level: hashes,
                    line_start: offset,
                    content_start: offset + line.len(),
                    text: text.to_string(),
                });
            }
        }
        offset += line.len();
    }
    found
}

fn section(markdown: &str, heading: &str) -> std::result::Result<String, String> {
    let found = headings(markdown);
    let wanted = heading.trim();
    let index = found
        .iter()
        .position(|item| item.text == wanted)
        .ok_or_else(|| {
            let names = found
                .iter()
                .take(15)
                .map(|item| format!("- {}", item.text))
                .collect::<Vec<_>>()
                .join("\n");
            if names.is_empty() {
                format!("見出し {wanted} はありません。heading を省略すると先頭を読みます")
            } else {
                format!("見出し {wanted} はありません。次の heading を指定してください。\n{names}")
            }
        })?;
    let current = &found[index];
    let end = found
        .iter()
        .skip(index + 1)
        .find(|item| item.level <= current.level)
        .map(|item| item.line_start)
        .unwrap_or(markdown.len());
    Ok(markdown[current.content_start..end].to_string())
}

fn take_capped(text: &str, offset: usize) -> std::result::Result<(String, bool, usize), String> {
    if offset > text.len() || !text.is_char_boundary(offset) {
        return Err("offset は前回の next_offset を指定してください".to_string());
    }
    let rest = &text[offset..];
    if rest.len() <= READ_CAP {
        return Ok((rest.to_string(), false, offset + rest.len()));
    }
    let mut end = READ_CAP;
    while !rest.is_char_boundary(end) {
        end -= 1;
    }
    if let Some(newline) = rest[..end].rfind('\n')
        && newline > READ_CAP / 2
    {
        end = newline + 1;
    }
    Ok((rest[..end].to_string(), true, offset + end))
}

fn short(text: &str, max_chars: usize) -> String {
    let mut clipped: String = text.chars().take(max_chars).collect();
    if text.chars().nth(max_chars).is_some() {
        clipped.push('…');
    }
    clipped
}

fn tool_result(id: Value, text: &str, is_error: bool) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": {
            "content": [{ "type": "text", "text": text }],
            "isError": is_error
        }
    })
}

fn rpc_error(id: Value, code: i64, message: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hub_with(files: &[(&str, &str)]) -> (tempfile::TempDir, Hub) {
        let root = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        for (path, body) in files {
            let file = root.path().join(path);
            if let Some(parent) = file.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(file, body).unwrap();
        }
        let mut hub = Hub::open(state.path()).unwrap();
        hub.register("demo", "Demo", root.path()).unwrap();
        hub.scan("demo").unwrap();
        drop(root);
        (state, hub)
    }

    #[test]
    fn printed_config_names_each_client() {
        let text = mcp_config_text(Path::new("hub.exe"), "--data", Path::new("D:\\Hub")).unwrap();
        assert!(text.contains("Claude Code"));
        assert!(text.contains("Claude Desktop"));
        assert!(text.contains("Gemini CLI"));
        assert!(text.contains("Antigravity"));
        assert!(text.contains("\"mcp\""));
        assert!(text.contains("--data"));
    }

    #[test]
    fn initialize_instructions_tell_the_model_to_search_before_reading() {
        let (_state, hub) = hub_with(&[("a.md", "# A\n本文")]);
        let response = respond(
            &hub,
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18"}}"#,
        )
        .unwrap();
        assert_eq!(response["result"]["protocolVersion"], "2025-06-18");
        let instructions = response["result"]["instructions"].as_str().unwrap();
        assert!(instructions.contains("search"));
        assert!(instructions.contains("read"));
        assert!(instructions.contains("next_offset"));
        let listed = respond(&hub, r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#).unwrap();
        let tools = listed["result"]["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 2);
        assert_eq!(tools[0]["name"], "search");
        assert_eq!(tools[1]["name"], "read");
        assert!(
            respond(
                &hub,
                r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#
            )
            .is_none()
        );
    }

    #[test]
    fn search_returns_a_short_snippet_and_read_stops_at_the_cap() {
        let (_state, hub) = hub_with(&[
            (
                "choice.md",
                "# 価格\n\n価格の決め方です。\n\n## 詳細\n\n詳細本文\n",
            ),
            ("other.md", &format!("# 長い\n\n{}", "あ".repeat(9000))),
        ]);
        let found = respond(
            &hub,
            r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"search","arguments":{"query":"価格"}}}"#,
        )
        .unwrap();
        let text = found["result"]["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("path: choice.md"));
        assert!(text.contains("project: demo"));
        assert_eq!(found["result"]["isError"], false);
        let section = respond(
            &hub,
            r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"read","arguments":{"path":"choice.md","heading":"詳細"}}}"#,
        )
        .unwrap();
        let body = section["result"]["content"][0]["text"].as_str().unwrap();
        assert!(body.contains("詳細本文"));
        assert!(!body.contains("価格の決め方"));
        let long = respond(
            &hub,
            r#"{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"read","arguments":{"path":"other.md"}}}"#,
        )
        .unwrap();
        let long_text = long["result"]["content"][0]["text"].as_str().unwrap();
        assert!(long_text.contains("next_offset:"));
        assert!(long_text.contains("全文をまとめて読まない"));
        let next: usize = long_text
            .lines()
            .find_map(|line| line.strip_prefix("next_offset: "))
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        assert!(next <= READ_CAP);
        let continued = respond(
            &hub,
            &format!(
                r#"{{"jsonrpc":"2.0","id":8,"method":"tools/call","params":{{"name":"read","arguments":{{"path":"other.md","offset":{next}}}}}}}"#
            ),
        )
        .unwrap();
        let continued_text = continued["result"]["content"][0]["text"].as_str().unwrap();
        assert!(continued_text.contains(&format!("offset: {next}")));
        assert!(!continued_text.contains("# 長い"));
        let broken = respond(
            &hub,
            r#"{"jsonrpc":"2.0","id":9,"method":"tools/call","params":{"name":"read","arguments":{"path":"other.md","offset":11}}}"#,
        )
        .unwrap();
        assert_eq!(broken["result"]["isError"], true);
        assert!(
            broken["result"]["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("next_offset")
        );
        let empty = respond(
            &hub,
            r#"{"jsonrpc":"2.0","id":10,"method":"tools/call","params":{"name":"search","arguments":{"query":" "}}}"#,
        )
        .unwrap();
        assert_eq!(empty["result"]["isError"], true);
        assert!(
            !empty["result"]["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("path:")
        );
        let unknown = respond(
            &hub,
            r#"{"jsonrpc":"2.0","id":6,"method":"tools/call","params":{"name":"graph","arguments":{}}}"#,
        )
        .unwrap();
        assert_eq!(unknown["result"]["isError"], true);
    }

    #[test]
    fn several_projects_ask_for_an_id_instead_of_reading_everything() {
        let root = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("a.md"), "# Alpha\n価格").unwrap();
        std::fs::write(other.path().join("b.md"), "# Beta\n価格").unwrap();
        let mut hub = Hub::open(state.path()).unwrap();
        hub.register("alpha", "Alpha", root.path()).unwrap();
        hub.register("beta", "Beta", other.path()).unwrap();
        hub.scan("alpha").unwrap();
        hub.scan("beta").unwrap();
        let response = respond(
            &hub,
            r#"{"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"search","arguments":{"query":"価格"}}}"#,
        )
        .unwrap();
        assert_eq!(response["result"]["isError"], true);
        let text = response["result"]["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("alpha"));
        assert!(text.contains("beta"));
        assert!(!text.contains("path:"));
    }
}
