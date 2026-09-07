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
            "resources/templates/list" => json!({"resourceTemplates": []}),
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
                match crate::resources::read_payload(uri, &grants) {
                    crate::resources::ReadOutcome::Ok(body) => json!({
                        "contents": [{"uri": uri, "mimeType": "application/json", "text": body.to_string()}]
                    }),
                    outcome => {
                        let payload = crate::resources::read_result_payload(outcome);
                        let detail = &payload["error"];
                        let code = if detail["code"] == "resource_not_found" { -32002 } else { -32001 };
                        return Some(json!({"jsonrpc":"2.0","id":id,"error":{
                            "code":code,"message":detail["message"],"data":detail
                        }}));
                    }
                }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resource_reads_use_mcp_contents_and_jsonrpc_errors() {
        let server = McpServer;
        let request = json!({"jsonrpc":"2.0","id":1,"method":"resources/read",
            "params":{"uri":"membrane://resource/operation-registry/v1"}});
        let denied = server.dispatch(&request).unwrap();
        assert!(denied.get("result").is_none());
        assert_eq!(denied["error"]["code"], -32001);
        assert_eq!(denied["error"]["data"]["code"], "resource_access_denied");
        let mut authorized = request.clone();
        authorized["params"]["accessGrants"] = json!(["protocol.read"]);
        let response = server.dispatch(&authorized).unwrap();
        assert!(response.get("error").is_none());
        assert_eq!(response["result"]["contents"][0]["uri"], request["params"]["uri"]);
        let text = response["result"]["contents"][0]["text"].as_str().unwrap();
        assert!(serde_json::from_str::<Value>(text).unwrap().get("body").is_some());
        authorized["params"]["uri"] = json!("membrane://resource/unknown/v1");
        assert_eq!(server.dispatch(&authorized).unwrap()["error"]["code"], -32002);
        assert_eq!(server.dispatch(&json!({"id":2,"method":"resources/templates/list"})).unwrap()["result"],
            json!({"resourceTemplates":[]}));
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
