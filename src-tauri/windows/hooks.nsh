; NSIS installer hooks, wired in by `bundle.windows.nsis.installerHooks` in
; tauri.windows.conf.json. Tauri expands these macros inside its own installer
; template. Everything here is best effort: a failure must never block an
; install, an in-app update or an uninstall, so every result is discarded.
;
; ASCII only: NSIS reads this file as ANSI unless it has a byte order mark.

; The HKCU Run value the app's Launch at login switch writes
; (VALUE in src/login_item/windows.rs).
!define OPENRIZE_RUN_KEY "Software\Microsoft\Windows\CurrentVersion\Run"
!define OPENRIZE_APPROVED_KEY "Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run"
!define OPENRIZE_RUN_VALUE "OpenRize"

!macro NSIS_HOOK_POSTINSTALL
  ; Puts rize on the user PATH, as the Mac app does at launch. It adds this
  ; folder only if it is missing, so in-app updates (which run this again) and
  ; reinstalls change nothing. nsExec runs it hidden and waits.
  ${If} ${FileExists} "$INSTDIR\rize.exe"
    nsExec::Exec '"$INSTDIR\rize.exe" install-path'
    Pop $0
  ${EndIf}

  ; Earlier installers recorded the install folder under the identifier's
  ; "offlinestudios" key; the publisher name now writes it under its own.
  DeleteRegKey HKCU "Software\offlinestudios\openrize"
  DeleteRegKey /ifempty HKCU "Software\offlinestudios"
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  ; An in-app update runs the old uninstaller with /UPDATE; the PATH entry and
  ; the login item must survive that.
  ${If} $UpdateMode <> 1
    ${If} ${FileExists} "$INSTDIR\rize.exe"
      nsExec::Exec '"$INSTDIR\rize.exe" uninstall-path'
      Pop $0
    ${EndIf}
    DeleteRegValue HKCU "${OPENRIZE_RUN_KEY}" "${OPENRIZE_RUN_VALUE}"
    DeleteRegValue HKCU "${OPENRIZE_APPROVED_KEY}" "${OPENRIZE_RUN_VALUE}"
  ${EndIf}
!macroend
