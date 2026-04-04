param(
    [string]$ApiBase = "http://127.0.0.1:8787",
    [string]$ProviderName = "cloudflare-r2",
    [string]$Passphrase,
    [switch]$SkipProviderSetup
)

$ErrorActionPreference = "Stop"

function Invoke-OmniApi {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [string]$Method = "GET",
        [object]$Body = $null
    )

    $uri = "$ApiBase$Path"
    if ($null -ne $Body) {
        $json = $Body | ConvertTo-Json -Depth 16
        return Invoke-RestMethod -Method $Method -Uri $uri -ContentType "application/json" -Body $json
    }

    return Invoke-RestMethod -Method $Method -Uri $uri
}

function Read-PlainSecret {
    param([string]$Prompt)

    $secure = Read-Host -Prompt $Prompt -AsSecureString
    $bstr = [Runtime.InteropServices.Marshal]::SecureStringToBSTR($secure)
    try {
        return [Runtime.InteropServices.Marshal]::PtrToStringBSTR($bstr)
    }
    finally {
        [Runtime.InteropServices.Marshal]::ZeroFreeBSTR($bstr)
    }
}

Write-Host "[B8/Dell] Loading onboarding status from $ApiBase ..."
$status = Invoke-OmniApi -Path "/api/onboarding/status"
$provider = $status.providers | Where-Object { $_.provider_name -eq $ProviderName } | Select-Object -First 1

if ($null -eq $provider) {
    throw "[B8/Dell] Provider '$ProviderName' is not present in onboarding status."
}

if (-not $SkipProviderSetup) {
    if ([string]::IsNullOrWhiteSpace($provider.endpoint) -or [string]::IsNullOrWhiteSpace($provider.region) -or [string]::IsNullOrWhiteSpace($provider.bucket)) {
        throw "[B8/Dell] Provider '$ProviderName' does not expose endpoint/region/bucket in onboarding status."
    }

    Write-Host "[B8/Dell] Testing and enabling provider '$ProviderName' ..."
    $setupPayload = @{
        provider_name     = $ProviderName
        endpoint          = $provider.endpoint
        region            = $provider.region
        bucket            = $provider.bucket
        force_path_style  = [bool]$provider.force_path_style
        enabled           = $true
    }
    $setupResponse = Invoke-OmniApi -Path "/api/onboarding/setup-provider" -Method "POST" -Body $setupPayload
}
else {
    $setupResponse = $null
}

if ([string]::IsNullOrWhiteSpace($Passphrase)) {
    $Passphrase = Read-PlainSecret -Prompt "Vault passphrase for join-existing"
}

if ([string]::IsNullOrWhiteSpace($Passphrase)) {
    throw "[B8/Dell] Passphrase is required."
}

Write-Host "[B8/Dell] Joining existing vault with provider '$ProviderName' ..."
$joinPayload = @{
    provider_id = $ProviderName
    passphrase  = $Passphrase
}
$joinResponse = Invoke-OmniApi -Path "/api/onboarding/join-existing" -Method "POST" -Body $joinPayload
$Passphrase = $null

$finalOnboarding = Invoke-OmniApi -Path "/api/onboarding/status"
$diagnostics = Invoke-OmniApi -Path "/api/maintenance/diagnostics"
$multidevice = Invoke-OmniApi -Path "/api/multidevice/status"
$storage = Invoke-OmniApi -Path "/api/storage/cost"

$report = [pscustomobject]@{
    generated_at = (Get-Date).ToString("o")
    machine_role = "dell-secondary"
    api_base = $ApiBase
    setup_provider = $setupResponse
    join_existing = $joinResponse
    onboarding = $finalOnboarding
    diagnostics = $diagnostics
    multidevice = $multidevice
    storage = $storage
}

$repoRoot = Split-Path -Parent $PSScriptRoot
$reportDir = Join-Path $repoRoot ".omnidrive"
New-Item -ItemType Directory -Force -Path $reportDir | Out-Null
$reportPath = Join-Path $reportDir ("b8-dell-report-{0}.json" -f (Get-Date -Format "yyyyMMdd-HHmmss"))
$report | ConvertTo-Json -Depth 20 | Set-Content -Path $reportPath -Encoding utf8

Write-Host "[B8/Dell] Report saved: $reportPath"
$report | ConvertTo-Json -Depth 8
