Set shell = CreateObject("WScript.Shell")
Set fso = CreateObject("Scripting.FileSystemObject")
installDir = fso.GetParentFolderName(WScript.ScriptFullName)

' Start the daemon (angeld.exe) — hidden window, no wait
daemonPath = fso.BuildPath(installDir, "angeld.exe")
If fso.FileExists(daemonPath) Then
    shell.Run """" & daemonPath & """", 0, False
End If

' Start the tray companion (omnidrive-tray.exe) — hidden window, no wait
trayPath = fso.BuildPath(installDir, "omnidrive-tray.exe")
If fso.FileExists(trayPath) Then
    shell.Run """" & trayPath & """", 0, False
End If
