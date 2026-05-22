$ErrorActionPreference = "Stop"

if (-not $env:WINDOWS_CERTIFICATE) {
  throw "WINDOWS_CERTIFICATE is required. Store the base64-encoded PFX in CI secrets."
}

if (-not $env:WINDOWS_CERTIFICATE_PASSWORD) {
  throw "WINDOWS_CERTIFICATE_PASSWORD is required. Store the PFX password in CI secrets."
}

$CertificateDir = Join-Path $env:RUNNER_TEMP "parrot-windows-certificate"
$EncodedCertificatePath = Join-Path $CertificateDir "certificate.base64"
$PfxPath = Join-Path $CertificateDir "certificate.pfx"

New-Item -ItemType Directory -Force -Path $CertificateDir | Out-Null
Set-Content -Path $EncodedCertificatePath -Value $env:WINDOWS_CERTIFICATE -NoNewline

certutil -decode $EncodedCertificatePath $PfxPath | Out-Null
Remove-Item -Force $EncodedCertificatePath

$SecurePassword = ConvertTo-SecureString -String $env:WINDOWS_CERTIFICATE_PASSWORD -Force -AsPlainText
$ImportedCertificate = Import-PfxCertificate `
  -FilePath $PfxPath `
  -CertStoreLocation Cert:\CurrentUser\My `
  -Password $SecurePassword

if (-not $ImportedCertificate -or -not $ImportedCertificate.Thumbprint) {
  throw "Windows signing certificate import did not return a certificate thumbprint."
}

$Thumbprint = $ImportedCertificate.Thumbprint.Replace(" ", "").ToUpperInvariant()
Write-Host "Imported Windows signing certificate: $Thumbprint"

if ($env:GITHUB_ENV) {
  Add-Content -Path $env:GITHUB_ENV -Value "WINDOWS_CERTIFICATE_THUMBPRINT=$Thumbprint"
}

