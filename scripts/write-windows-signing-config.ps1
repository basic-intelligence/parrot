$ErrorActionPreference = "Stop"

$RootDir = Resolve-Path (Join-Path $PSScriptRoot "..")
$OutputPath = Join-Path $RootDir "src-tauri/tauri.windows.signing.generated.json"

if (-not $env:WINDOWS_CERTIFICATE_THUMBPRINT) {
  throw "WINDOWS_CERTIFICATE_THUMBPRINT is required. Run scripts/import-windows-signing-certificate.ps1 first."
}

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

$Config = [ordered]@{
  '$schema' = 'https://schema.tauri.app/config/2'
  bundle = [ordered]@{
    externalBin = @(
      "binaries/parrot-core-cpu",
      "binaries/parrot-core-cuda",
      "binaries/parrot-whisper-cpu",
      "binaries/parrot-whisper-cuda"
    )
    windows = [ordered]@{
      certificateThumbprint = $env:WINDOWS_CERTIFICATE_THUMBPRINT
      digestAlgorithm = $DigestAlgorithm
      timestampUrl = $TimestampUrl
      tsp = $true
    }
  }
}

$Config | ConvertTo-Json -Depth 8 | Set-Content -Path $OutputPath -Encoding UTF8
Write-Host "Wrote Windows signing config overlay: $OutputPath"

if ($env:GITHUB_ENV) {
  Add-Content -Path $env:GITHUB_ENV -Value "TAURI_WINDOWS_SIGNING_CONFIG=$OutputPath"
}
