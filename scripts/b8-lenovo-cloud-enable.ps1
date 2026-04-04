param(
    [string]$ApiBase = "http://127.0.0.1:8787",
    [string[]]$Providers = @("cloudflare-r2", "backblaze-b2", "scaleway"),
    [switch]$SkipComplete
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

Write-Host "[B8/Lenovo] Loading onboarding status from $ApiBase ..."
$status = Invoke-OmniApi -Path "/api/onboarding/status"

$providerMap = @{}
foreach ($provider in $status.providers) {
    $providerMap[$provider.provider_name] = $provider
}

$setupResults = @()
foreach ($providerName in $Providers) {
    if (-not $providerMap.ContainsKey($providerName)) {
        Write-Warning "[B8/Lenovo] Provider '$providerName' is not present in onboarding status. Skipping."
        continue
    }

    $draft = $providerMap[$providerName]
    if ([string]::IsNullOrWhiteSpace($draft.endpoint) -or [string]::IsNullOrWhiteSpace($draft.region) -or [string]::IsNullOrWhiteSpace($draft.bucket)) {
        Write-Warning "[B8/Lenovo] Provider '$providerName' has missing endpoint/region/bucket. Skipping."
        continue
    }

    Write-Host "[B8/Lenovo] Testing and enabling provider '$providerName' ..."
    $payload = @{
        provider_name     = $providerName
        endpoint          = $draft.endpoint
        region            = $draft.region
        bucket            = $draft.bucket
        force_path_style  = [bool]$draft.force_path_style
        enabled           = $true
    }

    $response = Invoke-OmniApi -Path "/api/onboarding/setup-provider" -Method "POST" -Body $payload
    $setupResults += [pscustomobject]@{
        provider_name      = $providerName
        enabled            = $response.enabled
        validation_status  = $response.validation.status
        validation_message = $response.validation.message
        error_kind         = $response.validation.error_kind
    }
}

if (-not $SkipComplete) {
    Write-Host "[B8/Lenovo] Completing onboarding ..."
    $null = Invoke-OmniApi -Path "/api/onboarding/complete" -Method "POST"
}

$finalOnboarding = Invoke-OmniApi -Path "/api/onboarding/status"
$diagnostics = Invoke-OmniApi -Path "/api/maintenance/diagnostics"
$multidevice = Invoke-OmniApi -Path "/api/multidevice/status"
$storage = Invoke-OmniApi -Path "/api/storage/cost"

$report = [pscustomobject]@{
    generated_at = (Get-Date).ToString("o")
    machine_role = "lenovo-primary"
    api_base = $ApiBase
    setup_results = $setupResults
    onboarding = $finalOnboarding
    diagnostics = $diagnostics
    multidevice = $multidevice
    storage = $storage
}

$repoRoot = Split-Path -Parent $PSScriptRoot
$reportDir = Join-Path $repoRoot ".omnidrive"
New-Item -ItemType Directory -Force -Path $reportDir | Out-Null
$reportPath = Join-Path $reportDir ("b8-lenovo-report-{0}.json" -f (Get-Date -Format "yyyyMMdd-HHmmss"))
$report | ConvertTo-Json -Depth 20 | Set-Content -Path $reportPath -Encoding utf8

Write-Host "[B8/Lenovo] Report saved: $reportPath"
$report | ConvertTo-Json -Depth 8
