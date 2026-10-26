Unicode True
!include "LogicLib.nsh"
Name "Kwel Connect"
OutFile "${OUTPUT}"
InstallDir "$LOCALAPPDATA\Kwel\NativeBridge"
RequestExecutionLevel user
ShowInstDetails show
ShowUninstDetails show
SetCompressor /SOLID lzma
Page instfiles
UninstPage uninstConfirm
UninstPage instfiles
Function .onInit
  StrCpy $INSTDIR "$LOCALAPPDATA\Kwel\NativeBridge"
FunctionEnd
Section "Kwel Connect"
  SetShellVarContext current
  InitPluginsDir
  SetOutPath "$PLUGINSDIR\payload"
  File /oname=kwel-native-host.exe "${PAYLOAD}\kwel-native-host.exe"
  File /oname=kwel-bridge.exe "${PAYLOAD}\kwel-bridge.exe"
  ExecWait '"$PLUGINSDIR\payload\kwel-native-host.exe" install --source "$PLUGINSDIR\payload"' $0
  ${If} $0 != 0
    MessageBox MB_ICONSTOP|MB_OK "Kwel Connect could not be installed safely. Disable Local Connection and check that no other Host registration conflicts." /SD IDOK
    SetErrorLevel 1
    Abort
  ${EndIf}
  WriteUninstaller "$INSTDIR\Uninstall.exe"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\KwelConnect" "DisplayName" "Kwel Connect"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\KwelConnect" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\KwelConnect" "Publisher" "Kwel"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\KwelConnect" "UninstallString" '$\"$INSTDIR\Uninstall.exe$\"'
SectionEnd
Section "Uninstall"
  SetShellVarContext current
  InitPluginsDir
  CopyFiles /SILENT "$INSTDIR\bin\kwel-native-host.exe" "$PLUGINSDIR\kwel-native-host.exe"
  ExecWait '"$PLUGINSDIR\kwel-native-host.exe" uninstall' $0
  ${If} $0 != 0
    MessageBox MB_ICONSTOP|MB_OK "Kwel Connect is in use or could not be removed safely. Disable Local Connection, then retry." /SD IDOK
    SetErrorLevel 1
    Abort
  ${EndIf}
  DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\KwelConnect"
  Delete "$INSTDIR\Uninstall.exe"
  RMDir "$INSTDIR\bin"
  RMDir "$INSTDIR\manifests"
  ; No recursive deletion: runtime and other launchers' files are preserved.
SectionEnd
