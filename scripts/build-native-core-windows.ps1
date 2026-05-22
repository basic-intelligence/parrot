$ErrorActionPreference = "Stop"

$RootDir = Resolve-Path (Join-Path $PSScriptRoot "..")
$TargetTriple = "x86_64-pc-windows-msvc"
$ManifestPath = Join-Path $RootDir "native-core/windows/Cargo.toml"
$BinaryName = "parrot-core.exe"
$CargoProfile = "native-core-release"
$BuiltBinary = Join-Path $RootDir "target/$TargetTriple/$CargoProfile/$BinaryName"
$OutputDir = Join-Path $RootDir "src-tauri/binaries"

function Get-RustHostTriple {
  $VersionInfo = & rustc -vV
  $HostLine = $VersionInfo | Where-Object { $_ -like "host:*" } | Select-Object -First 1
  if (-not $HostLine) {
    throw "Could not determine rustc host target triple."
  }
  return ($HostLine -replace "^host:\s*", "").Trim()
}

function Get-PeMachine {
  param([Parameter(Mandatory = $true)][string]$Path)

  $Stream = [System.IO.File]::OpenRead($Path)
  try {
    $Reader = [System.IO.BinaryReader]::new($Stream)
    $Stream.Seek(0x3c, [System.IO.SeekOrigin]::Begin) | Out-Null
    $PeOffset = $Reader.ReadInt32()
    $Stream.Seek($PeOffset, [System.IO.SeekOrigin]::Begin) | Out-Null
    $Signature = $Reader.ReadUInt32()
    if ($Signature -ne 0x00004550) {
      throw "$Path is not a valid PE executable."
    }
    return $Reader.ReadUInt16()
  } finally {
    if ($Reader) {
      $Reader.Dispose()
    } else {
      $Stream.Dispose()
    }
  }
}

function Set-LlvmNmPath {
  if ($env:NM_PATH -and (Test-Path $env:NM_PATH)) {
    return
  }

  $Candidates = @(
    (Join-Path $env:ProgramFiles "LLVM/bin/llvm-nm.exe"),
    (Join-Path ${env:ProgramFiles(x86)} "LLVM/bin/llvm-nm.exe")
  )

  $LlvmNm = $Candidates | Where-Object { $_ -and (Test-Path $_) } | Select-Object -First 1
  if ($LlvmNm) {
    $env:NM_PATH = $LlvmNm
    $LlvmBin = Split-Path -Parent $LlvmNm
    if (-not (($env:PATH -split ';') -contains $LlvmBin)) {
      $env:PATH = "$LlvmBin;$env:PATH"
    }
    Write-Host "Using llvm-nm: $LlvmNm"
  }
}

$HostTriple = Get-RustHostTriple
Set-LlvmNmPath
Write-Host "Rust host target: $HostTriple"
Write-Host "Using Cargo profile: $CargoProfile"

function Get-RequestedVariants {
  $RawVariants = if ($env:PARROT_WINDOWS_CORE_VARIANTS) {
    $env:PARROT_WINDOWS_CORE_VARIANTS
  } else {
    "cpu"
  }

  $Variants = $RawVariants -split "," |
    ForEach-Object { $_.Trim().ToLowerInvariant() } |
    Where-Object { $_ }

  if (-not $Variants -or $Variants.Count -eq 0) {
    throw "PARROT_WINDOWS_CORE_VARIANTS did not contain any variants."
  }

  foreach ($Variant in $Variants) {
    if ($Variant -notin @("cpu", "cuda")) {
      throw "Unknown Windows native core variant: $Variant. Expected cpu or cuda."
    }
  }

  return $Variants
}

function Invoke-CoreVariantBuild {
  param(
    [Parameter(Mandatory = $true)]
    [string]$Variant,
    [Parameter(Mandatory = $true)]
    [AllowEmptyCollection()]
    [System.Collections.Generic.List[string]]$OutputBinaries,
    [Parameter(Mandatory = $true)]
    [AllowEmptyCollection()]
    [System.Collections.Generic.HashSet[string]]$CopiedLibraries
  )

  $FeatureArgs = @("--no-default-features")
  if ($Variant -eq "cuda") {
    $FeatureArgs += @("--features", "cuda")

    $DefaultCudaPath = "C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v13.1"
    if (-not $env:CUDA_PATH -and (Test-Path $DefaultCudaPath)) {
      $env:CUDA_PATH = $DefaultCudaPath
    }
    if ($env:CUDA_PATH -and -not $env:CUDA_PATH_V13_1) {
      $env:CUDA_PATH_V13_1 = $env:CUDA_PATH
    }
    if ($env:CUDA_PATH -and (Test-Path (Join-Path $env:CUDA_PATH "bin"))) {
      $CudaBin = Join-Path $env:CUDA_PATH "bin"
      if (-not (($env:PATH -split ';') -contains $CudaBin)) {
        $env:PATH = "$CudaBin;$env:PATH"
      }
    }
  }

  Write-Host "Building Windows native core variant '$Variant' for $TargetTriple"

  & cargo build `
    --manifest-path $ManifestPath `
    --profile $CargoProfile `
    --target $TargetTriple `
    @FeatureArgs

  if ($LASTEXITCODE -ne 0) {
    throw "cargo build failed for native-core/windows variant '$Variant'."
  }

  if (-not (Test-Path $BuiltBinary)) {
    throw "Expected Windows sidecar binary was not found: $BuiltBinary"
  }

  $Machine = Get-PeMachine -Path $BuiltBinary
  if ($Machine -ne 0x8664) {
    throw ("Windows sidecar architecture is 0x{0:x}; expected 0x8664 for x86_64." -f $Machine)
  }

  New-Item -ItemType Directory -Force -Path $OutputDir | Out-Null

  $OutputBinary = Join-Path $OutputDir "parrot-core-$Variant-$TargetTriple.exe"
  Copy-Item -Force $BuiltBinary $OutputBinary
  $OutputBinaries.Add($OutputBinary)

  $OutputMachine = Get-PeMachine -Path $OutputBinary
  if ($OutputMachine -ne 0x8664) {
    throw ("Copied Windows sidecar architecture is 0x{0:x}; expected 0x8664 for x86_64." -f $OutputMachine)
  }

  $BuiltBinaryDir = Split-Path -Parent $BuiltBinary
  $DynamicLibraries = Get-ChildItem -Path $BuiltBinaryDir -Filter "*.dll" -File -ErrorAction SilentlyContinue
  foreach ($Library in $DynamicLibraries) {
    $Destination = Join-Path $OutputDir $Library.Name
    Copy-Item -Force $Library.FullName $Destination
    [void]$CopiedLibraries.Add($Destination)
  }

  Write-Host "Installed Windows sidecar variant '$Variant': $OutputBinary"
}

$OutputBinaries = [System.Collections.Generic.List[string]]::new()
$CopiedLibraries = [System.Collections.Generic.HashSet[string]]::new()

foreach ($Variant in Get-RequestedVariants) {
  Invoke-CoreVariantBuild `
    -Variant $Variant `
    -OutputBinaries $OutputBinaries `
    -CopiedLibraries $CopiedLibraries
}

if ($env:WINDOWS_CERTIFICATE_THUMBPRINT) {
  $SignScript = Join-Path $RootDir "scripts/sign-windows-file.ps1"

  foreach ($Binary in $OutputBinaries) {
    & $SignScript -Path $Binary
  }

  foreach ($Library in $CopiedLibraries) {
    & $SignScript -Path $Library
  }
}

foreach ($Binary in $OutputBinaries) {
  if (-not (Test-Path $Binary)) {
    throw "Failed to create Tauri sidecar binary: $Binary"
  }
}
