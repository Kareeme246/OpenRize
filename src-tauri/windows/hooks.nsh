; NSIS installer hooks, wired in by `bundle.windows.nsis.installerHooks` in
; tauri.windows.conf.json. Tauri expands these macros inside its own installer
; template. Everything here is best effort: a failure must never block an
; install, an in-app update or an uninstall, so every result is discarded.
;
; Every command below is run with `nsExec::Exec ... /TIMEOUT=30000`: nsExec
; otherwise waits for its child forever, with no window and no way out, so one
; wedged `rize` process would hang the installer, an in-app update or an
; uninstall. The /TIMEOUT gives up after 30 seconds with no output from the
; child, terminates it and pushes "timeout" instead of an exit code, so the
; worst case is a missing PATH entry rather than a stuck installer.
; `rize install-path` and `uninstall-path` finish in well under a second
; (measured on a CI runner), so the bound only fires when something is wrong.
;
; ASCII only: NSIS reads this file as ANSI unless it has a byte order mark.

; The HKCU Run value the app's Launch at login switch writes
; (VALUE in src/login_item/windows.rs).
!define OPENRIZE_RUN_KEY "Software\Microsoft\Windows\CurrentVersion\Run"
!define OPENRIZE_APPROVED_KEY "Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run"
!define OPENRIZE_RUN_VALUE "OpenRize"
!define OPENRIZE_EXEC_TIMEOUT 30000

!macro NSIS_HOOK_POSTINSTALL
  ; Puts rize on the user PATH, as the Mac app does at launch. It adds this
  ; folder only if it is missing, so in-app updates (which run this again) and
  ; reinstalls change nothing. nsExec runs it hidden and waits, at most
  ; OPENRIZE_EXEC_TIMEOUT for any output.
  ${If} ${FileExists} "$INSTDIR\rize.exe"
    nsExec::Exec /TIMEOUT=${OPENRIZE_EXEC_TIMEOUT} '"$INSTDIR\rize.exe" install-path'
    Pop $0
  ${EndIf}
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  ; An in-app update runs the old uninstaller with /UPDATE; the PATH entry and
  ; the login item must survive that.
  ${If} $UpdateMode <> 1
    ${If} ${FileExists} "$INSTDIR\rize.exe"
      nsExec::Exec /TIMEOUT=${OPENRIZE_EXEC_TIMEOUT} '"$INSTDIR\rize.exe" uninstall-path'
      Pop $0
    ${EndIf}
    DeleteRegValue HKCU "${OPENRIZE_RUN_KEY}" "${OPENRIZE_RUN_VALUE}"
    DeleteRegValue HKCU "${OPENRIZE_APPROVED_KEY}" "${OPENRIZE_RUN_VALUE}"
  ${EndIf}
!macroend
