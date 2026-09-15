# Build a Windows zip: Folio.exe plus the license files. Called from
# .github/workflows/windows-release.yml on a version tag; also runnable
# on a Windows machine. Does not sign, and does not upload.
$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$root = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
Set-Location $root

function Read-ReleaseVar([string] $name) {
    $line = Get-Content -LiteralPath (Join-Path $root "packaging/release.env") |
        Where-Object { $_ -match "^$name=" } |
        Select-Object -First 1
    if (-not $line) {
        throw "packaging/release.env is missing $name"
    }
    return $line.Substring($name.Length + 1)
}

$version = Read-ReleaseVar "VERSION"
$build = Read-ReleaseVar "BUILD"
$bucket = Read-ReleaseVar "R2_BUCKET"
$account = Read-ReleaseVar "CLOUDFLARE_ACCOUNT_ID"
$cargoVersion = (Select-String -LiteralPath (Join-Path $root "Cargo.toml") -Pattern '^version = "([^"]+)"').Matches[0].Groups[1].Value
if ($cargoVersion -ne $version) {
    throw "Cargo.toml is $cargoVersion, packaging/release.env is $version"
}

cargo build --locked --release --target-dir target
if ($LASTEXITCODE -ne 0) {
    throw "cargo build failed"
}

$exe = Join-Path $root "target/release/folio.exe"
if (-not (Test-Path -LiteralPath $exe)) {
    throw "Missing $exe after cargo build"
}

$stage = Join-Path $root "target/windows-stage"
$artifacts = Join-Path $root "target/release-artifacts"
Remove-Item -LiteralPath $stage -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Path $stage | Out-Null
New-Item -ItemType Directory -Path $artifacts -Force | Out-Null
Copy-Item -LiteralPath $exe -Destination (Join-Path $stage "Folio.exe")
Copy-Item -LiteralPath (Join-Path $root "LICENSE") -Destination $stage
Copy-Item -LiteralPath (Join-Path $root "NOTICE") -Destination $stage

$zipName = "Folio-$version-$build-windows-x64.zip"
$zipPath = Join-Path $artifacts $zipName
Remove-Item -LiteralPath $zipPath -Force -ErrorAction SilentlyContinue
Compress-Archive -Path (Join-Path $stage "*") -DestinationPath $zipPath -CompressionLevel Optimal

Write-Host "Windows zip $zipPath"

if ($env:GITHUB_ENV) {
    $unixZip = $zipPath.Replace('\', '/')
    $lines = @(
        "VERSION=$version"
        "BUILD=$build"
        "R2_BUCKET=$bucket"
        "CLOUDFLARE_ACCOUNT_ID=$account"
        "ZIP_KEY=$zipName"
        "ZIP_PATH=$unixZip"
        "LATEST_KEY=Folio-latest-windows.zip"
    )
    Add-Content -LiteralPath $env:GITHUB_ENV -Value $lines -Encoding utf8NoBOM
}
