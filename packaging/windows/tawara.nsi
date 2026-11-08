; Tawara's Windows installer, built with NSIS 3.13. packaging/windows.sh runs
; makensis on this file and passes the defines listed below. The file holds no
; key, certificate or password; the signing certificate's thumbprint is passed
; as an optional define.
;
; Defines, all set by packaging/windows.sh:
;   TAWARA_VERSION        the version people see, X.Y.Z (or X.Y.Z-dev)
;   TAWARA_FILE_VERSION   the numeric version, X.Y.Z.0
;   TAWARA_STAGE          the folder holding tawara-desktop.exe, LICENSE.md and
;                         THIRD-PARTY-NOTICES.txt
;   TAWARA_OUT            the installer file to write
;   TAWARA_PUBLISHER      the publisher name that Installed apps shows
; Optional:
;   TAWARA_ICO            an .ico file for the installer's and uninstaller's icons
;   TAWARA_WIN_CERT_SHA1  the signing certificate's SHA-1 thumbprint. With it, the
;                         uninstaller is signed as NSIS makes it, through
;                         !uninstfinalize. TAWARA_TIMESTAMP_URL and
;                         TAWARA_UNINST_STAMP are then required.
;   TAWARA_TIMESTAMP_URL  the certificate authority's RFC 3161 timestamp address
;   TAWARA_UNINST_STAMP   a file the uninstaller signing command writes on
;                         success, checked by the script
;
; The program installs per user into %LOCALAPPDATA%\Programs\Tawara, without an
; administrator prompt. The uninstaller removes only the program's files, the
; folder if it is then empty, the Start menu entry and the Installed apps entry.
; It never touches the wallet store under %LOCALAPPDATA%\Tawara or the
; preferences. The pages use NSIS's standard English text.

Unicode true
SetCompressor zlib
RequestExecutionLevel user
ManifestDPIAware true

!ifndef TAWARA_VERSION
  !error "TAWARA_VERSION is not set: run packaging/windows.sh, which passes it"
!endif
!ifndef TAWARA_FILE_VERSION
  !error "TAWARA_FILE_VERSION is not set: run packaging/windows.sh, which passes it"
!endif
!ifndef TAWARA_STAGE
  !error "TAWARA_STAGE is not set: run packaging/windows.sh, which passes it"
!endif
!ifndef TAWARA_OUT
  !error "TAWARA_OUT is not set: run packaging/windows.sh, which passes it"
!endif
!ifndef TAWARA_PUBLISHER
  !error "TAWARA_PUBLISHER is not set: run packaging/windows.sh, which passes it"
!endif

!ifdef TAWARA_WIN_CERT_SHA1
  !ifndef TAWARA_TIMESTAMP_URL
    !error "TAWARA_WIN_CERT_SHA1 is set, so TAWARA_TIMESTAMP_URL must be set too"
  !endif
  !ifndef TAWARA_UNINST_STAMP
    !error "TAWARA_WIN_CERT_SHA1 is set, so TAWARA_UNINST_STAMP must be set too"
  !endif
  ; Signs the generated uninstaller ("%1"). signtool is found on PATH, which
  ; packaging/windows.sh extends with the SDK folder, because cmd.exe would
  ; split a quoted path here. The script validates the thumbprint and timestamp
  ; address. makensis ignores a failing command, so success is signalled by
  ; writing TAWARA_UNINST_STAMP, which the script requires.
  !uninstfinalize 'signtool sign /sha1 ${TAWARA_WIN_CERT_SHA1} /fd sha256 /tr ${TAWARA_TIMESTAMP_URL} /td sha256 "%1" && echo signed > "${TAWARA_UNINST_STAMP}"'
!endif

!define TAWARA_UNINSTALL_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\Tawara"

Name "Tawara"
OutFile "${TAWARA_OUT}"
InstallDir "$LOCALAPPDATA\Programs\Tawara"

VIProductVersion "${TAWARA_FILE_VERSION}"
VIAddVersionKey "ProductName" "Tawara"
VIAddVersionKey "FileDescription" "Tawara installer"
VIAddVersionKey "FileVersion" "${TAWARA_FILE_VERSION}"
; NSIS warns (9100, fatal under -WX) when LegalCopyright is missing.
VIAddVersionKey "LegalCopyright" "Copyright (c) 2026 Adequate Systems HK Limited"

!include "MUI2.nsh"

!ifdef TAWARA_ICO
  !define MUI_ICON "${TAWARA_ICO}"
  !define MUI_UNICON "${TAWARA_ICO}"
!endif

!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"

; Resolve $SMPROGRAMS and the other shell folders for the current user.
; SetShellVarContext is valid only inside a section or function.
Function .onInit
  SetShellVarContext current
FunctionEnd

Function un.onInit
  SetShellVarContext current
FunctionEnd

Section "Tawara"
  SetOutPath "$INSTDIR"
  File "${TAWARA_STAGE}\tawara-desktop.exe"
  File "${TAWARA_STAGE}\LICENSE.md"
  File "${TAWARA_STAGE}\THIRD-PARTY-NOTICES.txt"
  WriteUninstaller "$INSTDIR\uninstall.exe"
  CreateShortCut "$SMPROGRAMS\Tawara.lnk" "$INSTDIR\tawara-desktop.exe" "" "$INSTDIR\tawara-desktop.exe" 0
  WriteRegStr HKCU "${TAWARA_UNINSTALL_KEY}" "DisplayName" "Tawara"
  WriteRegStr HKCU "${TAWARA_UNINSTALL_KEY}" "DisplayVersion" "${TAWARA_VERSION}"
  WriteRegStr HKCU "${TAWARA_UNINSTALL_KEY}" "Publisher" "${TAWARA_PUBLISHER}"
  WriteRegStr HKCU "${TAWARA_UNINSTALL_KEY}" "DisplayIcon" "$INSTDIR\tawara-desktop.exe"
  WriteRegStr HKCU "${TAWARA_UNINSTALL_KEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${TAWARA_UNINSTALL_KEY}" "UninstallString" "$\"$INSTDIR\uninstall.exe$\""
  WriteRegStr HKCU "${TAWARA_UNINSTALL_KEY}" "QuietUninstallString" "$\"$INSTDIR\uninstall.exe$\" /S"
  WriteRegDWORD HKCU "${TAWARA_UNINSTALL_KEY}" "NoModify" 1
  WriteRegDWORD HKCU "${TAWARA_UNINSTALL_KEY}" "NoRepair" 1
SectionEnd

Section "un.Tawara"
  Delete "$INSTDIR\tawara-desktop.exe"
  Delete "$INSTDIR\LICENSE.md"
  Delete "$INSTDIR\THIRD-PARTY-NOTICES.txt"
  Delete "$SMPROGRAMS\Tawara.lnk"
  DeleteRegKey HKCU "${TAWARA_UNINSTALL_KEY}"
  Delete "$INSTDIR\uninstall.exe"
  RMDir "$INSTDIR"
SectionEnd
