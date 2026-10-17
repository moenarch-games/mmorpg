use serde_json::{Value, json};
use std::{
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
    time::Instant,
};

fn requests() -> Vec<Value> {
    let mut cases = Vec::new();
    for (name, eye) in [
        ("outpost", [0.0, 15.0, 35.0]),
        ("woods", [-95.0, 12.0, -20.0]),
        ("farm", [90.0, 18.0, 20.0]),
        ("lake", [0.0, 15.0, 125.0]),
        ("hollow", [0.0, 15.0, -150.0]),
        ("far", [600.0, 40.0, 600.0]),
    ] {
        cases.push(json!({"name":format!("region:{name}"),"kind":"world","seconds":0,"animate":true,"eye":eye}));
    }
    for seconds in [0, 300, 600, 900, 1200] {
        cases.push(json!({"name":format!("day:{seconds}"),"kind":"world","seconds":seconds,"animate":true,"eye":[0,15,35]}));
    }
    for seconds in [0, 1, 600] {
        cases.push(json!({"name":format!("frozen:{seconds}"),"kind":"world","seconds":seconds,"animate":false,"eye":[0,15,35]}));
    }
    cases
}

fn export(cases: &[Value]) -> Value {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let input = json!({"scenery":serde_json::from_str::<Value>(mmorpg_wasm::scenery::hosted_scenery_json()).unwrap(),
        "requests":cases});
    let started = Instant::now();
    let mut child = Command::new("bun")
        .arg("tests/support/native-world-reference.ts")
        .current_dir(root.join("web"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.to_string().as_bytes())
        .unwrap();
    let result = child.wait_with_output().unwrap();
    assert!(
        result.status.success(),
        "Pages reference exporter failed: {}",
        result.status
    );
    eprintln!(
        "Pages reference: {} bytes for {} cases in {:?}",
        result.stdout.len(),
        cases.len(),
        started.elapsed()
    );
    serde_json::from_slice(&result.stdout).unwrap()
}

pub fn shared() -> &'static (Vec<Value>, Value) {
    static REFERENCE: std::sync::OnceLock<(Vec<Value>, Value)> = std::sync::OnceLock::new();
    REFERENCE.get_or_init(|| {
        let cases = requests();
        let output = export(&cases);
        (cases, output)
    })
}
