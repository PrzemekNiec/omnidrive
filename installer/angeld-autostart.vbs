Set shell = CreateObject("WScript.Shell")
Set fso = CreateObject("Scripting.FileSystemObject")
installDir = fso.GetParentFolderName(WScript.ScriptFullName)
daemonPath = fso.BuildPath(installDir, "angeld.exe")
shell.Run """" & daemonPath & """", 0, False
