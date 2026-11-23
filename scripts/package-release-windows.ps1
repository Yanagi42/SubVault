# Build portable SubVault Windows release bundle (.zip) and SHA256SUMS-windows.txt.
param(
    [string]$TagName = ""
)

$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $Root

if ([string]::IsNullOrWhiteSpace($TagName)) {
    $CargoToml = Get-Content -Path (Join-Path $Root "Cargo.toml") -Raw
    if ($CargoToml -match '(?m)^version\s*=\s*"([^"]+)"') {
        $Version = $Matches[1]
    } else {
        throw "Could not determine version from Cargo.toml"
    }
} else {
    $Version = $TagName.TrimStart("v")
}

$BundleName = "subvault-v$Version-windows-x86_64"
$DistDir = Join-Path $Root "dist"
$StageDir = Join-Path $DistDir $BundleName
$ZipPath = Join-Path $DistDir "$BundleName.zip"
$SumsPath = Join-Path $DistDir "SHA256SUMS-windows.txt"

$GitSha = $env:SUBVAULT_GIT_SHA
if ([string]::IsNullOrWhiteSpace($GitSha)) {
    try {
        $GitSha = (git rev-parse HEAD 2>$null).Trim()
    } catch {
        $GitSha = "unknown"
    }
}
if ([string]::IsNullOrWhiteSpace($GitSha)) { $GitSha = "unknown" }

Write-Host "==> Packaging SubVault v$Version (windows-x86_64, git=$($GitSha.Substring(0, [Math]::Min(12, $GitSha.Length))))"

Write-Host "==> 1/4 Building release binaries (subvault.exe & subvault-app.exe)..."
$env:SUBVAULT_GIT_SHA = $GitSha
cargo build --release --locked
if ($LASTEXITCODE -ne 0) { throw "cargo build --release failed" }

$BinPath = Join-Path $Root "target\release\subvault.exe"
$AppBinPath = Join-Path $Root "target\release\subvault-app.exe"
if (-not (Test-Path $BinPath)) {
    throw "Expected compiled binary at $BinPath"
}
if (-not (Test-Path $AppBinPath)) {
    throw "Expected compiled workbench binary at $AppBinPath"
}

Write-Host "==> 2/4 Assembling portable directory: dist\$BundleName\"
New-Item -ItemType Directory -Force -Path $DistDir | Out-Null
if (Test-Path $StageDir) { Remove-Item -Recurse -Force $StageDir }
New-Item -ItemType Directory -Force -Path (Join-Path $StageDir "bin") | Out-Null
New-Item -ItemType Directory -Force -Path (Join-Path $StageDir "web") | Out-Null
New-Item -ItemType Directory -Force -Path (Join-Path $StageDir "synthetic") | Out-Null

Copy-Item -Path $BinPath -Destination (Join-Path $StageDir "bin\subvault.exe")
Copy-Item -Path $AppBinPath -Destination (Join-Path $StageDir "subvault-app.exe")

# Copy pure-static web assets (excluding package.json used only for local Node ESM tests)
$WebFiles = @(".nojekyll", "index.html", "styles.css", "app.js", "engine.js", "demo-data.js", "pdf.js", "zip.js")
foreach ($File in $WebFiles) {
    Copy-Item -Path (Join-Path $Root "web\$File") -Destination (Join-Path $StageDir "web\$File")
}
Copy-Item -Recurse -Path (Join-Path $Root "web\assets") -Destination (Join-Path $StageDir "web\assets")
Copy-Item -Recurse -Path (Join-Path $Root "synthetic\corpus") -Destination (Join-Path $StageDir "synthetic\corpus")
Copy-Item -Path (Join-Path $Root "README.md") -Destination (Join-Path $StageDir "README.md")
foreach ($Doc in @("RELEASE_NOTES.md", "SECURITY.md", "LICENSE", "LICENSE-MIT", "LICENSE-APACHE")) {
    $SrcDoc = Join-Path $Root $Doc
    if (Test-Path $SrcDoc) {
        Copy-Item -Path $SrcDoc -Destination (Join-Path $StageDir $Doc)
    }
}

$Utf8NoBom = [System.Text.UTF8Encoding]::new($false)
$BuiltUtc = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ssZ")
$VersionText = @"
version=v$Version
target=windows-x86_64
git_sha=$GitSha
built_utc=$BuiltUtc
"@
[System.IO.File]::WriteAllText((Join-Path $StageDir "VERSION.txt"), ($VersionText + "`n"), $Utf8NoBom)

$RunDemoScript = @'
param(
    [ValidateSet("cli", "web")]
    [string]$Mode = "cli",
    [int]$Port = 4173
)

$ErrorActionPreference = "Stop"
$Here = Split-Path -Parent $MyInvocation.MyCommand.Path
$Bin = Join-Path $Here "bin\subvault.exe"

if ($Mode -eq "web") {
    Write-Host "Serving SubVault Web Demo at http://127.0.0.1:$Port/ (Ctrl+C to stop)"
    python -m http.server $Port --bind 127.0.0.1 --directory (Join-Path $Here "web")
    exit $LASTEXITCODE
}

$Work = Join-Path ([System.IO.Path]::GetTempPath()) ("subvault-portable-demo-" + [System.Guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Force -Path $Work | Out-Null
try {
    $Vault = Join-Path $Work "vault"
    $Bill = Join-Path $Work "northline-2026-11.html"
    $ExportDir = Join-Path $Work "offline-exit-pack"
    $ReviewOut = Join-Path $Work "review.txt"

    $BillHtml = @"
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
"@
    [System.IO.File]::WriteAllText($Bill, ($BillHtml + "`n"), [System.Text.UTF8Encoding]::new($false))

    Write-Host "==> 1. SubVault binary version:"
    & $Bin --version
    Write-Host ""
    Write-Host "==> 2. Initializing isolated Format 5 vault (PDF + HTML) at $Vault..."
    & $Bin init --vault $Vault --format 5
    Write-Host ""
    Write-Host "==> 3. Preserving & extracting a real synthetic text-PDF receipt:"
    & $Bin pdf --vault $Vault --command-id demo-pdf-1 --source (Join-Path $Here "synthetic\corpus\originals\v-2026-09-receipt.pdf")
    Write-Host ""
    Write-Host "==> 4. Re-importing renamed byte-identical PDF copy (SHA-256 reuse proof):"
    & $Bin pdf --vault $Vault --command-id demo-pdf-dup --source (Join-Path $Here "synthetic\corpus\originals\renamed\NLV-september-statement-copy.pdf")
    Write-Host ""
    Write-Host "==> 5. Preserving & extracting a static HTML bill in the same Format 5 vault:"
    & $Bin html --vault $Vault --command-id demo-html-1 --source $Bill
    Write-Host ""
    Write-Host "==> 6. Exporting SHA-256 verifiable offline exit package:"
    & $Bin export --vault $Vault --out $ExportDir
    Write-Host ""
    Write-Host "==> 7. Independently verifying exported package (without live vault):"
    & $Bin verify-package --package $ExportDir
    Write-Host ""
    Write-Host "==> 8. Writing lock-consistent read-only review sheet:"
    & $Bin review --vault $Vault --out $ReviewOut --package $ExportDir
    Write-Host ""
    Write-Host "--- Generated Review Sheet ---"
    Get-Content -Path $ReviewOut -Encoding UTF8
} finally {
    if (Test-Path $Work) { Remove-Item -Recurse -Force $Work }
}
'@
[System.IO.File]::WriteAllText((Join-Path $StageDir "run-demo.ps1"), ($RunDemoScript + "`n"), $Utf8NoBom)

Write-Host "==> 3/4 Creating portable zip archive: $ZipPath"
if (Test-Path $ZipPath) { Remove-Item -Force $ZipPath }
Compress-Archive -Path $StageDir -DestinationPath $ZipPath -CompressionLevel Optimal

Write-Host "==> 4/4 Writing SHA256SUMS-windows.txt..."
$Hash = (Get-FileHash -Path $ZipPath -Algorithm SHA256).Hash.ToLowerInvariant()
"$Hash  $BundleName.zip" | Set-Content -Path $SumsPath -Encoding ASCII

Write-Host ""
Write-Host "Windows release artifact ready:"
Get-Item $ZipPath, $SumsPath | Format-Table Name, Length -AutoSize
Get-Content $SumsPath
