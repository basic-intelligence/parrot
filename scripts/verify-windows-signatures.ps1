param(
  [string[]]$Path
)

$ErrorActionPreference = "Stop"

$RootDir = Resolve-Path (Join-Path $PSScriptRoot "..")

if (-not $Path -or $Path.Count -eq 0) {
  $Candidates = @()
  $Sidecars = @(
    (Join-Path $RootDir "src-tauri/binaries/parrot-core-cpu-x86_64-pc-windows-msvc.exe"),
    (Join-Path $RootDir "src-tauri/binaries/parrot-core-cuda-x86_64-pc-windows-msvc.exe"),
    (Join-Path $RootDir "src-tauri/binaries/parrot-whisper-cpu-x86_64-pc-windows-msvc.exe"),
    (Join-Path $RootDir "src-tauri/binaries/parrot-whisper-cuda-x86_64-pc-windows-msvc.exe")
  )
  foreach ($Sidecar in $Sidecars) {
    if (Test-Path $Sidecar) {
      $Candidates += $Sidecar
    }
  }

  $TargetDir = Join-Path $RootDir "src-tauri/target"
  if (Test-Path $TargetDir) {
    $Candidates += Get-ChildItem -Path $TargetDir -Filter "*.exe" -File -Recurse |
      Where-Object {
        $_.FullName -match "\\release\\" -or $_.FullName -match "\\bundle\\nsis\\"
      } |
      Select-Object -ExpandProperty FullName
  }

  $Path = $Candidates | Sort-Object -Unique
}

if (-not $Path -or $Path.Count -eq 0) {
  throw "No Windows executables were found for Authenticode verification."
}

$Failures = @()
foreach ($Item in $Path) {
  if (-not (Test-Path $Item)) {
    $Failures += "$Item is missing"
    continue
  }

  $Signature = Get-AuthenticodeSignature -FilePath $Item
  if ($Signature.Status -ne "Valid") {
    $Failures += "$Item signature status: $($Signature.Status)"
    continue
  }

  Write-Host "Valid Authenticode signature: $Item"
}

if ($Failures.Count -gt 0) {
  $Failures | ForEach-Object { Write-Error $_ }
  throw "One or more Windows artifacts failed signature verification."
}
