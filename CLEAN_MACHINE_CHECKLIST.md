# OmniDrive Clean-Machine Checklist

## Scope
- Validate that OmniDrive can be installed and started on a clean Windows laptop without terminal setup.
- Confirm that the installed daemon bootstraps its runtime, registers Smart Sync, and exposes the virtual drive.

## Test Artifact
- Installer:
  - `dist\installer\output\OmniDrive-Setup-0.1.0.exe`

## 1. Environment Preparation
- Log in to a fresh Windows user profile.
- Confirm there is no previous OmniDrive state in:
  - `%LOCALAPPDATA%\OmniDrive`
  - `%LOCALAPPDATA%\Programs\OmniDrive`
- Confirm there is no leftover autostart entry:
  - `HKCU\Software\Microsoft\Windows\CurrentVersion\Run\OmniDriveAngeld`

## 2. Installation
- Run `OmniDrive-Setup-0.1.0.exe`.
- Optionally enable adding OmniDrive to the user `PATH`.
- Complete the installation wizard.

## 3. Installed Files Validation
- Verify the installation directory exists:
  - `%LOCALAPPDATA%\Programs\OmniDrive`
- Verify installed files:
  - `angeld.exe`
  - `omnidrive.exe`
  - `angeld-autostart.vbs`

## 4. Runtime Bootstrap Validation
- Verify the runtime base directory exists:
  - `%LOCALAPPDATA%\OmniDrive`
- Verify subdirectories exist:
  - `%LOCALAPPDATA%\OmniDrive\Cache`
  - `%LOCALAPPDATA%\OmniDrive\Spool`
  - `%LOCALAPPDATA%\OmniDrive\download-spool`
  - `%LOCALAPPDATA%\OmniDrive\logs`
  - `%LOCALAPPDATA%\OmniDrive\SyncRoot`

## 5. Autostart Validation
- Open Registry Editor or use a shell command to inspect:
  - `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`
- Verify the `OmniDriveAngeld` value exists.
- Verify it points to:
  - `wscript.exe //B ...\angeld-autostart.vbs`

## 6. PATH Validation
- Open a new `cmd.exe` or PowerShell session.
- Run:
  - `omnidrive --help`
- Expected:
  - the CLI runs successfully from the user `PATH`

## 7. Daemon First-Start Validation
- Verify the daemon is running automatically after installation, or launch it manually if needed.
- Check log output in:
  - `%LOCALAPPDATA%\OmniDrive\logs`
- Expected log signals:
  - runtime bootstrap completed
  - default local vault initialized
  - smart sync bootstrap ready
  - virtual drive mounted

## 8. Diagnostics API Validation
- Open:
  - `http://127.0.0.1:8787/api/diagnostics/health`
- Expected:
  - valid JSON response
  - no critical startup failures

## 9. Explorer and Drive Validation
- Open Windows Explorer.
- Verify OmniDrive appears as:
  - `O:\`
  - or the first available fallback letter if `O:` was already occupied
- Verify:
  - drive label
  - drive icon
  - drive opens normally

## 10. SyncRoot Validation
- Inspect:
  - `%LOCALAPPDATA%\OmniDrive\SyncRoot`
- Expected:
  - directory exists
  - Smart Sync root is active and ready for placeholder projection

## 11. Re-Login Validation
- Sign out and sign back in.
- Verify:
  - daemon starts again without a visible console window
  - diagnostics API responds
  - the OmniDrive virtual drive is still present

## 12. Uninstall Validation
- Uninstall OmniDrive.
- Verify removal of:
  - `%LOCALAPPDATA%\Programs\OmniDrive`
  - autostart entry in `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`
  - OmniDrive entry from the user `PATH`
- Decide whether `%LOCALAPPDATA%\OmniDrive` should remain for data preservation or be removed explicitly.

## Pass Criteria
- Installer completes successfully.
- Daemon starts without manual terminal setup.
- Runtime directories are created automatically.
- Diagnostics API responds.
- Smart Sync initializes correctly.
- Virtual drive appears in Explorer.
- Autostart works after sign-out / sign-in.
- No persistent console window appears during autostart.
- Uninstall cleans user-level shell and autostart state.
