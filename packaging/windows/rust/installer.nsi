Unicode true
!include "MUI2.nsh"
!include "nsDialogs.nsh"
!include "LogicLib.nsh"
!include "x64.nsh"
!include "FileFunc.nsh"
Name "Zipelo 개발판"
OutFile "Zipelo-0.1.0-dev-windows-x64-setup.exe"
!define INSTALLROOT "$LOCALAPPDATA\Programs\Zipelo-Dev"
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
!define APPKEY "Software\Zipelo-Dev"
!define PROGKEY "Software\Classes\ZipeloDev.Archive"
!define UNKEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\Zipelo-Dev"

Function .onInit
    ${IfNot} ${RunningX64}
        MessageBox MB_OK|MB_ICONSTOP "64비트 Windows가 필요합니다."
        Abort
    ${EndIf}
    SetShellVarContext current
    SetRegView 64
    System::Call 'kernel32::SetLastError(i 0)'
    System::Call 'kernel32::CreateMutexW(p 0, i 0, w "Local\ZipeloDevSetup") p.r0 ?e'
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
    ; 기존 설치와 등록을 덮어쓰지 않음.
    IfFileExists "$INSTDIR" failed
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
        System::Call 'advapi32::RegQueryValueExW(p r1, w "Zipelo 개발판", p 0, p 0, p 0, p 0) i.r2'
        System::Call 'advapi32::RegCloseKey(p r1)'
        ${If} $2 != 2
            Goto failed
        ${EndIf}
    ${ElseIf} $0 != 2
        Goto failed
    ${EndIf}
    IfFileExists "$SMPROGRAMS\Zipelo 개발판.lnk" failed
    ClearErrors
    CreateDirectory "$LOCALAPPDATA\Programs"
    IfErrors failed
    System::Call 'kernel32::CreateDirectoryW(w "$INSTDIR", p 0) i.r0'
    IntCmp $0 0 failed
    StrCpy $RootOwned 1
    SetOutPath "$INSTDIR"
    ClearErrors
    File "payload\Zipelo.exe"
    File "payload\zipelo-archive-worker.exe"
    File "payload\LICENSE"
    File "payload\THIRD-PARTY-LICENSES.txt"
    File "payload\BUILD-INFO.json"
    IfErrors failed
    WriteUninstaller "$INSTDIR\Uninstall.exe"
    IfErrors failed
    WriteINIStr "$INSTDIR\install.ini" "Zipelo" "Location" "$INSTDIR"
    IfErrors failed
    StrCpy $ShortcutOwned 1
    CreateShortcut "$SMPROGRAMS\Zipelo 개발판.lnk" "$INSTDIR\Zipelo.exe"
    IfErrors failed
    StrCpy $UninstallOwned 1
    WriteRegStr HKCU "${UNKEY}" "DisplayName" "Zipelo 개발판"
    WriteRegStr HKCU "${UNKEY}" "DisplayIcon" '$\"$INSTDIR\Zipelo.exe$\",0'
    WriteRegStr HKCU "${UNKEY}" "DisplayVersion" "0.1.0-dev"
    WriteRegStr HKCU "${UNKEY}" "UninstallString" '$\"$INSTDIR\Uninstall.exe$\"'
    IfErrors failed
    StrCpy $InstallComplete 1
    Goto done
failed:
    Call Rollback
    SetErrorLevel 1
    MessageBox MB_OK|MB_ICONSTOP "설치를 완료하지 못했습니다. 기존 Zipelo 개발판이 있으면 먼저 제거하고, 여유 공간과 쓰기 권한을 확인하세요. 이번 설치 항목을 정리했으며, 사용 중이라 지우지 못한 항목은 상세 보기에서 확인하세요."
    Abort
 done:
SectionEnd

Function Rollback
    ${If} $InstallComplete == 1
    ${OrIf} $RootOwned != 1
        Return
    ${EndIf}
    SetOutPath "$TEMP"
    ${If} $ShortcutOwned == 1
        Delete "$SMPROGRAMS\Zipelo 개발판.lnk"
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
    WriteRegStr HKCU "${APPKEY}\Capabilities" "ApplicationName" "Zipelo 개발판"
    WriteRegStr HKCU "${APPKEY}\Capabilities" "ApplicationDescription" "압축파일 열기와 풀기"
    WriteRegStr HKCU "${APPKEY}\Capabilities\FileAssociations" ".zip" "ZipeloDev.Archive"
    WriteRegStr HKCU "${APPKEY}\Capabilities\FileAssociations" ".7z" "ZipeloDev.Archive"
    WriteRegStr HKCU "${APPKEY}\Capabilities\FileAssociations" ".tar" "ZipeloDev.Archive"
    WriteRegStr HKCU "${APPKEY}\Capabilities\FileAssociations" ".gz" "ZipeloDev.Archive"
    WriteRegStr HKCU "${APPKEY}\Capabilities\FileAssociations" ".tgz" "ZipeloDev.Archive"
    WriteRegStr HKCU "Software\RegisteredApplications" "Zipelo 개발판" "${APPKEY}\Capabilities"
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

Function un.onInit
    SetShellVarContext current
    SetRegView 64
    System::Call 'kernel32::SetLastError(i 0)'
    System::Call 'kernel32::CreateMutexW(p 0, i 0, w "Local\ZipeloDevSetup") p.r0 ?e'
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
    Delete "$SMPROGRAMS\Zipelo 개발판.lnk"
    IfErrors uninstall_failed
    ; 자신이 등록한 값만 제거하고 다른 파일과 키를 보존함.
    ReadRegStr $0 HKCU "Software\RegisteredApplications" "Zipelo 개발판"
    ${If} $0 == "${APPKEY}\Capabilities"
        DeleteRegValue HKCU "Software\RegisteredApplications" "Zipelo 개발판"
    ${EndIf}
    DeleteRegValue HKCU "${APPKEY}\Capabilities\FileAssociations" ".zip"
    DeleteRegValue HKCU "${APPKEY}\Capabilities\FileAssociations" ".7z"
    DeleteRegValue HKCU "${APPKEY}\Capabilities\FileAssociations" ".tar"
    DeleteRegValue HKCU "${APPKEY}\Capabilities\FileAssociations" ".gz"
    DeleteRegValue HKCU "${APPKEY}\Capabilities\FileAssociations" ".tgz"
    DeleteRegValue HKCU "${APPKEY}\Capabilities" "ApplicationName"
    DeleteRegValue HKCU "${APPKEY}\Capabilities" "ApplicationDescription"
    DeleteRegKey /ifempty HKCU "${APPKEY}\Capabilities\FileAssociations"
    DeleteRegKey /ifempty HKCU "${APPKEY}\Capabilities"
    DeleteRegKey /ifempty HKCU "${APPKEY}"
    DeleteRegValue HKCU "${PROGKEY}\shell\open\command" ""
    DeleteRegValue HKCU "${PROGKEY}\DefaultIcon" ""
    DeleteRegKey /ifempty HKCU "${PROGKEY}\DefaultIcon"
    DeleteRegValue HKCU "${PROGKEY}" ""
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
