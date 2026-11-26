$ErrorActionPreference = "Stop"
$RepoRoot = Split-Path -Parent $PSScriptRoot
$DestDir = Join-Path $env:LOCALAPPDATA "Earnest\bin"
New-Item -ItemType Directory -Force -Path $DestDir | Out-Null

Write-Host "Building earnest_gov_mcp (release)..."
cargo build --release --manifest-path (Join-Path $RepoRoot "earnest-ide\core\Cargo.toml") --bin earnest_gov_mcp

Write-Host "Building earnest-bcpe kb (release)..."
cargo build --release --manifest-path (Join-Path $RepoRoot "earnest-bcpe\Cargo.toml")

Copy-Item -Force (Join-Path $RepoRoot "earnest-ide\core\target\release\earnest_gov_mcp.exe") $DestDir
Copy-Item -Force (Join-Path $RepoRoot "earnest-bcpe\target\release\kb.exe") $DestDir

$KbDb = Join-Path $RepoRoot "earnest-bcpe\kb.db"
if (Test-Path $KbDb) {
    Copy-Item -Force $KbDb $DestDir
}

Write-Host "Installed to $DestDir"
Write-Host "Add $DestDir to your PATH, then copy mcp.example.json -> .mcp.json in your project."
