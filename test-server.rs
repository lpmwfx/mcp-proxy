// Minimal MCP server for testing — responds to initialize, tools/list, tools/call
use std::io::{self, BufRead, Write};

fn main() {
    let stdin = io::stdin();
    let mut stdout = io::stdout();
    let reader = stdin.lock();

    for line in reader.lines() {
        if let Ok(line) = line {
            if line.trim().is_empty() {
                continue;
            }

            // Parse method and id from JSON string (simple string matching)
            let method = if line.contains("\"method\":\"initialize\"") {
                "initialize"
            } else if line.contains("\"method\":\"tools/list\"") {
                "tools/list"
            } else if line.contains("\"method\":\"tools/call\"") {
                "tools/call"
            } else {
                "unknown"
            };

            // Extract id
            let id = extract_id(&line);

            let response = match method {
                "initialize" => {
                    format!(
                        r#"{{"jsonrpc":"2.0","id":{},"result":{{"protocolVersion":"2024-11-05","serverInfo":{{"name":"test-server","version":"0.1.0"}},"capabilities":{{"tools":{{}}}}}}}}"#,
                        id
                    )
                }
                "tools/list" => {
                    let tools_json = r#"[{"name":"echo","description":"Echo a message","inputSchema":{"type":"object","properties":{"message":{"type":"string","description":"Message to echo"}},"required":["message"]}}]"#;
                    format!(
                        r#"{{"jsonrpc":"2.0","id":{},"result":{{"tools":{}}}}}"#,
                        id, tools_json
                    )
                }
                "tools/call" => {
                    let message = extract_message(&line).unwrap_or_else(|| "(no message)".to_string());
                    format!(
                        r#"{{"jsonrpc":"2.0","id":{},"result":{{"text":"Echo: {}"}}}}"#,
                        id, message
                    )
                }
                _ => {
                    format!(
                        r#"{{"jsonrpc":"2.0","id":{},"error":{{"code":-32601,"message":"Method not found"}}}}"#,
                        id
                    )
                }
            };

            let _ = writeln!(stdout, "{}", response);
            let _ = stdout.flush();
        }
    }
}

fn extract_id(json: &str) -> String {
    if let Some(start) = json.find("\"id\":") {
        let after_id = &json[start + 5..];
        if let Some(end) = after_id.find(|c: char| !c.is_numeric() && c != '-') {
            after_id[..end].to_string()
        } else {
            "null".to_string()
        }
    } else {
        "null".to_string()
    }
}

fn extract_message(json: &str) -> Option<String> {
    if let Some(start) = json.find("\"message\":\"") {
        let after = &json[start + 11..];
        if let Some(end) = after.find('"') {
            Some(after[..end].to_string())
        } else {
            None
        }
    } else {
        None
    }
}
