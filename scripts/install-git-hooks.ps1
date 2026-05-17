# OmniDrive — install-git-hooks.ps1
#
# Konfiguruje git żeby używał hooków z `.githooks/` (zamiast `.git/hooks/`).
# Dzięki temu hooki są wersjonowane razem z repo i każdy clone dostaje je
# po jednym poleceniu.
#
# Uruchom JEDEN RAZ na clone:
#     pwsh scripts/install-git-hooks.ps1
#
# Hooks (po instalacji aktywne):
#   .githooks/pre-push  — wymusza `cargo fmt --check` + `cargo clippy -D warnings`
#                          przed każdym `git push`. Bypass: `git push --no-verify`.

$ErrorActionPreference = "Stop"

# Upewnij się że stoimy w drzewie git
git rev-parse --is-inside-work-tree | Out-Null

# Ustaw hooksPath na nasz folder .githooks/
git config core.hooksPath .githooks

Write-Host "[install-git-hooks] OK core.hooksPath = .githooks" -ForegroundColor Green
Write-Host ""
Write-Host "Aktywne hooki:" -ForegroundColor Cyan
Get-ChildItem .githooks -File | ForEach-Object {
    Write-Host "  - $($_.Name)"
}
Write-Host ""
Write-Host "Test: zrob `git push --dry-run` aby zobaczyc pre-push w akcji." -ForegroundColor Yellow
Write-Host "Bypass (awaryjnie): `git push --no-verify`" -ForegroundColor Yellow
