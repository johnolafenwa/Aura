use std::io::Write;
use std::process::{Command, Stdio};

const SOURCE: &str = "type Doubler = Callable[def(value: int64) -> int64]\ndef double(value: int64) -> int64:\n    return value * 2\ndef main():\n    callback = Doubler(double)\n    result = callback(21)\n    print(result)\n";

fn query(verb: &str, line: &str, character: &str, extra: &[&str]) -> serde_json::Value {
    let mut child = Command::new(env!("CARGO_BIN_EXE_aura"))
        .args([verb, "--line", line, "--character", character])
        .args(extra)
        .args(["--stdin", "/tmp/aura-editor-query.au"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start editor query");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(SOURCE.as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("compiler query JSON")
}

#[test]
fn editor_verbs_share_the_compiler_query_model_over_stdin() {
    let signature = query("signature-help", "5", "23", &[]);
    assert_eq!(signature["active_parameter"], 0);
    assert!(signature["label"]
        .as_str()
        .unwrap()
        .contains("value: int64"));
    assert_eq!(
        query("references", "4", "18", &["--include-declaration"])
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        query("references", "4", "18", &[])
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(query("prepare-rename", "5", "7", &[])["start_character"], 4);
    assert_eq!(
        query("rename", "5", "7", &["--new-name", "answer"])["edits"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert!(query("rename", "5", "7", &["--new-name", "callback"]).is_null());
    assert!(query("prepare-rename", "6", "6", &[]).is_null());
}
