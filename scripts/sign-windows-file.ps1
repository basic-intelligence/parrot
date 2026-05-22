param(
  [Parameter(Mandatory = $true)]
  [string]$Path
)

$ErrorActionPreference = "Stop"

if (-not (Test-Path $Path)) {
  throw "Cannot sign missing file: $Path"
}

if (-not $env:WINDOWS_CERTIFICATE_THUMBPRINT) {
  throw "WINDOWS_CERTIFICATE_THUMBPRINT is required for Windows code signing."
}

function Find-SignTool {
  if ($env:TAURI_WINDOWS_SIGNTOOL_PATH -and (Test-Path $env:TAURI_WINDOWS_SIGNTOOL_PATH)) {
    return $env:TAURI_WINDOWS_SIGNTOOL_PATH
  }

  $Command = Get-Command signtool.exe -ErrorAction SilentlyContinue
  if ($Command) {
    return $Command.Source
  }

  $KitsRoot = Join-Path ${env:ProgramFiles(x86)} "Windows Kits/10/bin"
  if (Test-Path $KitsRoot) {
    $Candidate = Get-ChildItem -Path $KitsRoot -Filter signtool.exe -Recurse -File |
      Where-Object { $_.FullName -match "\\x64\\signtool\.exe$" } |
      Sort-Object FullName -Descending |
      Select-Object -First 1
    if ($Candidate) {
      return $Candidate.FullName
    }
  }

  throw "signtool.exe was not found. Install Windows SDK or set TAURI_WINDOWS_SIGNTOOL_PATH."
}

$SignTool = Find-SignTool
$DigestAlgorithm = if ($env:WINDOWS_SIGNING_DIGEST_ALGORITHM) {
  $env:WINDOWS_SIGNING_DIGEST_ALGORITHM
} else {
  "sha256"
}
$TimestampUrl = if ($env:WINDOWS_SIGNING_TIMESTAMP_URL) {
  $env:WINDOWS_SIGNING_TIMESTAMP_URL
} else {
  "http://timestamp.digicert.com"
}
$Thumbprint = $env:WINDOWS_CERTIFICATE_THUMBPRINT.Replace(" ", "")

Write-Host "Signing $Path"
& $SignTool sign `
  /sha1 $Thumbprint `
  /fd $DigestAlgorithm `
  /tr $TimestampUrl `
  /td $DigestAlgorithm `
  /v `
  $Path

if ($LASTEXITCODE -ne 0) {
  throw "signtool failed for $Path"
}

$Signature = Get-AuthenticodeSignature -FilePath $Path
if ($Signature.Status -ne "Valid") {
  throw "Authenticode signature is $($Signature.Status) for $Path"
}
