; Membrane Hub Windows installer (NSIS, rendered by the Tauri bundler).
;
; Written from scratch on 2026-09-02 to replace the Tauri-derived template.
; The installer does exactly four things and records each one:
;   1. copy the signed release into  $INSTDIR\versions\<version>\
;   2. point the stable junction      $INSTDIR\current  ->  versions\<version>
;   3. register uninstall, Start Menu shortcut and the login-launch Run value
;   4. write  $INSTDIR\logs\install-<version>.log  with one line per step
; It never activates the product. Activation (client hooks, PATH, resident
; tray) is the product's own command, `membrane activate`, which the
; qualification script and the first interactive launch run where its output
; is visible. Silent installs (/S) do nothing beyond the four steps above.
;
; NSIS quoting rule used throughout: strings are single-quoted when they
; contain double quotes. A dollar sign directly before a double quote is NOT
; an NSIS escape (the escape is dollar, backslash, quote); never write it.

; RightKit policy: upgrades and same-version repairs are automatic and in place.
; There is no maintenance page, no uninstall/reinstall choice, no reinstall
; prompt; the installer simply lays the version down and repoints current.
!define RIGHTKIT_AUTOMATIC_IN_PLACE_UPGRADE

Unicode true
ManifestDPIAware true
ManifestDPIAwareness PerMonitorV2

!if "{{compression}}" == "none"
  SetCompress off
!else
  SetCompressor /SOLID "{{compression}}"
!endif

; Signed plugin directories must precede any plugin use (see tauri-apps/tauri#15422).
!addplugindir "$%NSISPLUGINS%\x86-unicode"
{{#if signed_plugins_path}}
!addplugindir "{{signed_plugins_path}}"
{{/if}}
!addplugindir "{{additional_plugins_path}}"

!include MUI2.nsh
!include FileFunc.nsh
!include x64.nsh
!include WordFunc.nsh
; utils.nsh (Tauri) drives shortcut AppUserModelId and unpinning through COM;
; it needs these two headers, and reads ${ARCH}, ${INSTALLMODE}, ${BUNDLEID}.
!include "Win\COM.nsh"
!include "Win\Propkey.nsh"
!include "utils.nsh"

{{#if installer_hooks}}
!include "{{installer_hooks}}"
{{/if}}

!define WEBVIEW2APPGUID "{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}"
!define MANUFACTURER "{{manufacturer}}"
!define PRODUCTNAME "{{product_name}}"
!define INSTALLIDENTITY "Membrane Hub"
!define VERSION "{{version}}"
!define VERSIONWITHBUILD "{{version_with_build}}"
!define MAINBINARYNAME "{{main_binary_name}}"
; utils.nsh selects the current-user process probes from this define.
!define INSTALLMODE "{{install_mode}}"
!define ARCH "{{arch}}"
!define BUNDLEID "{{bundle_id}}"
!define COPYRIGHT "{{copyright}}"
!define OUTFILE "{{out_file}}"
!define INSTALLERICON "{{installer_icon}}"
!define INSTALLWEBVIEW2MODE "{{install_webview2_mode}}"
!define WEBVIEW2INSTALLERARGS "{{webview2_installer_args}}"
!define WEBVIEW2BOOTSTRAPPERPATH "{{webview2_bootstrapper_path}}"
!define WEBVIEW2INSTALLERPATH "{{webview2_installer_path}}"
!define UNINSTALLERSIGNCOMMAND "{{uninstaller_sign_cmd}}"
!define ESTIMATEDSIZE "{{estimated_size}}"
!define STARTMENUFOLDER "{{start_menu_folder}}"
!define UNINSTKEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\${INSTALLIDENTITY}"
!define MANUKEY "Software\${MANUFACTURER}"
!define MANUPRODUCTKEY "${MANUKEY}\${INSTALLIDENTITY}"
!define RUNKEY "Software\Microsoft\Windows\CurrentVersion\Run"
!define INSTALLLOG "$INSTDIR\logs\install-${VERSION}.log"

Name "${PRODUCTNAME}"
BrandingText "${COPYRIGHT}"
OutFile "${OUTFILE}"
RequestExecutionLevel user
InstallDir "$LOCALAPPDATA\Orthic Labs\Membrane"
ShowInstDetails show
ShowUninstDetails show

VIProductVersion "${VERSIONWITHBUILD}"
VIAddVersionKey "ProductName" "${PRODUCTNAME}"
VIAddVersionKey "FileDescription" "${PRODUCTNAME}"
VIAddVersionKey "CompanyName" "${MANUFACTURER}"
VIAddVersionKey "InternalName" "${INSTALLIDENTITY}"
VIAddVersionKey "LegalCopyright" "${COPYRIGHT}"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "ProductVersion" "${VERSION}"

!if "${UNINSTALLERSIGNCOMMAND}" != ""
  !uninstfinalize '${UNINSTALLERSIGNCOMMAND}'
!endif

!if "${INSTALLERICON}" != ""
  !define MUI_ICON "${INSTALLERICON}"
!endif

Var PassiveMode
Var NoShortcutMode
Var UpdateMode
Var AppStartMenuFolder
Var InstallStep

; ---------------------------------------------------------------- pages
!define MUI_PAGE_CUSTOMFUNCTION_PRE SkipIfPassive
!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_INSTFILES
!define MUI_FINISHPAGE_NOAUTOCLOSE
!define MUI_FINISHPAGE_RUN
!define MUI_FINISHPAGE_RUN_TEXT "Start Membrane"
!define MUI_FINISHPAGE_RUN_FUNCTION RunTray
!define MUI_PAGE_CUSTOMFUNCTION_PRE SkipIfPassive
!insertmacro MUI_PAGE_FINISH

!define MUI_PAGE_CUSTOMFUNCTION_PRE un.SkipIfPassive
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES

{{#each languages}}
!insertmacro MUI_LANGUAGE "{{this}}"
{{/each}}
!insertmacro MUI_RESERVEFILE_LANGDLL
{{#each language_files}}
  !include "{{this}}"
{{/each}}

; ---------------------------------------------------------------- helpers
; Append one line to the install log. Usage: ${Log} "text"
!macro LogLine text
  CreateDirectory "$INSTDIR\logs"
  ClearErrors
  FileOpen $9 "${INSTALLLOG}" a
  ${If} ${Errors}
    DetailPrint "log unavailable: ${text}"
  ${Else}
    FileSeek $9 0 END
    FileWrite $9 "${text}$\r$\n"
    FileClose $9
  ${EndIf}
!macroend
!define Log "!insertmacro LogLine"

Function SkipIfPassive
  ${If} $PassiveMode = 1
    Abort
  ${EndIf}
FunctionEnd

Function un.SkipIfPassive
  ${If} $PassiveMode = 1
    Abort
  ${EndIf}
FunctionEnd

Function RunTray
  nsis_tauri_utils::RunAsUser "$INSTDIR\current\membrane-tray.exe" ""
FunctionEnd

Function .onInit
  ${GetOptions} $CMDLINE "/P" $PassiveMode
  ${IfNot} ${Errors}
    StrCpy $PassiveMode 1
  ${EndIf}
  ${GetOptions} $CMDLINE "/NS" $NoShortcutMode
  ${IfNot} ${Errors}
    StrCpy $NoShortcutMode 1
  ${EndIf}
  ${GetOptions} $CMDLINE "/UPDATE" $UpdateMode
  ${IfNot} ${Errors}
    StrCpy $UpdateMode 1
  ${EndIf}
  SetShellVarContext current
  ; The product root is fixed. Nothing on the command line may move it.
  StrCpy $INSTDIR "$LOCALAPPDATA\Orthic Labs\Membrane"
  !if "${STARTMENUFOLDER}" != ""
    StrCpy $AppStartMenuFolder "${STARTMENUFOLDER}"
  !else
    StrCpy $AppStartMenuFolder "${PRODUCTNAME}"
  !endif
FunctionEnd

Function un.onInit
  SetShellVarContext current
  !insertmacro MUI_UNGETLANGUAGE
  ${GetOptions} $CMDLINE "/P" $PassiveMode
  ${IfNot} ${Errors}
    StrCpy $PassiveMode 1
  ${EndIf}
  ${GetOptions} $CMDLINE "/UPDATE" $UpdateMode
  ${IfNot} ${Errors}
    StrCpy $UpdateMode 1
  ${EndIf}
  !if "${STARTMENUFOLDER}" != ""
    StrCpy $AppStartMenuFolder "${STARTMENUFOLDER}"
  !else
    StrCpy $AppStartMenuFolder "${PRODUCTNAME}"
  !endif
FunctionEnd

; ---------------------------------------------------------------- WebView2
; The Hub is a Tauri app and needs the Evergreen WebView2 runtime. Present on
; every supported Windows by default; install it only when the registry says
; it is missing, using whatever mode the bundle configured.
Section WebView2
  ${If} ${RunningX64}
    ReadRegStr $4 HKLM "SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\${WEBVIEW2APPGUID}" "pv"
  ${Else}
    ReadRegStr $4 HKLM "SOFTWARE\Microsoft\EdgeUpdate\Clients\${WEBVIEW2APPGUID}" "pv"
  ${EndIf}
  ${If} $4 == ""
    ReadRegStr $4 HKCU "SOFTWARE\Microsoft\EdgeUpdate\Clients\${WEBVIEW2APPGUID}" "pv"
  ${EndIf}
  ${If} $4 != ""
    Return
  ${EndIf}
  ${If} $UpdateMode = 1
    Return
  ${EndIf}
  StrCpy $6 ""
  !if "${INSTALLWEBVIEW2MODE}" == "downloadBootstrapper"
    Delete "$TEMP\MicrosoftEdgeWebview2Setup.exe"
    DetailPrint "Downloading the WebView2 runtime bootstrapper"
    NSISdl::download "https://go.microsoft.com/fwlink/p/?LinkId=2124703" "$TEMP\MicrosoftEdgeWebview2Setup.exe"
    Pop $0
    ${If} $0 != "success"
      ${Log} "webview2-download exit=$0"
      Abort "WebView2 runtime download failed ($0). Install Microsoft Edge WebView2 and run this installer again."
    ${EndIf}
    StrCpy $6 "$TEMP\MicrosoftEdgeWebview2Setup.exe"
  !endif
  !if "${INSTALLWEBVIEW2MODE}" == "embedBootstrapper"
    Delete "$TEMP\MicrosoftEdgeWebview2Setup.exe"
    File "/oname=$TEMP\MicrosoftEdgeWebview2Setup.exe" "${WEBVIEW2BOOTSTRAPPERPATH}"
    StrCpy $6 "$TEMP\MicrosoftEdgeWebview2Setup.exe"
  !endif
  !if "${INSTALLWEBVIEW2MODE}" == "offlineInstaller"
    Delete "$TEMP\MicrosoftEdgeWebView2RuntimeInstaller.exe"
    File "/oname=$TEMP\MicrosoftEdgeWebView2RuntimeInstaller.exe" "${WEBVIEW2INSTALLERPATH}"
    StrCpy $6 "$TEMP\MicrosoftEdgeWebView2RuntimeInstaller.exe"
  !endif
  ${If} $6 == ""
    ${Log} "webview2-missing exit=1 no bootstrapper in this installer"
    Abort "The Microsoft Edge WebView2 runtime is missing and this installer carries no bootstrapper. Install WebView2 and run this installer again."
  ${EndIf}
  DetailPrint "Installing the WebView2 runtime"
  ExecWait '"$6" ${WEBVIEW2INSTALLERARGS} /install' $1
  ${If} $1 <> 0
    ${Log} "webview2-install exit=$1"
    Abort "WebView2 runtime installation failed (exit $1)."
  ${EndIf}
  ${Log} "webview2-install ok"
SectionEnd

; ---------------------------------------------------------------- install
Section Install
  !ifmacrodef NSIS_HOOK_PREINSTALL
    !insertmacro NSIS_HOOK_PREINSTALL
  !endif

  ; Silent and passive runs terminate the product; interactive runs ask.
  !insertmacro CheckIfAppIsRunning "${MAINBINARYNAME}.exe" "${PRODUCTNAME}"
  !insertmacro CheckIfAppIsRunning "membrane-tray.exe" "Membrane"
  !insertmacro CheckIfAppIsRunning "membrane-daemon.exe" "Membrane"

  CreateDirectory "$INSTDIR\logs"
  ${Log} "install ${VERSION} begin"

  ; 1. Extract the release straight into place. The bundler's resource entries
  ;    are already rooted at versions\<version>\..., so with $OUTDIR at the
  ;    product root each File lands in its final path: no staging copy, no
  ;    MAX_PATH doubling. Same-version repair overwrites in place.
  StrCpy $InstallStep "extract-version-tree"
  SetOutPath "$INSTDIR"
  {{#each resources_dirs}}
    CreateDirectory "$INSTDIR\\{{this}}"
  {{/each}}
  {{#each resources}}
    File /a "/oname={{this.[1]}}" "{{no-escape @key}}"
  {{/each}}
  ${Log} "extract-version-tree ok"

  ; Every executable the product runs from this version must be present.
  StrCpy $InstallStep "verify-version-tree"
  ${IfNot} ${FileExists} "$INSTDIR\versions\${VERSION}\membrane.exe"
  ${OrIfNot} ${FileExists} "$INSTDIR\versions\${VERSION}\${MAINBINARYNAME}.exe"
  ${OrIfNot} ${FileExists} "$INSTDIR\versions\${VERSION}\membrane-tray.exe"
  ${OrIfNot} ${FileExists} "$INSTDIR\versions\${VERSION}\membrane-daemon.exe"
  ${OrIfNot} ${FileExists} "$INSTDIR\versions\${VERSION}\cortex.exe"
    StrCpy $R0 1
    Goto install_failed
  ${EndIf}
  ${Log} "verify-version-tree ok"

  ; 2. Point the stable junction. RMDir removes a junction or an empty
  ;    directory through RemoveDirectory without touching its target; only a
  ;    populated real directory (never a junction) reaches the recursive form.
  StrCpy $InstallStep "remove-old-current"
  ${If} ${FileExists} "$INSTDIR\current\*.*"
    RMDir "$INSTDIR\current"
  ${EndIf}
  ${If} ${FileExists} "$INSTDIR\current\*.*"
    RMDir /r "$INSTDIR\current"
  ${EndIf}
  ${If} ${FileExists} "$INSTDIR\current\*.*"
    StrCpy $R0 1
    Goto install_failed
  ${EndIf}
  ${Log} "remove-old-current ok"

  StrCpy $InstallStep "create-current-junction"
  ExecWait '"$SYSDIR\cmd.exe" /d /c mklink /J "$INSTDIR\current" "$INSTDIR\versions\${VERSION}" >> "${INSTALLLOG}" 2>&1' $R0
  ${If} $R0 <> 0
    Goto install_failed
  ${EndIf}
  ${IfNot} ${FileExists} "$INSTDIR\current\membrane.exe"
    StrCpy $R0 1
    Goto install_failed
  ${EndIf}
  ${Log} "create-current-junction ok"

  ; 3. Registration: uninstall entry, Start Menu shortcut, login launch.
  StrCpy $InstallStep "register"
  SetOutPath "$INSTDIR"
  WriteUninstaller "$INSTDIR\uninstall.exe"
  WriteRegStr HKCU "${MANUPRODUCTKEY}" "" "$INSTDIR"
  WriteRegStr HKCU "${UNINSTKEY}" "DisplayName" "${PRODUCTNAME}"
  WriteRegStr HKCU "${UNINSTKEY}" "DisplayIcon" '"$INSTDIR\current\${MAINBINARYNAME}.exe"'
  WriteRegStr HKCU "${UNINSTKEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "${UNINSTKEY}" "Publisher" "${MANUFACTURER}"
  WriteRegStr HKCU "${UNINSTKEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${UNINSTKEY}" "MainBinaryName" "${MAINBINARYNAME}.exe"
  WriteRegStr HKCU "${UNINSTKEY}" "UninstallString" '"$INSTDIR\uninstall.exe"'
  WriteRegStr HKCU "${UNINSTKEY}" "QuietUninstallString" '"$INSTDIR\uninstall.exe" /S'
  WriteRegDWORD HKCU "${UNINSTKEY}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINSTKEY}" "NoRepair" 1
  !if "${ESTIMATEDSIZE}" != ""
    WriteRegDWORD HKCU "${UNINSTKEY}" "EstimatedSize" "${ESTIMATEDSIZE}"
  !endif

  ; The tray starts at login. Keep an existing user decision (value present or
  ; absent) on upgrade; write it on first install.
  ReadRegStr $R2 HKCU "${UNINSTKEY}" "LoginLaunchWritten"
  ${If} $R2 == ""
    WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "Membrane" '"$INSTDIR\current\membrane-tray.exe" --login-launch'
    WriteRegStr HKCU "${UNINSTKEY}" "LoginLaunchWritten" "1"
  ${EndIf}
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "Membrane Tray"

  ${If} $NoShortcutMode <> 1
    CreateDirectory "$SMPROGRAMS\$AppStartMenuFolder"
    Delete "$SMPROGRAMS\$AppStartMenuFolder\${PRODUCTNAME}.lnk"
    CreateShortcut "$SMPROGRAMS\$AppStartMenuFolder\${PRODUCTNAME}.lnk" "$INSTDIR\current\membrane-tray.exe" "--open-dashboard"
    !insertmacro SetLnkAppUserModelId "$SMPROGRAMS\$AppStartMenuFolder\${PRODUCTNAME}.lnk"
  ${EndIf}
  ${Log} "register ok"

  ; 4. Activation is the product's job. Interactive installs run it here with
  ;    the console hidden and its output captured to logs\activate.log; its
  ;    result is recorded, never fatal, and the finish page's tray launch only
  ;    happens after it returns. Silent installs leave activation to the caller
  ;    (qualification runs `membrane activate` itself).
  ${IfNot} ${Silent}
    DetailPrint "Activating Membrane"
    nsExec::ExecToStack '"$INSTDIR\current\membrane.exe" activate --install-root "$INSTDIR\current"'
    Pop $R0
    Pop $R2
    ClearErrors
    FileOpen $9 "$INSTDIR\logs\activate.log" w
    ${IfNot} ${Errors}
      FileWrite $9 "$R2"
      FileClose $9
    ${EndIf}
    ${Log} "activate exit=$R0 (output in logs\activate.log)"
  ${Else}
    ${Log} "activate skipped (silent install)"
  ${EndIf}

  ${Log} "install ${VERSION} complete"
  !ifmacrodef NSIS_HOOK_POSTINSTALL
    !insertmacro NSIS_HOOK_POSTINSTALL
  !endif
  Goto install_done

  install_failed:
    ${Log} "$InstallStep exit=$R0"
    Abort "Membrane installation failed at $InstallStep (exit $R0). See ${INSTALLLOG}"
  install_done:
SectionEnd

Function .onInstSuccess
  ${If} $PassiveMode = 1
  ${OrIf} ${Silent}
    ${GetOptions} $CMDLINE "/R" $R0
    ${IfNot} ${Errors}
      ${GetOptions} $CMDLINE "/ARGS" $R0
      nsis_tauri_utils::RunAsUser "$INSTDIR\current\membrane-tray.exe" "$R0"
    ${EndIf}
  ${EndIf}
FunctionEnd

; ---------------------------------------------------------------- uninstall
Section Uninstall
  !ifmacrodef NSIS_HOOK_PREUNINSTALL
    !insertmacro NSIS_HOOK_PREUNINSTALL
  !endif

  !insertmacro CheckIfAppIsRunning "${MAINBINARYNAME}.exe" "${PRODUCTNAME}"
  !insertmacro CheckIfAppIsRunning "membrane-tray.exe" "Membrane"
  !insertmacro CheckIfAppIsRunning "membrane-daemon.exe" "Membrane"

  ; Deactivation removes the product's client hooks and PATH entry. Its result
  ; is recorded, not fatal: an uninstall must always remove the files.
  ${If} ${FileExists} "$INSTDIR\current\membrane.exe"
    CreateDirectory "$INSTDIR\logs"
    ExecWait '"$SYSDIR\cmd.exe" /d /s /c ""$INSTDIR\current\membrane.exe" deactivate --install-root "$INSTDIR\current" > "$INSTDIR\logs\deactivate.log" 2>&1"' $R0
    DetailPrint "membrane deactivate exit=$R0"
  ${EndIf}

  ; Remove the junction first so no recursive delete below can follow it into
  ; a version tree, then remove the whole product root. Durable user data lives
  ; outside $INSTDIR (see membrane doctor roots) and is never touched here.
  ${If} ${FileExists} "$INSTDIR\current\*.*"
    RMDir "$INSTDIR\current"
  ${EndIf}
  ${If} ${FileExists} "$INSTDIR\current\*.*"
    Abort "Membrane uninstall stopped: $INSTDIR\current could not be removed as a junction."
  ${EndIf}
  RMDir /r "$INSTDIR\versions"
  Delete "$INSTDIR\integration-journal.json"
  Delete "$INSTDIR\uninstall.exe"
  RMDir /r "$INSTDIR"

  ${If} $UpdateMode <> 1
    !insertmacro DeleteAppUserModelId
    !insertmacro UnpinShortcut "$SMPROGRAMS\$AppStartMenuFolder\${PRODUCTNAME}.lnk"
    Delete "$SMPROGRAMS\$AppStartMenuFolder\${PRODUCTNAME}.lnk"
    RMDir "$SMPROGRAMS\$AppStartMenuFolder"
    Delete "$SMPROGRAMS\${PRODUCTNAME}.lnk"
    Delete "$DESKTOP\${PRODUCTNAME}.lnk"
    DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "Membrane"
    DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "Membrane Tray"
  ${EndIf}

  DeleteRegKey HKCU "${UNINSTKEY}"
  DeleteRegKey HKCU "${MANUPRODUCTKEY}"
  DeleteRegKey /ifempty HKCU "${MANUKEY}"

  !ifmacrodef NSIS_HOOK_POSTUNINSTALL
    !insertmacro NSIS_HOOK_POSTUNINSTALL
  !endif

  ${If} $PassiveMode = 1
  ${OrIf} $UpdateMode = 1
    SetAutoClose true
  ${EndIf}
SectionEnd
