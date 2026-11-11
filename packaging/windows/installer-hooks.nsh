; Hooks of the NSIS installer (tauri.conf.json `bundle.windows.nsis.installerHooks`).

; Before the uninstaller removes anything: the machine self-test's Hyper-V VM, if one was left
; running (the test interrupted). It runs from a disk in the app's cache folder, which the
; uninstaller may delete with the app's data; left running, it would hold that disk and stay,
; with nothing left to remove it. Only a VM named exactly "CyberCTF test VM" whose every disk is
; in that folder is removed (remove-test-vm.ps1, written from the app's own removal). Without the
; rights to manage Hyper-V (an administrator, or a member of Hyper-V Administrators) it changes
; nothing, and the uninstall goes on.
; The macro's body expands where Tauri inserts it (its generated installer.nsi, in another folder), so
; ${__FILEDIR__} is read here, at definition, not inside the macro.
!define CYBERCTF_HOOKS_DIR "${__FILEDIR__}"
!macro NSIS_HOOK_PREUNINSTALL
  InitPluginsDir
  File "/oname=$PLUGINSDIR\remove-test-vm.ps1" "${CYBERCTF_HOOKS_DIR}/remove-test-vm.ps1"
  nsExec::ExecToLog 'powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -WindowStyle Hidden -File "$PLUGINSDIR\remove-test-vm.ps1"'
  Pop $0
  DetailPrint "Removing the self-test VM: exit code $0"
!macroend
