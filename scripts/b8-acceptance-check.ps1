param(
    [string]$ApiBase = "http://127.0.0.1:8787",
    [int]$MinEnabledProviders = 1,
    [switch]$RequireCloudMode
)

$ErrorActionPreference = "Stop"

function Invoke-OmniApi {
    param([Parameter(Mandatory = $true)][string]$Path)
    return Invoke-RestMethod -Method "GET" -Uri "$ApiBase$Path"
}

$onboarding = Invoke-OmniApi -Path "/api/onboarding/status"
$diagnostics = Invoke-OmniApi -Path "/api/maintenance/diagnostics"
$multidevice = Invoke-OmniApi -Path "/api/multidevice/status"
$storage = Invoke-OmniApi -Path "/api/storage/cost"

$enabledProviders = @($onboarding.providers | Where-Object { $_.enabled -eq $true })
$enabledNames = @($enabledProviders | ForEach-Object { $_.provider_name })
$enabledWithFailedTest = @(
    $enabledProviders | Where-Object {
        $_.last_test_status -and $_.last_test_status -ne "OK"
    }
)

$checks = @()
$checks += [pscustomobject]@{
    name = "onboarding_completed"
    passed = ($onboarding.onboarding_state -eq "COMPLETED")
    details = "state=$($onboarding.onboarding_state)"
}
$checks += [pscustomobject]@{
    name = "min_enabled_providers"
    passed = ($enabledProviders.Count -ge $MinEnabledProviders)
    details = "enabled=$($enabledProviders.Count), required=$MinEnabledProviders, names=$($enabledNames -join ',')"
}
$checks += [pscustomobject]@{
    name = "enabled_provider_tests_ok"
    passed = ($enabledWithFailedTest.Count -eq 0)
    details = if ($enabledWithFailedTest.Count -eq 0) { "all enabled providers have OK or empty test status" } else { "failed=$((@($enabledWithFailedTest | ForEach-Object { $_.provider_name + ':' + $_.last_test_status })) -join ',')" }
}
$checks += [pscustomobject]@{
    name = "maintenance_status_ok"
    passed = ($diagnostics.status -eq "OK")
    details = "maintenance=$($diagnostics.status)"
}
$checks += [pscustomobject]@{
    name = "shell_state_healthy"
    passed = ($diagnostics.shell.status -eq "OK")
    details = "shell=$($diagnostics.shell.status), drive=$($diagnostics.shell.preferred_drive_letter)"
}
$checks += [pscustomobject]@{
    name = "sync_root_healthy"
    passed = ($diagnostics.sync_root.status -eq "OK")
    details = "sync_root=$($diagnostics.sync_root.status)"
}

if ($RequireCloudMode) {
    $checks += [pscustomobject]@{
        name = "onboarding_cloud_mode"
        passed = ($onboarding.onboarding_mode -eq "CLOUD_ENABLED")
        details = "mode=$($onboarding.onboarding_mode)"
    }
}

$failed = @($checks | Where-Object { -not $_.passed })
$summary = [pscustomobject]@{
    generated_at = (Get-Date).ToString("o")
    api_base = $ApiBase
    checks = $checks
    failed_count = $failed.Count
    onboarding = $onboarding
    diagnostics = $diagnostics
    multidevice = $multidevice
    storage = $storage
}

$repoRoot = Split-Path -Parent $PSScriptRoot
$reportDir = Join-Path $repoRoot ".omnidrive"
New-Item -ItemType Directory -Force -Path $reportDir | Out-Null
$reportPath = Join-Path $reportDir ("b8-acceptance-{0}.json" -f (Get-Date -Format "yyyyMMdd-HHmmss"))
$summary | ConvertTo-Json -Depth 20 | Set-Content -Path $reportPath -Encoding utf8

if ($failed.Count -gt 0) {
    Write-Error "[B8] Acceptance FAILED. Report: $reportPath"
    $summary | ConvertTo-Json -Depth 8
    exit 1
}

Write-Host "[B8] Acceptance PASSED. Report: $reportPath"
$summary | ConvertTo-Json -Depth 8
