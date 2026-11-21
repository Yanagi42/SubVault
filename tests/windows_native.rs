#![cfg(windows)]

use std::ffi::c_void;
use std::fs;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::ptr;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;

fn bin() -> PathBuf {
    if let Ok(path) = std::env::var("SUBVAULT_TEST_BIN") {
        return PathBuf::from(path);
    }
    let compiled = PathBuf::from(env!("CARGO_BIN_EXE_subvault"));
    if compiled.exists() {
        return compiled;
    }
    // Cross-compiled test binary executed on a Windows host: locate `subvault.exe`
    // in the sibling parent directory of `deps\*.exe`.
    if let Ok(exe) = std::env::current_exe() {
        if let Some(candidate) = exe
            .parent()
            .and_then(|p| p.parent())
            .map(|p| p.join("subvault.exe"))
        {
            if candidate.exists() {
                return candidate;
            }
        }
    }
    compiled
}

struct Tmp {
    path: PathBuf,
}

impl Tmp {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "subvault-win-it-{label}-{}-{}",
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
        .unwrap_or_else(|err| panic!("failed to spawn {}: {err}", bin().display()));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let body = serde_json::from_str(&stdout).unwrap_or_else(|err| {
        panic!(
            "stdout is not JSON ({err})\nstdout:\n{stdout}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (output.status.code().unwrap_or(1), body)
}

fn create_junction(link: &Path, target: &Path) {
    let status = Command::new("cmd")
        .args([
            "/C",
            "mklink",
            "/J",
            link.to_str().unwrap(),
            target.to_str().unwrap(),
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .expect("spawn cmd mklink /J");
    assert!(
        status.success(),
        "mklink /J {} -> {} failed",
        link.display(),
        target.display()
    );
}

fn minimal_pdf(lines: &[&str]) -> Vec<u8> {
    let mut stream = String::from("BT\n/F1 12 Tf\n");
    for (idx, line) in lines.iter().enumerate() {
        let y = 720 - (idx as i32 * 18);
        let esc = line
            .replace('\\', "\\\\")
            .replace('(', "\\(")
            .replace(')', "\\)");
        stream.push_str(&format!("72 {y} Td ({esc}) Tj\n"));
    }
    stream.push_str("ET\n");
    let mut out = Vec::new();
    out.extend_from_slice(b"%PDF-1.4\n");
    let mut offsets = Vec::new();
    let objs = vec![
        "1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n".to_string(),
        "2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 >>\nendobj\n".to_string(),
        "3 0 obj\n<< /Type /Page /Parent 2 0 R /Contents 4 0 R >>\nendobj\n".to_string(),
        format!(
            "4 0 obj\n<< /Length {} >>\nstream\n{}endstream\nendobj\n",
            stream.len(),
            stream
        ),
    ];
    for obj in &objs {
        offsets.push(out.len());
        out.extend_from_slice(obj.as_bytes());
    }
    let xref_pos = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n", objs.len() + 1).as_bytes());
    out.extend_from_slice(b"0000000000 65535 f \n");
    for off in offsets {
        out.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref_pos}\n%%EOF\n",
            objs.len() + 1
        )
        .as_bytes(),
    );
    out
}

#[repr(C)]
struct Overlapped {
    internal: usize,
    internal_high: usize,
    offset: u32,
    offset_high: u32,
    h_event: *mut c_void,
}

#[link(name = "kernel32")]
extern "system" {
    fn CreateFileW(
        name: *const u16,
        access: u32,
        share: u32,
        security: *const c_void,
        disposition: u32,
        flags: u32,
        template: *mut c_void,
    ) -> *mut c_void;
    fn LockFileEx(
        file: *mut c_void,
        flags: u32,
        reserved: u32,
        len_low: u32,
        len_high: u32,
        overlapped: *mut Overlapped,
    ) -> i32;
    fn UnlockFileEx(
        file: *mut c_void,
        reserved: u32,
        len_low: u32,
        len_high: u32,
        overlapped: *mut Overlapped,
    ) -> i32;
    fn CloseHandle(handle: *mut c_void) -> i32;
}

struct ExternalWinLock {
    handle: *mut c_void,
}

impl ExternalWinLock {
    fn hold(path: &Path) -> Self {
        let mut wide: Vec<u16> = path.as_os_str().encode_wide().collect();
        wide.push(0);
        let handle = unsafe {
            CreateFileW(
                wide.as_ptr(),
                0x8000_0000 | 0x4000_0000, // GENERIC_READ | GENERIC_WRITE
                0x0000_0001 | 0x0000_0002, // FILE_SHARE_READ | FILE_SHARE_WRITE
                ptr::null(),
                4, // OPEN_ALWAYS
                0x80,
                ptr::null_mut(),
            )
        };
        assert!(!handle.is_null() && handle != (-1isize) as usize as *mut c_void);
        let mut ov = Overlapped {
            internal: 0,
            internal_high: 0,
            offset: 0,
            offset_high: 0,
            h_event: ptr::null_mut(),
        };
        let ok = unsafe { LockFileEx(handle, 0x0000_0002 | 0x0000_0001, 0, 1, 0, &mut ov) };
        assert_ne!(ok, 0, "parent process failed to lock writer.lock");
        Self { handle }
    }
}

impl Drop for ExternalWinLock {
    fn drop(&mut self) {
        let mut ov = Overlapped {
            internal: 0,
            internal_high: 0,
            offset: 0,
            offset_high: 0,
            h_event: ptr::null_mut(),
        };
        unsafe {
            UnlockFileEx(self.handle, 0, 1, 0, &mut ov);
            CloseHandle(self.handle);
        }
    }
}

#[test]
fn windows_format5_end_to_end_pdf_html_export_verify_review_and_read() {
    let tmp = Tmp::new("e2e");
    let vault = tmp.path.join("MyVault");
    let (code, init) = run(&["init", "--vault", vault.to_str().unwrap(), "--format", "5"]);
    assert_eq!(code, 0, "{init}");
    assert_eq!(init["outcome"], "Initialized");
    assert_eq!(
        init["platform_limits"]["writer_lock"],
        "windows_lockfileex_exclusive_nonblocking"
    );
    assert_eq!(
        init["platform_limits"]["windows_native"],
        "tested_x86_64_windows_msvc"
    );
    assert_eq!(
        init["platform_limits"]["directory_identity"],
        "windows_file_id_128"
    );
    assert_eq!(
        init["platform_limits"]["directory_publish"],
        "windows_file_rename_info_noreplace"
    );

    // 1. Ingest a real PDF + renamed duplicate
    let pdf_bytes = minimal_pdf(&[
        "SYNTHETIC VENDOR RECEIPT",
        "Amount paid USD 19.00",
        "Service period: 2026-01-01 through 2026-01-31",
        "Account: u***@example.test",
    ]);
    let pdf1 = tmp.path.join("invoice-jan.pdf");
    let pdf2 = tmp.path.join("invoice-jan-copy.pdf");
    fs::write(&pdf1, &pdf_bytes).unwrap();
    fs::write(&pdf2, &pdf_bytes).unwrap();

    let (code, p1) = run(&[
        "pdf",
        "--vault",
        vault.to_str().unwrap(),
        "--command-id",
        "win-pdf-1",
        "--source",
        pdf1.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{p1}");
    assert_eq!(p1["outcome"], "Committed");
    assert_eq!(p1["new_effect"], true);
    let source_id = p1["source_id"].as_str().unwrap().to_string();

    let (code, p2) = run(&[
        "pdf",
        "--vault",
        vault.to_str().unwrap(),
        "--command-id",
        "win-pdf-2",
        "--source",
        pdf2.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{p2}");
    assert_eq!(p2["outcome"], "AlreadyPreserved");
    assert_eq!(p2["content_identity"], "same_bytes");
    assert_eq!(p2["facts_created"], false);
    assert_eq!(p2["source_id"], p1["source_id"]);
    assert_eq!(p2["object_id"], p1["object_id"]);

    // 2. Ingest a static HTML bill into the same Format 5 vault
    let html_path = tmp.path.join("bill.html");
    fs::write(
        &html_path,
        b"<html><body><p>Amount paid USD 9.99</p><p>Service period: 2026-02-01 through 2026-02-28</p></body></html>\n",
    )
    .unwrap();
    let (code, h1) = run(&[
        "html",
        "--vault",
        vault.to_str().unwrap(),
        "--command-id",
        "win-html-1",
        "--source",
        html_path.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{h1}");

    // 3. Export offline package + standalone verify-package + review sheet
    let pkg = tmp.path.join("offline_pkg");
    let (code, exp) = run(&[
        "export",
        "--vault",
        vault.to_str().unwrap(),
        "--out",
        pkg.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{exp}");
    assert_eq!(exp["completeness"], "complete");

    let (code, ver) = run(&["verify-package", "--package", pkg.to_str().unwrap()]);
    assert_eq!(code, 0, "{ver}");
    assert_eq!(ver["outcome"], "Verified");

    let sheet = tmp.path.join("review.txt");
    let (code, rev) = run(&[
        "review",
        "--vault",
        vault.to_str().unwrap(),
        "--out",
        sheet.to_str().unwrap(),
        "--package",
        pkg.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{rev}");
    assert!(sheet.exists());

    // 4. Read preserved bytes out cleanly and verify bit-for-bit equality
    let out_pdf = tmp.path.join("recovered.pdf");
    let (code, rd) = run(&[
        "read",
        "--vault",
        vault.to_str().unwrap(),
        "--source-id",
        &source_id,
        "--out",
        out_pdf.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{rd}");
    assert_eq!(fs::read(&out_pdf).unwrap(), pdf_bytes);
}

#[test]
fn windows_place_output_refuses_case_alias_traversal_and_ntfs_junction() {
    let tmp = Tmp::new("place");
    let vault = tmp.path.join("CaseVault");
    let (code, init) = run(&["init", "--vault", vault.to_str().unwrap(), "--format", "5"]);
    assert_eq!(code, 0, "{init}");

    let pdf_path = tmp.path.join("sample.pdf");
    fs::write(&pdf_path, minimal_pdf(&["Amount paid USD 12.00"])).unwrap();
    let (code, ing) = run(&[
        "pdf",
        "--vault",
        vault.to_str().unwrap(),
        "--command-id",
        "place-1",
        "--source",
        pdf_path.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{ing}");
    let source_id = ing["source_id"].as_str().unwrap();

    // 1. Different-case spelling of vault directory -> MUST be refused as output_inside_vault
    let lower_out = tmp.path.join("casevault").join("leaked.pdf");
    let (code, body) = run(&[
        "read",
        "--vault",
        vault.to_str().unwrap(),
        "--source-id",
        source_id,
        "--out",
        lower_out.to_str().unwrap(),
    ]);
    assert_eq!(code, 16, "{body}");
    assert_eq!(body["reason"], "output_inside_vault");
    assert!(!lower_out.exists());

    // 2. Parent traversal (`..`) back into case-aliased vault -> output_inside_vault
    let trav_out = vault
        .join("objects")
        .join("..")
        .join("..")
        .join("CASEVAULT")
        .join("trav.pdf");
    let (code, body) = run(&[
        "read",
        "--vault",
        vault.to_str().unwrap(),
        "--source-id",
        source_id,
        "--out",
        trav_out.to_str().unwrap(),
    ]);
    assert_eq!(code, 16, "{body}");
    assert_eq!(body["reason"], "output_inside_vault");

    // 3. NTFS directory junction pointing into vault -> output_inside_vault for BOTH read & export
    let junction = tmp.path.join("alias_junction");
    create_junction(&junction, &vault);
    let junc_read = junction.join("via-junction.pdf");
    let (code, body) = run(&[
        "read",
        "--vault",
        vault.to_str().unwrap(),
        "--source-id",
        source_id,
        "--out",
        junc_read.to_str().unwrap(),
    ]);
    assert_eq!(code, 16, "{body}");
    assert_eq!(body["reason"], "output_inside_vault");
    assert!(!vault.join("via-junction.pdf").exists());

    let junc_export = junction.join("nested_export");
    let (code, body) = run(&[
        "export",
        "--vault",
        vault.to_str().unwrap(),
        "--out",
        junc_export.to_str().unwrap(),
    ]);
    assert_eq!(code, 16, "{body}");
    assert_eq!(body["reason"], "output_inside_vault");
    assert!(!vault.join("nested_export").exists());

    // 4. Case-alias init collision & trailing backslash refusal
    let trailing_same = format!("{}\\", tmp.path.join("casevault").display());
    let (code, body) = run(&["init", "--vault", &trailing_same, "--format", "5"]);
    assert_eq!(code, 16, "{body}");
    assert_eq!(body["reason"], "vault_path_exists");
    assert_eq!(body["created_new_vault"], false);

    // 5. Existing junction as init/export target is refused without overwriting target
    let outside = tmp.path.join("outside_keep");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("KEEP.txt"), b"must-survive").unwrap();
    let target_junc = tmp.path.join("target_junction");
    create_junction(&target_junc, &outside);

    let (code, body) = run(&[
        "init",
        "--vault",
        target_junc.to_str().unwrap(),
        "--format",
        "5",
    ]);
    assert_eq!(code, 16, "{body}");
    assert_eq!(body["reason"], "vault_path_exists");
    assert_eq!(fs::read(outside.join("KEEP.txt")).unwrap(), b"must-survive");

    let (code, body) = run(&[
        "export",
        "--vault",
        vault.to_str().unwrap(),
        "--out",
        target_junc.to_str().unwrap(),
    ]);
    assert_eq!(code, 16, "{body}");
    assert_eq!(body["reason"], "output_exists");
    assert_eq!(fs::read(outside.join("KEEP.txt")).unwrap(), b"must-survive");

    let _ = fs::remove_dir(&junction);
    let _ = fs::remove_dir(&target_junc);
}

#[test]
fn windows_two_process_lockfileex_contention_returns_locked_and_recovers_after_release() {
    let tmp = Tmp::new("lock");
    let vault = tmp.path.join("LockVault");
    let (code, init) = run(&["init", "--vault", vault.to_str().unwrap(), "--format", "5"]);
    assert_eq!(code, 0, "{init}");

    // Writing fake pid/expiry into writer.lock must NOT block open when no LockFileEx is held.
    let lock_path = vault.join("writer.lock");
    fs::write(&lock_path, b"pid=999999\nexpires=2099-01-01T00:00:00Z\n").unwrap();
    let (code, open_ok) = run(&["open", "--vault", vault.to_str().unwrap()]);
    assert_eq!(code, 0, "{open_ok}");

    // Hold real Win32 LockFileEx in this parent process; child subvault.exe MUST exit 11 (Locked).
    let held = ExternalWinLock::hold(&lock_path);
    let (code, busy_open) = run(&["open", "--vault", vault.to_str().unwrap()]);
    assert_eq!(code, 11, "{busy_open}");
    assert_eq!(busy_open["outcome"], "Locked");
    assert_eq!(busy_open["reason"], "writer_busy");

    let review_out = tmp.path.join("busy-review.txt");
    let (code, busy_rev) = run(&[
        "review",
        "--vault",
        vault.to_str().unwrap(),
        "--out",
        review_out.to_str().unwrap(),
    ]);
    assert_eq!(code, 11, "{busy_rev}");
    assert_eq!(busy_rev["outcome"], "Locked");
    assert_eq!(busy_rev["reason"], "writer_busy");
    assert!(!review_out.exists());

    // Release Win32 lock -> child subvault.exe immediately succeeds again.
    drop(held);
    let (code, after_release) = run(&["open", "--vault", vault.to_str().unwrap()]);
    assert_eq!(code, 0, "{after_release}");
    assert_eq!(after_release["outcome"], "Ready");
}
