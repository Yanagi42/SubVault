use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::Value;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_subvault")
}

const ACTOR: &str = "act_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

struct Tmp {
    path: PathBuf,
}

impl Tmp {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "subvault-snapshot-{label}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&path).unwrap();
        Self { path }
    }
}

impl Drop for Tmp {
    fn drop(&mut self) {
        if std::thread::panicking() {
            eprintln!("kept {}", self.path.display());
        } else {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}

fn run(args: &[&str]) -> (i32, Value) {
    let output = Command::new(bin())
        .args(args)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let body = serde_json::from_str(&stdout).unwrap_or_else(|err| {
        panic!(
            "stdout is not JSON ({err})\nstdout:\n{stdout}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (output.status.code().unwrap_or(1), body)
}

fn html(vault: &Path, command: &str, bytes: &str) -> (i32, Value) {
    let path = vault.with_file_name(format!("{command}.html"));
    fs::write(&path, bytes).unwrap();
    run(&[
        "html",
        "--vault",
        vault.to_str().unwrap(),
        "--command-id",
        command,
        "--source",
        path.to_str().unwrap(),
    ])
}

fn doc(cents: u32) -> String {
    format!(
        "<p>Amount paid USD {}.{:02}</p><p>Service period: 2024-02-01 through 2024-02-28</p>\n",
        cents / 100,
        cents % 100
    )
}

#[test]
fn concurrent_insert_review_is_one_snapshot() {
    let tmp = Tmp::new("insert");
    let vault = tmp.path.join("vault");
    let (code, init) = run(&["init", "--vault", vault.to_str().unwrap(), "--format", "4"]);
    assert_eq!(code, 0, "{init}");
    let (code, seed) = html(&vault, "seed", &doc(100));
    assert_eq!(code, 0, "{seed}");

    let stop = Instant::now() + Duration::from_secs(3);
    let vault_writer = vault.clone();
    let writer = thread::spawn(move || {
        let mut i = 0u32;
        let mut busy = 0u32;
        let mut ok = 0u32;
        let mut other = Vec::new();
        while Instant::now() < stop {
            i += 1;
            let (code, body) = html(&vault_writer, &format!("c{i}"), &doc(100 + i));
            match code {
                0 => ok += 1,
                11 => {
                    assert_eq!(body["reason"], "writer_busy", "{body}");
                    assert_ne!(body["outcome"], "ReadOnlyDamaged");
                    busy += 1;
                }
                _ => other.push((code, body["reason"].clone())),
            }
        }
        (ok, busy, other)
    });
    let mut reviews = 0u32;
    let mut ok_sheets = 0u32;
    let mut busy = 0u32;
    let mut torn = Vec::new();
    let mut damaged = Vec::new();
    while Instant::now() < stop {
        reviews += 1;
        let out = tmp.path.join(format!("r{reviews}.txt"));
        let (code, body) = run(&[
            "review",
            "--vault",
            vault.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ]);
        match code {
            0 => {
                ok_sheets += 1;
                let text = fs::read_to_string(&out).unwrap();
                if sheet_torn(&text) {
                    torn.push(out);
                }
            }
            11 => {
                assert_eq!(body["reason"], "writer_busy", "{body}");
                assert_ne!(body["outcome"], "ReadOnlyDamaged");
                busy += 1;
            }
            _ => damaged.push((code, body)),
        }
    }
    let (html_ok, html_busy, html_other) = writer.join().unwrap();
    assert!(ok_sheets > 0, "reviews={reviews} busy={busy}");
    assert!(torn.is_empty(), "torn sheets: {torn:?}");
    assert!(damaged.is_empty(), "damaged reviews: {damaged:?}");
    assert!(html_other.is_empty(), "html exits: {html_other:?}");
    assert!(html_ok > 0, "html ok={html_ok} busy={html_busy}");
    let (code, opened) = run(&["open", "--vault", vault.to_str().unwrap()]);
    assert_eq!(code, 0, "{opened}");
    assert_eq!(opened["outcome"], "Ready");
    let _ = html_busy;
}

#[test]
fn concurrent_correct_and_undo_keep_facts_with_decisions() {
    let tmp = Tmp::new("revise");
    let vault = tmp.path.join("vault");
    let (code, init) = run(&["init", "--vault", vault.to_str().unwrap(), "--format", "4"]);
    assert_eq!(code, 0, "{init}");
    let (code, first) = html(&vault, "a", &doc(950));
    assert_eq!(code, 0, "{first}");
    let (code, second) = html(&vault, "b", &doc(107));
    assert_eq!(code, 0, "{second}");
    let source_a = first["source_id"].as_str().unwrap().to_string();
    let source_b = second["source_id"].as_str().unwrap().to_string();
    let (code, listed) = run(&["facts", "--vault", vault.to_str().unwrap()]);
    assert_eq!(code, 0, "{listed}");
    let fact_a = fact_id(&listed, &source_a, "stated_amount_paid");
    let fact_b = fact_id(&listed, &source_b, "stated_amount_paid");

    let stop = Instant::now() + Duration::from_secs(3);
    let vault_writer = vault.clone();
    let fact_a_writer = fact_a.clone();
    let fact_b_writer = fact_b.clone();
    let writer = thread::spawn(move || {
        let mut step = 0u32;
        let mut revision_a = 1i64;
        let mut revision_b = 1i64;
        let mut decision_a = String::new();
        let mut decision_b = String::new();
        let mut ok = 0u32;
        let mut busy = 0u32;
        let mut other = Vec::new();
        while Instant::now() < stop {
            step += 1;
            let (fact, revision, undo_decision, amount) = if step % 2 == 1 {
                (
                    fact_a_writer.as_str(),
                    revision_a,
                    decision_a.clone(),
                    if revision_a % 2 == 1 { "12.00" } else { "9.50" },
                )
            } else {
                (
                    fact_b_writer.as_str(),
                    revision_b,
                    decision_b.clone(),
                    if revision_b % 2 == 1 { "3.00" } else { "1.07" },
                )
            };
            let correcting = revision % 2 == 1;
            let (code, body) = if correcting {
                run(&[
                    "correct",
                    "--vault",
                    vault_writer.to_str().unwrap(),
                    "--command-id",
                    &format!("fix-{step}"),
                    "--fact-id",
                    fact,
                    "--expected-revision",
                    &revision.to_string(),
                    "--amount-text",
                    amount,
                    "--actor-id",
                    ACTOR,
                ])
            } else {
                run(&[
                    "undo",
                    "--vault",
                    vault_writer.to_str().unwrap(),
                    "--command-id",
                    &format!("undo-{step}"),
                    "--decision-id",
                    &undo_decision,
                    "--expected-revision",
                    &revision.to_string(),
                    "--actor-id",
                    ACTOR,
                ])
            };
            match code {
                0 => {
                    ok += 1;
                    if step % 2 == 1 {
                        revision_a += 1;
                        if correcting {
                            decision_a = body["decision_id"].as_str().unwrap().to_string();
                        }
                    } else {
                        revision_b += 1;
                        if correcting {
                            decision_b = body["decision_id"].as_str().unwrap().to_string();
                        }
                    }
                }
                11 => busy += 1,
                _ => other.push((code, body["reason"].clone())),
            }
        }
        (ok, busy, other)
    });

    let mut ok_sheets = 0u32;
    let mut damaged = Vec::new();
    while Instant::now() < stop {
        let failed = damaged.len();
        let out = tmp.path.join(format!("sheet-{ok_sheets}-{failed}.txt"));
        let (code, body) = run(&[
            "review",
            "--vault",
            vault.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ]);
        match code {
            0 => {
                ok_sheets += 1;
                let text = fs::read_to_string(&out).unwrap();
                assert!(!sheet_torn(&text), "{out:?}\n{text}");
                assert!(
                    text.contains(&format!("来源分组 source_id={source_a}")),
                    "{text}"
                );
                assert!(
                    text.contains(&format!("来源分组 source_id={source_b}")),
                    "{text}"
                );
                assert!(!text.contains("这条来源没有当前金额或服务期。"), "{text}");
                assert_fact_history(&text);
            }
            11 => assert_eq!(body["reason"], "writer_busy", "{body}"),
            _ => damaged.push((code, body)),
        }
    }
    let (writes, _busy, other) = writer.join().unwrap();
    assert!(writes > 0, "no correct or undo landed");
    assert!(other.is_empty(), "{other:?}");
    assert!(ok_sheets > 0);
    assert!(damaged.is_empty(), "{damaged:?}");
    let (code, opened) = run(&["open", "--vault", vault.to_str().unwrap()]);
    assert_eq!(code, 0, "{opened}");
    assert_eq!(opened["outcome"], "Ready");
}

#[test]
fn review_lock_contention_is_not_damage() {
    let tmp = Tmp::new("busy");
    let vault = tmp.path.join("vault");
    let (code, init) = run(&["init", "--vault", vault.to_str().unwrap(), "--format", "4"]);
    assert_eq!(code, 0, "{init}");
    let (code, seed) = html(&vault, "seed", &doc(100));
    assert_eq!(code, 0, "{seed}");
    let held = hold_flock(&vault.join("writer.lock"));
    let out = tmp.path.join("busy.txt");
    let (code, body) = run(&[
        "review",
        "--vault",
        vault.to_str().unwrap(),
        "--out",
        out.to_str().unwrap(),
    ]);
    assert_eq!(code, 11, "{body}");
    assert_eq!(body["outcome"], "Locked");
    assert_eq!(body["reason"], "writer_busy");
    assert!(!out.exists());
    let (code, opened) = run(&["open", "--vault", vault.to_str().unwrap()]);
    assert_eq!(code, 11, "{opened}");
    assert_eq!(opened["reason"], "writer_busy");
    assert_ne!(opened["outcome"], "ReadOnlyDamaged");
    drop(held);
    let (code, opened) = run(&["open", "--vault", vault.to_str().unwrap()]);
    assert_eq!(code, 0, "{opened}");
    assert_eq!(opened["outcome"], "Ready");
    let sheet = tmp.path.join("after.txt");
    let (code, reviewed) = run(&[
        "review",
        "--vault",
        vault.to_str().unwrap(),
        "--out",
        sheet.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{reviewed}");
    let text = fs::read_to_string(&sheet).unwrap();
    assert!(text.contains("当前记录金额 USD 1.00"));
    assert!(!sheet_torn(&text));
}

fn fact_id(body: &Value, source_id: &str, fact_type: &str) -> String {
    body["facts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["source_id"] == source_id && row["fact_type"] == fact_type)
        .unwrap()["fact_id"]
        .as_str()
        .unwrap()
        .to_string()
}

fn sheet_torn(text: &str) -> bool {
    let mut bare = Vec::new();
    for part in text.split("来源分组 source_id=").skip(1) {
        let mut lines = part.lines();
        let id = lines.next().unwrap_or("").trim();
        let body = lines.collect::<Vec<_>>().join("\n");
        if body.contains("这条来源没有当前金额或服务期。") {
            bare.push(id.to_string());
        }
        if bare.iter().any(|item| item == id)
            && body.contains("不是事实")
            && body.contains("usable=true")
        {
            return true;
        }
    }
    false
}

fn assert_fact_history(text: &str) {
    for part in text.split("种类 事实").skip(1) {
        let block = part.split("来源分组 source_id=").next().unwrap_or(part);
        let revision = block
            .lines()
            .find_map(|line| line.strip_prefix("revision="))
            .unwrap_or("0")
            .parse::<usize>()
            .unwrap();
        let decisions = block
            .lines()
            .filter(|line| line.starts_with("决定 "))
            .count();
        assert_eq!(decisions, revision, "{block}");
    }
}

struct Flock {
    child: Option<std::process::Child>,
}

impl Flock {
    fn hold(lock: &Path) -> Self {
        let ready = std::env::temp_dir().join(format!(
            "subvault-flock-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut child = Command::new("python3")
            .arg("-c")
            .arg(
                r#"
import fcntl, sys, time
handle = open(sys.argv[1], "a+")
fcntl.flock(handle.fileno(), fcntl.LOCK_EX | fcntl.LOCK_NB)
open(sys.argv[2], "w").write("ready")
time.sleep(30)
"#,
            )
            .arg(lock)
            .arg(&ready)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let start = Instant::now();
        loop {
            if ready.is_file() {
                break;
            }
            if let Some(status) = child.try_wait().unwrap() {
                panic!("flock helper exited: {status}");
            }
            if start.elapsed() > Duration::from_secs(5) {
                let _ = child.kill();
                panic!("flock helper did not take the lock");
            }
            thread::sleep(Duration::from_millis(10));
        }
        let _ = fs::remove_file(&ready);
        Self { child: Some(child) }
    }
}

impl Drop for Flock {
    fn drop(&mut self) {
        if let Some(child) = self.child.as_mut() {
            let _ = Command::new("kill")
                .args(["-KILL", &child.id().to_string()])
                .status();
            let _ = child.wait();
        }
    }
}

fn hold_flock(lock: &Path) -> Flock {
    Flock::hold(lock)
}
