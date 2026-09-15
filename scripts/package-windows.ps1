# Build a Windows installer exe. Called from
# .github/workflows/windows-release.yml on a version tag; also runnable
# on a Windows machine with Inno Setup 6. Does not sign, and does not upload.
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
$ico = Join-Path $root "target/folio.ico"
Remove-Item -LiteralPath $stage -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Path $stage | Out-Null
New-Item -ItemType Directory -Path $artifacts -Force | Out-Null
Copy-Item -LiteralPath $exe -Destination (Join-Path $stage "Folio.exe")
Copy-Item -LiteralPath (Join-Path $root "LICENSE") -Destination $stage
Copy-Item -LiteralPath (Join-Path $root "NOTICE") -Destination $stage

$magick = Get-Command magick -ErrorAction SilentlyContinue
if ($magick) {
    & magick (Join-Path $root "assets/app-icon/logo-1024.png") -define icon:auto-resize=256,128,64,48,32,16 $ico
    if ($LASTEXITCODE -ne 0) {
        throw "magick failed to write $ico"
    }
} else {
    throw "ImageMagick (magick) is required to embed the installer icon"
}

$iscc = @(
    (Join-Path ${env:ProgramFiles(x86)} "Inno Setup 6\ISCC.exe"),
    (Join-Path $env:ProgramFiles "Inno Setup 6\ISCC.exe")
) | Where-Object { Test-Path -LiteralPath $_ } | Select-Object -First 1
if (-not $iscc) {
    throw "Inno Setup 6 is not installed (ISCC.exe)"
}

& $iscc `
    "/DMyAppVersion=$version" `
    "/DMyAppBuild=$build" `
    "/DFolioRoot=$($root.Replace('\', '/'))" `
    (Join-Path $root "packaging/windows/folio.iss")
if ($LASTEXITCODE -ne 0) {
    throw "ISCC failed"
}

$publicBase = Read-ReleaseVar "PUBLIC_BASE"
$exeName = "Folio-$version-$build-windows-x64.exe"
$exePath = Join-Path $artifacts $exeName
if (-not (Test-Path -LiteralPath $exePath)) {
    throw "Missing installer $exePath"
}

$sha256 = (Get-FileHash -LiteralPath $exePath -Algorithm SHA256).Hash.ToLowerInvariant()
$feedPath = Join-Path $artifacts "windows-appcast.json"
$feed = @{
    version = $version
    build   = [int]$build
    url     = "$publicBase/$exeName"
    sha256  = $sha256
} | ConvertTo-Json -Compress
[System.IO.File]::WriteAllText($feedPath, $feed)

Write-Host "Windows installer $exePath"

if ($env:GITHUB_ENV) {
    $unixExe = $exePath.Replace('\', '/')
    $unixFeed = $feedPath.Replace('\', '/')
    $lines = @(
        "VERSION=$version"
        "BUILD=$build"
        "R2_BUCKET=$bucket"
        "CLOUDFLARE_ACCOUNT_ID=$account"
        "EXE_KEY=$exeName"
        "EXE_PATH=$unixExe"
        "LATEST_KEY=Folio-latest-windows.exe"
        "FEED_KEY=windows-appcast.json"
        "FEED_PATH=$unixFeed"
    )
    Add-Content -LiteralPath $env:GITHUB_ENV -Value $lines -Encoding utf8NoBOM
}
