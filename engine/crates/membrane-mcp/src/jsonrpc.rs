use serde_json::{json, Value};
use std::io::{self, BufRead, Write};

#[derive(Clone, Default)]
pub struct McpServer;

impl McpServer {
    pub fn dispatch(&self, request: &Value) -> Option<Value> {
        let method = request.get("method")?.as_str()?;
        let id = request.get("id").cloned().unwrap_or(Value::Null);
        let result = match method {
            "initialize" => crate::initialize_response(),
            "notifications/initialized" => return None,
            "tools/list" => {
                json!({"tools": crate::tools::negotiated_definitions(request.get("params"))})
            }
            "resources/list" => crate::resources::list_payload(),
            "resources/read" => {
                let uri = request
                    .pointer("/params/uri")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let grants = request
                    .pointer("/params/accessGrants")
                    .and_then(Value::as_array)
                    .map(|entries| {
                        entries
                            .iter()
                            .filter_map(|entry| entry.as_str())
                            .collect::<Vec<&str>>()
                    })
                    .unwrap_or_default();
                crate::resources::read_result_payload(crate::resources::read_payload(uri, &grants))
            }
            "prompts/list" => crate::prompts::list_payload(),
            "prompts/get" => {
                let name = request
                    .pointer("/params/name")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                match crate::prompts::get_payload(name) {
                    Some(payload) => payload,
                    None => json!({"error": "unknown_prompt", "name": name}),
                }
            }
            "ping" => json!({}),
            "tools/call" => {
                let name = request
                    .pointer("/params/name")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let arguments = request.pointer("/params/arguments").unwrap_or(&Value::Null);
                crate::tools::call(name, arguments)
            }
            _ => {
                return Some(
                    json!({"jsonrpc":"2.0","id":id,"error":{"code":-32601,"message":"Method not found"}}),
                )
            }
        };
        Some(json!({"jsonrpc":"2.0","id":id,"result":result}))
    }
}

pub fn serve_stdio() -> io::Result<()> {
    let server = McpServer;
    let stdin = io::stdin();
    let mut stdout = io::BufWriter::new(io::stdout().lock());
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let request: Value = serde_json::from_str(&line)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        if let Some(response) = server.dispatch(&request) {
            writeln!(stdout, "{}", response)?;
            stdout.flush()?;
        }
    }
    Ok(())
}
