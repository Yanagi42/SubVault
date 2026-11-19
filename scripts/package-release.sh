#!/usr/bin/env bash
# Build portable SubVault release bundles (zip + tar.gz + standalone web demo) and SHA256SUMS.txt.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

RAW_VERSION="${1:-$(awk -F'"' '/^version *=/{print $2; exit}' Cargo.toml)}"
VERSION="${RAW_VERSION#v}"
OS="$(uname -s | tr '[:upper:]' '[:lower:]')"
ARCH="$(uname -m)"
BUNDLE_NAME="subvault-v${VERSION}-${OS}-${ARCH}"
WEB_ARCHIVE_NAME="subvault-v${VERSION}-web-demo"

DIST_DIR="$ROOT/dist"
STAGE_DIR="$DIST_DIR/$BUNDLE_NAME"
GIT_SHA="${SUBVAULT_GIT_SHA:-$(git rev-parse HEAD 2>/dev/null || echo unknown)}"

echo "==> Packaging SubVault v${VERSION} (${OS}-${ARCH}, git=${GIT_SHA:0:12})"

echo "==> 1/5 Verifying synthetic T0 corpus..."
python3 synthetic/generate_corpus.py --check

if command -v node >/dev/null 2>&1; then
  echo "==> 2/5 Running Web Demo smoke & subpath checks..."
  node scripts/test-web.mjs
else
  echo "==> 2/5 Skipping Node Web Demo check (node not installed)"
fi

echo "==> 3/5 Building release binary..."
SUBVAULT_GIT_SHA="$GIT_SHA" cargo build --release --locked

BIN_PATH="$ROOT/target/release/subvault"
if [[ ! -x "$BIN_PATH" ]]; then
  echo "Expected compiled binary at $BIN_PATH" >&2
  exit 1
fi

echo "==> 4/5 Assembling portable directory: dist/${BUNDLE_NAME}/"
rm -rf "$DIST_DIR"
mkdir -p "$STAGE_DIR/bin" "$STAGE_DIR/web" "$STAGE_DIR/synthetic"

cp "$BIN_PATH" "$STAGE_DIR/bin/subvault"
chmod 0755 "$STAGE_DIR/bin/subvault"

# Copy pure-static web assets (excluding package.json used only for local Node ESM tests)
cp "$ROOT/web/.nojekyll" "$STAGE_DIR/web/.nojekyll"
cp "$ROOT/web/index.html" "$STAGE_DIR/web/index.html"
cp "$ROOT/web/styles.css" "$STAGE_DIR/web/styles.css"
cp "$ROOT/web/app.js" "$STAGE_DIR/web/app.js"
cp "$ROOT/web/engine.js" "$STAGE_DIR/web/engine.js"
cp "$ROOT/web/demo-data.js" "$STAGE_DIR/web/demo-data.js"
cp "$ROOT/web/zip.js" "$STAGE_DIR/web/zip.js"
cp -R "$ROOT/web/assets" "$STAGE_DIR/web/assets"

# Copy verified T0 synthetic corpus fixtures
cp -R "$ROOT/synthetic/corpus" "$STAGE_DIR/synthetic/corpus"

# Copy documentation & metadata
cp "$ROOT/README.md" "$STAGE_DIR/README.md"
if [[ -f "$ROOT/RELEASE_NOTES.md" ]]; then
  cp "$ROOT/RELEASE_NOTES.md" "$STAGE_DIR/RELEASE_NOTES.md"
fi
cat >"$STAGE_DIR/VERSION.txt" <<EOF
version=v${VERSION}
target=${OS}-${ARCH}
git_sha=${GIT_SHA}
built_utc=$(date -u +%Y-%m-%dT%H:%M:%SZ)
EOF

# Create portable launcher script inside bundle
cat >"$STAGE_DIR/run-demo.sh" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
MODE="${1:-cli}"

case "$MODE" in
  web)
    PORT="${2:-4173}"
    echo "Serving SubVault Web Demo at http://127.0.0.1:${PORT}/ (Ctrl+C to stop)"
    exec python3 -m http.server "$PORT" --bind 127.0.0.1 --directory "$HERE/web"
    ;;
  cli)
    WORK="$(mktemp -d "${TMPDIR:-/tmp}/subvault-portable-demo-XXXXXX")"
    trap 'rm -rf "$WORK"' EXIT
    VAULT="$WORK/vault"
    BILL="$WORK/northline-2026-11.html"
    EXPORT_DIR="$WORK/offline-exit-pack"
    REVIEW_OUT="$WORK/review.txt"

    cat >"$BILL" <<'HTML'
<!doctype html>
<html>
<body>
  <h2>Northline Video Billing Statement</h2>
  <table>
    <tr><th>Amount paid</th><td>USD 15.99</td></tr>
    <tr><th>Service period:</th><td>2026-11-01 through 2026-11-30</td></tr>
  </table>
</body>
</html>
HTML

    echo "==> 1. SubVault binary version:"
    "$HERE/bin/subvault" version
    echo ""
    echo "==> 2. Initializing isolated Format 4 vault at $VAULT..."
    "$HERE/bin/subvault" init --vault "$VAULT" --format 4
    echo ""
    echo "==> 3. Preserving & extracting static HTML bill:"
    "$HERE/bin/subvault" html --vault "$VAULT" --command-id demo-html-1 --source "$BILL"
    echo ""
    echo "==> 4. Re-importing same bytes with a second CommandId (dedup / reuse proof):"
    "$HERE/bin/subvault" html --vault "$VAULT" --command-id demo-html-2 --source "$BILL"
    echo ""
    echo "==> 5. Exporting SHA-256 verifiable offline exit package:"
    "$HERE/bin/subvault" export --vault "$VAULT" --out "$EXPORT_DIR"
    echo ""
    echo "==> 6. Independently verifying exported package (without live vault):"
    "$HERE/bin/subvault" verify-package --package "$EXPORT_DIR"
    echo ""
    echo "==> 7. Writing lock-consistent read-only review sheet:"
    "$HERE/bin/subvault" review --vault "$VAULT" --out "$REVIEW_OUT" --package "$EXPORT_DIR"
    echo ""
    echo "--- Generated Review Sheet ---"
    cat "$REVIEW_OUT"
    ;;
  *)
    echo "Usage: ./run-demo.sh [cli | web [port]]" >&2
    exit 2
    ;;
esac
EOF
chmod 0755 "$STAGE_DIR/run-demo.sh"

echo "==> 5/5 Creating portable zip/tar.gz archives and SHA256SUMS.txt..."
python3 - "$DIST_DIR" "$BUNDLE_NAME" "$WEB_ARCHIVE_NAME" <<'PY'
import os
import sys
import tarfile
import zipfile
from pathlib import Path

dist_dir = Path(sys.argv[1])
bundle_name = sys.argv[2]
web_archive_name = sys.argv[3]
stage_dir = dist_dir / bundle_name


def add_dir_to_zip(zip_path: Path, src_dir: Path, arc_prefix: str) -> None:
    with zipfile.ZipFile(zip_path, "w", compression=zipfile.ZIP_DEFLATED, compresslevel=9) as zf:
        for path in sorted(src_dir.rglob("*")):
            if path.is_dir():
                continue
            rel = path.relative_to(src_dir).as_posix()
            arcname = f"{arc_prefix}/{rel}" if arc_prefix else rel
            info = zipfile.ZipInfo(arcname)
            st = path.stat()
            info.external_attr = (st.st_mode & 0xFFFF) << 16
            info.compress_type = zipfile.ZIP_DEFLATED
            zf.writestr(info, path.read_bytes())


add_dir_to_zip(dist_dir / f"{bundle_name}.zip", stage_dir, bundle_name)
add_dir_to_zip(dist_dir / f"{web_archive_name}.zip", stage_dir / "web", web_archive_name)

tar_path = dist_dir / f"{bundle_name}.tar.gz"
with tarfile.open(tar_path, "w:gz") as tf:
    for path in sorted(stage_dir.rglob("*")):
        rel = path.relative_to(dist_dir).as_posix()
        tf.add(path, arcname=rel, recursive=False)
PY

(
  cd "$DIST_DIR"
  sha256sum \
    "${BUNDLE_NAME}.zip" \
    "${BUNDLE_NAME}.tar.gz" \
    "${WEB_ARCHIVE_NAME}.zip" \
    > SHA256SUMS.txt
  sha256sum -c SHA256SUMS.txt
)

echo ""
echo "Release artifacts ready in dist/:"
ls -lh "$DIST_DIR/${BUNDLE_NAME}.zip" "$DIST_DIR/${BUNDLE_NAME}.tar.gz" "$DIST_DIR/${WEB_ARCHIVE_NAME}.zip" "$DIST_DIR/SHA256SUMS.txt"
