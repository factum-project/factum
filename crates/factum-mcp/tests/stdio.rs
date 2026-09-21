//! Exercise the shipped binary over real stdin/stdout, as an MCP client does.
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

fn exchange(messages: Vec<Value>) -> Vec<Value> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_factum-mcp-server"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let stdout = child.stdout.take().unwrap();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            if tx.send(line.unwrap()).is_err() {
                break;
            }
        }
    });
    let mut stdin = child.stdin.take().unwrap();
    for message in messages {
        writeln!(stdin, "{message}").unwrap();
    }
    drop(stdin);
    let mut responses = Vec::new();
    loop {
        match rx.recv_timeout(Duration::from_secs(5)) {
            Ok(line) => responses
                .push(serde_json::from_str(&line).expect("stdout must contain JSON-RPC only")),
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(_) => {
                child.kill().ok();
                child.wait().ok();
                panic!("stdio server did not close on EOF");
            }
        }
    }
    assert!(child.wait().unwrap().success());
    responses
}
fn req(id: u64, method: &str, params: Value) -> Value {
    json!({"jsonrpc":"2.0","id":id,"method":method,"params":params})
}
fn init() -> Value {
    req(
        0,
        "initialize",
        json!({"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"stdio-test","version":"1"}}),
    )
}
fn call(id: u64, name: &str, arguments: Value) -> Value {
    req(id, "tools/call", json!({"name":name,"arguments":arguments}))
}
fn insert(id: u64, node_id: &str, permission: &str) -> Value {
    call(
        id,
        "factum_insert",
        json!({"node":format!("(node {node_id} :pred (located-in @FACTORY @CITY) :valid forever :src (asserted \"test\") :conf 0.95 :auth 0.8 :perm {permission} :deps [])")}),
    )
}
fn data(response: &Value) -> Value {
    let result = &response["result"];
    assert_eq!(
        result["content"][0]["type"], "text",
        "standard MCP clients reject custom content types"
    );
    let text: Value = serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(text, result["structuredContent"]);
    text
}

#[test]
fn notifications_are_silent_and_ping_works() {
    let r = exchange(vec![
        init(),
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
        req(1, "ping", json!({})),
        json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":99,"reason":"test"}}),
        req(2, "tools/list", json!({})),
    ]);
    assert_eq!(r.len(), 3, "notifications must never receive responses");
    assert_eq!(r[1], json!({"jsonrpc":"2.0","id":1,"result":{}}));
    assert_eq!(r[2]["result"]["tools"].as_array().unwrap().len(), 9);
}

#[test]
fn plain_client_can_insert_query_retract_and_query_again() {
    let r = exchange(vec![
        init(),
        insert(1, "public001", "public"),
        call(
            2,
            "factum_query",
            json!({"query":"(located-in @FACTORY ?city)"}),
        ),
        call(3, "factum_retract", json!({"node_id":"public001"})),
        call(
            4,
            "factum_query",
            json!({"query":"(located-in @FACTORY ?city)"}),
        ),
    ]);
    assert_eq!(r[1]["result"]["isError"], false);
    let q = data(&r[2]);
    assert_eq!(q["count"], 1);
    assert_eq!(
        q["form"], "canonical",
        "a plain client has no Factum numeric morpheme table"
    );
    assert!(q["nodes"][0]
        .as_str()
        .unwrap()
        .contains("(located-in @FACTORY @CITY)"));
    assert_eq!(data(&r[3])["retracted"], json!(["public001"]));
    assert_eq!(data(&r[4])["count"], 0);
}

#[test]
fn resources_are_discoverable_and_have_resource_contents() {
    let r = exchange(vec![
        init(),
        insert(1, "public001", "public"),
        req(2, "resources/list", json!({})),
        req(3, "resources/templates/list", json!({})),
        req(
            4,
            "resources/read",
            json!({"uri":"factum://nodes/public001"}),
        ),
    ]);
    assert_eq!(
        r[2]["result"]["resources"][0]["uri"],
        "factum://nodes/public001"
    );
    assert_eq!(
        r[3]["result"]["resourceTemplates"][0]["uriTemplate"],
        "factum://nodes/{id}"
    );
    let resource = &r[4]["result"]["contents"][0];
    assert_eq!(resource["uri"], "factum://nodes/public001");
    assert_eq!(resource["mimeType"], "text/plain");
    assert!(resource["text"].as_str().unwrap().contains("@CITY"));
}

#[test]
fn resources_do_not_bypass_public_visibility_or_retraction() {
    let r = exchange(vec![
        init(),
        insert(1, "public001", "public"),
        insert(2, "private001", "confidential"),
        req(3, "resources/list", json!({})),
        req(
            4,
            "resources/read",
            json!({"uri":"factum://nodes/private001"}),
        ),
        call(5, "factum_retract", json!({"node_id":"public001"})),
        req(6, "resources/list", json!({})),
        req(
            7,
            "resources/read",
            json!({"uri":"factum://nodes/public001"}),
        ),
    ]);
    assert_eq!(r[3]["result"]["resources"].as_array().unwrap().len(), 1);
    assert!(r[4].get("error").is_some());
    assert_eq!(r[6]["result"]["resources"], json!([]));
    assert!(r[7].get("error").is_some());
}

#[test]
fn initialization_only_advertises_implemented_notifications() {
    let r = exchange(vec![init()]);
    let caps = &r[0]["result"]["capabilities"];
    // resources.subscribe is NOT implemented (no push subscriptions over stdio)
    assert_ne!(caps["resources"]["subscribe"], true);
    // resources.listChanged is NOT implemented (resources are static)
    assert_ne!(caps["resources"]["listChanged"], true);
    // tools.listChanged IS declared as true so clients re-query tools on reconnect
    // (tool set is static within a session, but changes across server restarts/version updates)
    assert_eq!(caps["tools"]["listChanged"], true);
}
