#!/usr/bin/env bash
# Reproduce the T0/T1 evidence bundle. This does not upload vault bytes.
set -euo pipefail

root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"
out=/opt/cursor/artifacts/subvault-t0-t1
rm -rf "$out"
mkdir -p "$out/licenses"

{
  echo "started_utc=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "root=$root"
  uname -a
  rustc --version
  cargo --version
  python3 --version
  sqlite3 -version
  gcc --version | sed -n '1p'
  findmnt -T "$root" || true
  stat -f -c 'stat_type=%T magic=0x%t' "$root" || true
  echo "nproc=$(nproc)"
  echo "memtotal_kb=$(awk '/MemTotal/ {print $2}' /proc/meminfo)"
  echo "HEAD=$(git rev-parse HEAD)"
  echo "branch=$(git rev-parse --abbrev-ref HEAD)"
  git status --short
  git log --oneline main..HEAD
} >"$out/environment.txt"

python3 synthetic/generate_corpus.py --check >"$out/corpus-check.txt" 2>&1

export SUBVAULT_GIT_SHA
SUBVAULT_GIT_SHA=$(git rev-parse HEAD)
rm -f target/t1-evidence.jsonl
test_started=$(date -u +%Y-%m-%dT%H:%M:%SZ)
set +e
cargo test --offline --test public_behavior -- --test-threads=1 >"$out/cargo-test.txt" 2>&1
test_rc=$?
set -e
test_finished=$(date -u +%Y-%m-%dT%H:%M:%SZ)
{
  echo "exit=$test_rc"
  echo "started_utc=$test_started"
  echo "finished_utc=$test_finished"
} >"$out/cargo-test.exit"

if [[ -f target/t1-evidence.jsonl ]]; then
  cp target/t1-evidence.jsonl "$out/t1-evidence.jsonl"
fi
if [[ "$test_rc" -eq 0 ]]; then
  if grep -q 'SENTINEL_BODY_9f3c2a\|Amount paid' "$out/cargo-test.txt" "$out/t1-evidence.jsonl"; then
    echo "evidence contains original document text" >&2
    exit 1
  fi
fi

bin=target/debug/subvault
sha256sum "$bin" >"$out/binary.sha256"
"$bin" version >"$out/version.json"

python3 - "$out/licenses" <<'PY'
import json, os, shutil, subprocess, sys
dest = sys.argv[1]
meta = json.loads(subprocess.check_output(["cargo", "metadata", "--offline", "--format-version", "1"]))
root_id = next(p["id"] for p in meta["packages"] if p["name"] == "subvault" and p["source"] is None)
nodes = {n["id"]: n for n in next(r["nodes"] for r in meta["resolve"]["root"] if r["id"] == root_id)}
wanted = set(nodes) - {root_id}
lines = []
for pkg in sorted(meta["packages"], key=lambda p: (p["name"], p["version"])):
    if pkg["id"] not in wanted:
        continue
    license_name = pkg.get("license") or ""
    lines.append(f"{pkg['name']} {pkg['version']} {license_name}")
    folder = os.path.join(dest, f"{pkg['name']}-{pkg['version']}")
    os.makedirs(folder, exist_ok=True)
    src = os.path.dirname(pkg["manifest_path"])
    copied = False
    for name in os.listdir(src):
        if name.upper().startswith("LICENSE") or name.upper().startswith("COPYING"):
            shutil.copy2(os.path.join(src, name), os.path.join(folder, name))
            copied = True
    if not copied:
        open(os.path.join(folder, "NO_LICENSE_FILE"), "w").write(license_name + "\n")
open(os.path.join(dest, "packages.txt"), "w").write("\n".join(lines) + "\n")
sqlite = None
for pkg in meta["packages"]:
    if pkg["name"] == "libsqlite3-sys" and pkg["id"] in wanted:
        sqlite = os.path.join(os.path.dirname(pkg["manifest_path"]), "sqlite3", "sqlite3.h")
if sqlite and os.path.isfile(sqlite):
    text = open(sqlite, encoding="utf-8", errors="replace").read().splitlines()
    version = next((line for line in text if "SQLITE_VERSION " in line and "#define" in line), "")
    notice = "\n".join(text[:12])
    open(os.path.join(dest, "sqlite-amalgamation-notice.txt"), "w").write(notice + "\n\n" + version + "\n")
PY

enospc_note=$out/enospc.txt
big=/tmp/subvault-enospc-source.bin
if sudo -n true >/dev/null 2>&1; then
  mount_dir=$(mktemp -d /tmp/subvault-enospc-XXXX)
  cleanup() {
    rm -f "$big"
    sudo -n umount "$mount_dir" >/dev/null 2>&1 || true
    rmdir "$mount_dir" >/dev/null 2>&1 || true
  }
  trap cleanup EXIT
  sudo -n mount -t tmpfs -o size=2097152,mode=0755 tmpfs "$mount_dir"
  sudo -n chown "$(id -u):$(id -g)" "$mount_dir"
  vault=$mount_dir/vault
  "$bin" init --vault "$vault" >"$out/enospc-init.json"
  tiny=$mount_dir/tiny.bin
  printf 'tiny-preserved' >"$tiny"
  "$bin" import --vault "$vault" --command-id tiny --media-type application/octet-stream --source "$tiny" >"$out/enospc-tiny.json"
  avail=$(df -B1 --output=avail "$mount_dir" | awk 'NR==2 {print $1}')
  dd if=/dev/zero of="$big" bs=1 count=$((avail + 65536)) status=none
  set +e
  "$bin" import --vault "$vault" --command-id big --media-type application/octet-stream --source "$big" >"$out/enospc-import.json"
  enospc_rc=$?
  set -e
  set +e
  "$bin" open --vault "$vault" >"$out/enospc-open.json"
  open_rc=$?
  set -e
  source_id=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["source_id"])' "$out/enospc-tiny.json")
  read_out=$mount_dir/read-back.bin
  "$bin" read --vault "$vault" --source-id "$source_id" --out "$read_out" >"$out/enospc-read.json"
  cmp "$tiny" "$read_out"
  python3 - "$out/enospc-import.json" "$enospc_rc" <<'PY'
import json, sys
body = json.load(open(sys.argv[1]))
exit_code = int(sys.argv[2])
if body.get("injected") is not False:
    raise SystemExit(f"tmpfs result was marked injected: {body}")
if exit_code == 0 or body.get("reason") != "enospc":
    raise SystemExit(f"bounded tmpfs did not hit ENOSPC: exit={exit_code} body={body}")
PY
  {
    echo "experiment=bounded_tmpfs"
    echo "tmpfs_bytes=2097152"
    echo "available_before_large_copy=$avail"
    echo "source_bytes=$((avail + 65536))"
    echo "source_is_on_host_tmp_not_on_the_tmpfs=true"
    echo "import_exit=$enospc_rc"
    echo "open_exit=$open_rc"
    echo "injected=false"
    echo "label=real_tmpfs_enospc_not_the_inject_hook"
  } >"$enospc_note"
  cleanup
  trap - EXIT
else
  echo "skipped=no_passwordless_sudo" >"$enospc_note"
fi

{
  echo "finished_utc=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "test_exit=$test_rc"
} >>"$out/environment.txt"

if [[ "$test_rc" -ne 0 ]]; then
  exit "$test_rc"
fi
