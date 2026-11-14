; Hooks of the NSIS installer (tauri.conf.json `bundle.windows.nsis.installerHooks`).

; Before the uninstaller removes anything: the machine self-test's Hyper-V VM, if one was left
; running (the test interrupted). It runs from a disk in the app's cache folder, which the
; uninstaller may delete with the app's data; left running, it would hold that disk and stay,
; with nothing left to remove it. Only a VM named exactly "CyberCTF test VM" whose every disk is
; in that folder is removed (remove-test-vm.ps1, written from the app's own removal). Without the
; rights to manage Hyper-V (an administrator, or a member of Hyper-V Administrators) it changes
; nothing, and the uninstall goes on.
; The macro's body expands where Tauri inserts it (its generated installer.nsi, in another folder), so
; ${__FILEDIR__} is read here, at definition, not inside the macro. The path's separator is the
; backslash: makensis splits a File path at its last backslash into the folder and the file
; pattern, so with a forward slash it looked for `windows/remove-test-vm.ps1` inside `packaging`
; and found no file (every NSIS build from 2026-11-12 to 2026-11-13).
!define CYBERCTF_HOOKS_DIR "${__FILEDIR__}"
!macro NSIS_HOOK_PREUNINSTALL
  InitPluginsDir
  File "/oname=$PLUGINSDIR\remove-test-vm.ps1" "${CYBERCTF_HOOKS_DIR}\remove-test-vm.ps1"
  nsExec::ExecToLog 'powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -WindowStyle Hidden -File "$PLUGINSDIR\remove-test-vm.ps1"'
  Pop $0
  DetailPrint "Removing the self-test VM: exit code $0"
!macroend

; The command line, cyberctf.exe, is installed next to the app's executable (Tauri copies every
; binary of the package). Run with /CLIPATH, the installer also puts the install folder on the
; user's PATH, so `cyberctf` runs from any terminal; the uninstaller takes it off again whether or
; not it was put there. PowerShell reads and writes the user's PATH as the registry keeps it
; (REG_EXPAND_SZ), entry by entry, so nothing else on it changes and a second install adds nothing;
; clearing a variable through .NET then tells Explorer the environment changed, so a terminal opened
; from it next sees the new PATH.
!macro NSIS_HOOK_POSTINSTALL
  ClearErrors
  ${GetOptions} $CMDLINE "/CLIPATH" $0
  ${IfNot} ${Errors}
    nsExec::ExecToLog 'powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -WindowStyle Hidden -Command "$$d = $\'$INSTDIR$\'; $$k = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey($\'Environment$\', $$true); $$p = $$k.GetValue($\'Path$\', $\'$\', $\'DoNotExpandEnvironmentNames$\'); $$e = @($$p -split $\';$\' | Where-Object { $$_ }); if ($$e -notcontains $$d) { $$k.SetValue($\'Path$\', (($$e + $$d) -join $\';$\'), $\'ExpandString$\'); [Environment]::SetEnvironmentVariable($\'CYBERCTF_PATH_REFRESH$\', $$null, $\'User$\') }"'
    Pop $0
    DetailPrint "Putting cyberctf on PATH: exit code $0"
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  nsExec::ExecToLog 'powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -WindowStyle Hidden -Command "$$d = $\'$INSTDIR$\'; $$k = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey($\'Environment$\', $$true); if ($$k) { $$p = $$k.GetValue($\'Path$\', $\'$\', $\'DoNotExpandEnvironmentNames$\'); $$e = @($$p -split $\';$\' | Where-Object { $$_ -and $$_ -ne $$d }); $$k.SetValue($\'Path$\', ($$e -join $\';$\'), $\'ExpandString$\') }"'
  Pop $0
!macroend
