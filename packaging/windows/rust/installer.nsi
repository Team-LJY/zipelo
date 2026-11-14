Unicode true
!include "MUI2.nsh"
!include "nsDialogs.nsh"
!include "LogicLib.nsh"
!include "x64.nsh"
!include "FileFunc.nsh"
Name "Zipelo"
OutFile "Zipelo-0.2.0-windows-x64-setup.exe"
!define INSTALLROOT "$LOCALAPPDATA\Programs\Zipelo"
InstallDir "${INSTALLROOT}"
RequestExecutionLevel user
SetCompressor /SOLID lzma
SetOverwrite off
AllowSkipFiles off
ShowInstDetails show
!define MUI_ICON "zipelo.ico"
!define MUI_UNICON "zipelo.ico"
!define MUI_ABORTWARNING
!define MUI_CUSTOMFUNCTION_ABORT Rollback
!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_INSTFILES
Page custom FinishCreate
UninstPage custom un.AssociationsCreate
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "Korean"
Var Dialog
Var ApplyButton
Var Mutex
Var RootOwned
Var ShortcutOwned
Var UninstallOwned
Var InstallComplete
Var Upgrading
Var StageDir
Var BackupBits
Var PublishedBits
Var RestoreFailed
Var OldVersion
Var VersionChanged
!define APPKEY "Software\Zipelo"
!define PROGKEY "Software\Classes\Zipelo.Archive"
!define UNKEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\Zipelo"

; 기존 앱을 먼저 치우고 새 앱은 마지막에 공개하여 버전 혼합 실행을 줄임.
!macro BackupFile NAME BIT
    ClearErrors
    Rename "$INSTDIR\${NAME}" "$StageDir\previous\${NAME}"
    IfErrors failed
    IntOp $BackupBits $BackupBits | ${BIT}
!macroend

!macro PublishFile NAME BIT
    ClearErrors
    Rename "$StageDir\${NAME}" "$INSTDIR\${NAME}"
    IfErrors failed
    IntOp $PublishedBits $PublishedBits | ${BIT}
!macroend

!macro RestoreFile NAME BIT
    IntOp $0 $PublishedBits & ${BIT}
    ${If} $0 != 0
        ClearErrors
        Delete "$INSTDIR\${NAME}"
        ${If} ${Errors}
            StrCpy $RestoreFailed 1
        ${Else}
            IntOp $PublishedBits $PublishedBits ^ ${BIT}
        ${EndIf}
    ${EndIf}
    IntOp $0 $BackupBits & ${BIT}
    ${If} $0 != 0
        ClearErrors
        Rename "$StageDir\previous\${NAME}" "$INSTDIR\${NAME}"
        ${If} ${Errors}
            StrCpy $RestoreFailed 1
        ${Else}
            IntOp $BackupBits $BackupBits ^ ${BIT}
        ${EndIf}
    ${EndIf}
!macroend

!macro CleanupStageFile NAME
    Delete "$StageDir\${NAME}"
    ${If} $InstallComplete == 1
        Delete "$StageDir\previous\${NAME}"
    ${EndIf}
!macroend

!macro CheckInstalledFile NAME
    System::Call 'kernel32::GetFileAttributesW(w "$INSTDIR\${NAME}") i.r0'
    ${If} $0 == -1
        Goto failed
    ${EndIf}
    IntOp $0 $0 & 0x410
    ${If} $0 != 0
        Goto failed
    ${EndIf}
!macroend

Function .onInit
    ${IfNot} ${RunningX64}
        MessageBox MB_OK|MB_ICONSTOP "64비트 Windows가 필요합니다."
        Abort
    ${EndIf}
    SetShellVarContext current
    SetRegView 64
    System::Call 'kernel32::SetLastError(i 0)'
    System::Call 'kernel32::CreateMutexW(p 0, i 0, w "Local\ZipeloSetup") p.r0 ?e'
    Pop $1
    StrCpy $Mutex $0
    ${If} $0 == 0
    ${OrIf} $1 != 0
        MessageBox MB_OK "다른 설치나 제거를 마친 뒤 다시 실행하세요."
        Abort
    ${EndIf}
FunctionEnd

Section "Zipelo"
    ; /D로 바뀐 경로를 파일 쓰기 전에 고정하여 제거 경로와 일치시킴.
    StrCpy $INSTDIR "${INSTALLROOT}"
    ; 설치 표식과 제거 등록이 일치하는 기존 설치만 업데이트함.
    IfFileExists "$INSTDIR" upgrade
    Goto fresh
upgrade:
    System::Call 'kernel32::GetFileAttributesW(w "$INSTDIR") i.r0'
    IntOp $0 $0 & 0x410
    IntCmp $0 0x10 0 failed failed
    ReadINIStr $0 "$INSTDIR\install.ini" "Zipelo" "Location"
    StrCmp $0 $INSTDIR 0 failed
    ReadRegStr $0 HKCU "${UNKEY}" "UninstallString"
    StrCmp $0 '$\"$INSTDIR\Uninstall.exe$\"' 0 failed
    !insertmacro CheckInstalledFile "Zipelo.exe"
    !insertmacro CheckInstalledFile "zipelo-archive-worker.exe"
    !insertmacro CheckInstalledFile "LICENSE"
    !insertmacro CheckInstalledFile "THIRD-PARTY-LICENSES.txt"
    !insertmacro CheckInstalledFile "BUILD-INFO.json"
    !insertmacro CheckInstalledFile "Uninstall.exe"
    !insertmacro CheckInstalledFile "install.ini"
    ClearErrors
    ReadRegStr $OldVersion HKCU "${UNKEY}" "DisplayVersion"
    IfErrors failed
    StrCpy $Upgrading 1
    Goto prepare
fresh:
    System::Call 'advapi32::RegOpenKeyExW(p 0x80000001, w "${APPKEY}", i 0, i 0x20119, *p.r1) i.r0'
    ${If} $0 == 0
        System::Call 'advapi32::RegCloseKey(p r1)'
        Goto failed
    ${ElseIf} $0 != 2
        Goto failed
    ${EndIf}
    System::Call 'advapi32::RegOpenKeyExW(p 0x80000001, w "${PROGKEY}", i 0, i 0x20119, *p.r1) i.r0'
    ${If} $0 == 0
        System::Call 'advapi32::RegCloseKey(p r1)'
        Goto failed
    ${ElseIf} $0 != 2
        Goto failed
    ${EndIf}
    System::Call 'advapi32::RegOpenKeyExW(p 0x80000001, w "${UNKEY}", i 0, i 0x20119, *p.r1) i.r0'
    ${If} $0 == 0
        System::Call 'advapi32::RegCloseKey(p r1)'
        Goto failed
    ${ElseIf} $0 != 2
        Goto failed
    ${EndIf}
    ; 공유 등록 값이 이미 있으면 형식과 관계없이 보존함.
    System::Call 'advapi32::RegOpenKeyExW(p 0x80000001, w "Software\RegisteredApplications", i 0, i 0x20119, *p.r1) i.r0'
    ${If} $0 == 0
        System::Call 'advapi32::RegQueryValueExW(p r1, w "Zipelo", p 0, p 0, p 0, p 0) i.r2'
        System::Call 'advapi32::RegCloseKey(p r1)'
        ${If} $2 != 2
            Goto failed
        ${EndIf}
    ${ElseIf} $0 != 2
        Goto failed
    ${EndIf}
    IfFileExists "$SMPROGRAMS\Zipelo.lnk" failed
    ClearErrors
    CreateDirectory "$LOCALAPPDATA\Programs"
    IfErrors failed
    System::Call 'kernel32::CreateDirectoryW(w "$INSTDIR", p 0) i.r0'
    IntCmp $0 0 failed
    StrCpy $RootOwned 1
prepare:
    ; 같은 볼륨에 새 파일을 모두 준비한 뒤에만 기존 파일을 이동함.
    ClearErrors
    GetTempFileName $StageDir "$LOCALAPPDATA\Programs"
    IfErrors failed
    Delete "$StageDir"
    System::Call 'kernel32::CreateDirectoryW(w "$StageDir", p 0) i.r0'
    ${If} $0 == 0
        StrCpy $StageDir ""
        Goto failed
    ${EndIf}
    SetOutPath "$StageDir"
    ClearErrors
    File "payload\Zipelo.exe"
    File "payload\zipelo-archive-worker.exe"
    File "payload\LICENSE"
    File "payload\THIRD-PARTY-LICENSES.txt"
    File "payload\BUILD-INFO.json"
    IfErrors failed
    WriteUninstaller "$StageDir\Uninstall.exe"
    IfErrors failed
    WriteINIStr "$StageDir\install.ini" "Zipelo" "Location" "$INSTDIR"
    IfErrors failed
    ${If} $Upgrading == 1
        ; 준비 후 취소하면 기존 설치와 연결은 전혀 바뀌지 않음.
        MessageBox MB_OKCANCEL|MB_ICONINFORMATION "업데이트 준비가 끝났습니다. Zipelo와 압축 작업을 닫고 [확인]을 누르세요. [취소]를 누르면 기존 설치를 유지합니다." /SD IDOK IDCANCEL cancelled
        CreateDirectory "$StageDir\previous"
        IfErrors failed
        !insertmacro BackupFile "Zipelo.exe" 1
        !insertmacro BackupFile "zipelo-archive-worker.exe" 2
        !insertmacro BackupFile "LICENSE" 4
        !insertmacro BackupFile "THIRD-PARTY-LICENSES.txt" 8
        !insertmacro BackupFile "BUILD-INFO.json" 16
        !insertmacro BackupFile "Uninstall.exe" 32
        !insertmacro BackupFile "install.ini" 64
    ${EndIf}
    !insertmacro PublishFile "zipelo-archive-worker.exe" 2
    !insertmacro PublishFile "LICENSE" 4
    !insertmacro PublishFile "THIRD-PARTY-LICENSES.txt" 8
    !insertmacro PublishFile "BUILD-INFO.json" 16
    !insertmacro PublishFile "Uninstall.exe" 32
    !insertmacro PublishFile "install.ini" 64
    !insertmacro PublishFile "Zipelo.exe" 1
    ${If} $Upgrading == 1
        StrCpy $VersionChanged 1
        ClearErrors
        WriteRegStr HKCU "${UNKEY}" "DisplayVersion" "0.2.0"
        IfErrors failed
        Goto committed
    ${EndIf}
    StrCpy $ShortcutOwned 1
    CreateShortcut "$SMPROGRAMS\Zipelo.lnk" "$INSTDIR\Zipelo.exe"
    IfErrors failed
    StrCpy $UninstallOwned 1
    WriteRegStr HKCU "${UNKEY}" "DisplayName" "Zipelo"
    WriteRegStr HKCU "${UNKEY}" "DisplayIcon" '$\"$INSTDIR\Zipelo.exe$\",0'
    WriteRegStr HKCU "${UNKEY}" "DisplayVersion" "0.2.0"
    WriteRegStr HKCU "${UNKEY}" "UninstallString" '$\"$INSTDIR\Uninstall.exe$\"'
    IfErrors failed
committed:
    StrCpy $InstallComplete 1
    Call CleanupStage
    Goto done
cancelled:
    Call Rollback
    SetErrorLevel 2
    Abort "업데이트를 취소했습니다"
failed:
    Call Rollback
    SetErrorLevel 1
    MessageBox MB_OK|MB_ICONSTOP "설치를 완료하지 못했습니다. Zipelo와 압축 작업을 닫고 여유 공간과 쓰기 권한을 확인하세요. 복원하지 못한 파일이 있으면 상세 보기의 백업 경로를 보존하고 다시 설치하지 마세요." /SD IDOK
    Abort
 done:
SectionEnd

Function Rollback
    ${If} $InstallComplete == 1
        Return
    ${EndIf}
    ${If} $Upgrading == 1
        SetOutPath "$TEMP"
        !insertmacro RestoreFile "zipelo-archive-worker.exe" 2
        !insertmacro RestoreFile "LICENSE" 4
        !insertmacro RestoreFile "THIRD-PARTY-LICENSES.txt" 8
        !insertmacro RestoreFile "BUILD-INFO.json" 16
        !insertmacro RestoreFile "Uninstall.exe" 32
        !insertmacro RestoreFile "install.ini" 64
        !insertmacro RestoreFile "Zipelo.exe" 1
        ${If} $VersionChanged == 1
            ClearErrors
            WriteRegStr HKCU "${UNKEY}" "DisplayVersion" "$OldVersion"
            ${If} ${Errors}
                StrCpy $RestoreFailed 1
            ${Else}
                StrCpy $VersionChanged 0
            ${EndIf}
        ${EndIf}
        ${If} $RestoreFailed == 1
            DetailPrint "복원 실패: 백업 폴더 $StageDir\previous 를 보존하세요."
        ${EndIf}
        Call CleanupStage
        Return
    ${EndIf}
    Call CleanupStage
    ${If} $RootOwned != 1
        Return
    ${EndIf}
    SetOutPath "$TEMP"
    ${If} $ShortcutOwned == 1
        Delete "$SMPROGRAMS\Zipelo.lnk"
    ${EndIf}
    ${If} $UninstallOwned == 1
        DeleteRegValue HKCU "${UNKEY}" "DisplayIcon"
        DeleteRegValue HKCU "${UNKEY}" "DisplayName"
        DeleteRegValue HKCU "${UNKEY}" "DisplayVersion"
        DeleteRegValue HKCU "${UNKEY}" "UninstallString"
        DeleteRegKey /ifempty HKCU "${UNKEY}"
    ${EndIf}
    Delete "$INSTDIR\Zipelo.exe"
    Delete "$INSTDIR\zipelo-archive-worker.exe"
    Delete "$INSTDIR\LICENSE"
    Delete "$INSTDIR\THIRD-PARTY-LICENSES.txt"
    Delete "$INSTDIR\BUILD-INFO.json"
    Delete "$INSTDIR\install.ini"
    Delete "$INSTDIR\Uninstall.exe"
    RMDir "$INSTDIR"
    StrCpy $RootOwned 0
FunctionEnd

Function CleanupStage
    ${If} $StageDir == ""
        Return
    ${EndIf}
    SetOutPath "$TEMP"
    !insertmacro CleanupStageFile "Zipelo.exe"
    !insertmacro CleanupStageFile "zipelo-archive-worker.exe"
    !insertmacro CleanupStageFile "LICENSE"
    !insertmacro CleanupStageFile "THIRD-PARTY-LICENSES.txt"
    !insertmacro CleanupStageFile "BUILD-INFO.json"
    !insertmacro CleanupStageFile "Uninstall.exe"
    !insertmacro CleanupStageFile "install.ini"
    RMDir "$StageDir\previous"
    ClearErrors
    RMDir "$StageDir"
    ${If} ${Errors}
        DetailPrint "임시 파일 또는 복원 백업이 남아 있음: $StageDir"
    ${Else}
        StrCpy $StageDir ""
    ${EndIf}
FunctionEnd

Function .onInstFailed
    Call Rollback
    SetErrorLevel 1
FunctionEnd

Function FinishCreate
    GetDlgItem $0 $HWNDPARENT 1
    SendMessage $0 ${WM_SETTEXT} 0 "STR:마침"
    GetDlgItem $0 $HWNDPARENT 3
    EnableWindow $0 0
    nsDialogs::Create 1018
    Pop $Dialog
    ${If} $Dialog == error
        Abort
    ${EndIf}
    ${NSD_CreateLabel} 0 0 100% 28u "Zipelo 설치가 끝났습니다."
    Pop $0
    ${NSD_CreateLabel} 0 32u 100% 40u "[적용]을 누르면 Zipelo를 연결 프로그램으로 등록하고 Windows 기본 앱 설정을 엽니다. 기본 앱은 설정에서 직접 선택하세요."
    Pop $0
    ${NSD_CreateButton} 0 84u 100% 26u "적용"
    Pop $ApplyButton
    ${NSD_OnClick} $ApplyButton ApplyAssociations
    nsDialogs::Show
FunctionEnd

Function ApplyAssociations
    Pop $0
    ClearErrors
    WriteRegStr HKCU "${PROGKEY}" "" "Zipelo 압축파일"
    WriteRegStr HKCU "${PROGKEY}\DefaultIcon" "" '$\"$INSTDIR\Zipelo.exe$\",0'
    WriteRegStr HKCU "${PROGKEY}\shell\open\command" "" '$\"$INSTDIR\Zipelo.exe$\" $\"%1$\"'
    WriteRegStr HKCU "${APPKEY}\Capabilities" "ApplicationName" "Zipelo"
    WriteRegStr HKCU "${APPKEY}\Capabilities" "ApplicationDescription" "압축파일 열기와 풀기"
    WriteRegStr HKCU "${APPKEY}\Capabilities\FileAssociations" ".zip" "Zipelo.Archive"
    WriteRegStr HKCU "${APPKEY}\Capabilities\FileAssociations" ".7z" "Zipelo.Archive"
    WriteRegStr HKCU "${APPKEY}\Capabilities\FileAssociations" ".tar" "Zipelo.Archive"
    WriteRegStr HKCU "${APPKEY}\Capabilities\FileAssociations" ".gz" "Zipelo.Archive"
    WriteRegStr HKCU "${APPKEY}\Capabilities\FileAssociations" ".tgz" "Zipelo.Archive"
    WriteRegStr HKCU "Software\RegisteredApplications" "Zipelo" "${APPKEY}\Capabilities"
    IfErrors register_failed
    System::Call 'shell32::SHChangeNotify(i 0x8000000, i 0, p 0, p 0)'
    ExecShell "open" "ms-settings:defaultapps"
    IfErrors settings_failed
    Return
register_failed:
    MessageBox MB_OK|MB_ICONSTOP "연결 프로그램을 등록하지 못했습니다. 쓰기 권한을 확인한 뒤 다시 적용하세요."
    Return
settings_failed:
    MessageBox MB_OK "Windows 설정을 열지 못했습니다. 설정에서 앱 > 기본 앱을 직접 열어주세요."
FunctionEnd

; 현재 값이 설치 시 기록한 값과 같은 경우에만 지움.
!macro RemoveOwnedValue KEY NAME EXPECTED
    ReadRegStr $0 HKCU "${KEY}" "${NAME}"
    ${If} $0 S== "${EXPECTED}"
        DeleteRegValue HKCU "${KEY}" "${NAME}"
    ${EndIf}
!macroend

Function un.AssociationsCreate
    nsDialogs::Create 1018
    Pop $Dialog
    ${If} $Dialog == error
        Abort
    ${EndIf}
    ${NSD_CreateLabel} 0 0 100% 62u "Zipelo를 기본 앱으로 사용했다면 제거 전에 다른 앱을 선택하세요. [기본 앱 설정 열기]에서 .zip, .7z, .tar, .gz, .tgz를 검색해 사용할 앱을 고를 수 있습니다. 다른 앱이 없으면 먼저 설치하세요."
    Pop $0
    ${NSD_CreateLabel} 0 66u 100% 38u "직접 고른 기본 앱은 자동으로 바꾸지 않습니다. 제거 후 파일이 열리지 않으면 파일을 오른쪽 클릭하고 [연결 프로그램]에서 다른 앱을 선택하세요."
    Pop $0
    ${NSD_CreateButton} 0 112u 100% 24u "기본 앱 설정 열기"
    Pop $ApplyButton
    ${NSD_OnClick} $ApplyButton un.OpenDefaultApps
    nsDialogs::Show
FunctionEnd

Function un.OpenDefaultApps
    Pop $0
    ClearErrors
    ExecShell "open" "ms-settings:defaultapps"
    ${If} ${Errors}
        MessageBox MB_OK "Windows 설정을 열지 못했습니다. 설정 > 앱 > 기본 앱에서 확장자를 검색해 다른 앱을 선택하세요."
    ${EndIf}
FunctionEnd

Function un.onInit
    SetShellVarContext current
    SetRegView 64
    System::Call 'kernel32::SetLastError(i 0)'
    System::Call 'kernel32::CreateMutexW(p 0, i 0, w "Local\ZipeloSetup") p.r0 ?e'
    Pop $1
    StrCpy $Mutex $0
    ${If} $0 == 0
    ${OrIf} $1 != 0
        Abort
    ${EndIf}
    StrCmp $INSTDIR "${INSTALLROOT}" 0 invalid
    ReadINIStr $0 "$INSTDIR\install.ini" "Zipelo" "Location"
    StrCmp $0 $INSTDIR 0 invalid
    Return
invalid:
    MessageBox MB_OK|MB_ICONSTOP "설치 위치를 확인하지 못했습니다. 파일을 삭제하지 않습니다."
    Abort
FunctionEnd

Section "Uninstall"
    SetOutPath "$TEMP"
    ClearErrors
    Delete "$INSTDIR\Zipelo.exe"
    Delete "$INSTDIR\zipelo-archive-worker.exe"
    IfErrors uninstall_failed
    Delete "$INSTDIR\LICENSE"
    Delete "$INSTDIR\THIRD-PARTY-LICENSES.txt"
    Delete "$INSTDIR\BUILD-INFO.json"
    Delete "$SMPROGRAMS\Zipelo.lnk"
    IfErrors uninstall_failed
    ; 자신이 등록한 값만 제거하고 다른 파일과 키를 보존함.
    ReadRegStr $0 HKCU "Software\RegisteredApplications" "Zipelo"
    ${If} $0 == "${APPKEY}\Capabilities"
        DeleteRegValue HKCU "Software\RegisteredApplications" "Zipelo"
    ${EndIf}
    !insertmacro RemoveOwnedValue "${APPKEY}\Capabilities\FileAssociations" ".zip" "Zipelo.Archive"
    !insertmacro RemoveOwnedValue "${APPKEY}\Capabilities\FileAssociations" ".7z" "Zipelo.Archive"
    !insertmacro RemoveOwnedValue "${APPKEY}\Capabilities\FileAssociations" ".tar" "Zipelo.Archive"
    !insertmacro RemoveOwnedValue "${APPKEY}\Capabilities\FileAssociations" ".gz" "Zipelo.Archive"
    !insertmacro RemoveOwnedValue "${APPKEY}\Capabilities\FileAssociations" ".tgz" "Zipelo.Archive"
    !insertmacro RemoveOwnedValue "${APPKEY}\Capabilities" "ApplicationName" "Zipelo"
    !insertmacro RemoveOwnedValue "${APPKEY}\Capabilities" "ApplicationDescription" "압축파일 열기와 풀기"
    DeleteRegKey /ifempty HKCU "${APPKEY}\Capabilities\FileAssociations"
    DeleteRegKey /ifempty HKCU "${APPKEY}\Capabilities"
    DeleteRegKey /ifempty HKCU "${APPKEY}"
    !insertmacro RemoveOwnedValue "${PROGKEY}\shell\open\command" "" '$\"$INSTDIR\Zipelo.exe$\" $\"%1$\"'
    !insertmacro RemoveOwnedValue "${PROGKEY}\DefaultIcon" "" '$\"$INSTDIR\Zipelo.exe$\",0'
    DeleteRegKey /ifempty HKCU "${PROGKEY}\DefaultIcon"
    !insertmacro RemoveOwnedValue "${PROGKEY}" "" "Zipelo 압축파일"
    DeleteRegKey /ifempty HKCU "${PROGKEY}\shell\open\command"
    DeleteRegKey /ifempty HKCU "${PROGKEY}\shell\open"
    DeleteRegKey /ifempty HKCU "${PROGKEY}\shell"
    DeleteRegKey /ifempty HKCU "${PROGKEY}"
    DeleteRegValue HKCU "${UNKEY}" "DisplayIcon"
    DeleteRegValue HKCU "${UNKEY}" "DisplayName"
    DeleteRegValue HKCU "${UNKEY}" "DisplayVersion"
    DeleteRegValue HKCU "${UNKEY}" "UninstallString"
    DeleteRegKey /ifempty HKCU "${UNKEY}"
    Delete "$INSTDIR\install.ini"
    Delete "$INSTDIR\Uninstall.exe"
    RMDir "$INSTDIR"
    System::Call 'shell32::SHChangeNotify(i 0x8000000, i 0, p 0, p 0)'
    Goto un_done
uninstall_failed:
    SetErrorLevel 1
    MessageBox MB_OK|MB_ICONSTOP "Zipelo와 작업자를 닫은 뒤 제거 프로그램을 다시 실행하세요."
    Abort
un_done:
SectionEnd
