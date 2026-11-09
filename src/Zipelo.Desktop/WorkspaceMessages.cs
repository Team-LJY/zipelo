using Zipelo.Core;

namespace Zipelo.Desktop;

internal static class WorkspaceMessages
{
    public static string Error(ArchiveError code) => code switch
    {
        ArchiveError.EngineUnavailable => "작업 프로세스를 시작할 수 없습니다. Zipelo 폴더 전체를 확인한 뒤 다시 실행하세요.",
        ArchiveError.EngineFailure => "작업 프로세스가 정상 결과를 반환하지 못했습니다. ZIP 파일 상태와 Zipelo 폴더를 확인한 뒤 다시 여세요.",
        ArchiveError.UnsupportedOperation => "이 형식 또는 기능은 현재 연결된 엔진에서 지원하지 않습니다.",
        ArchiveError.UnsafePath => "안전하게 저장할 수 없는 파일 경로가 포함되어 작업을 중단했습니다.",
        ArchiveError.UnsupportedEntry => "현재 처리할 수 없는 파일 항목이 포함되어 있습니다.",
        ArchiveError.OutputUnavailable => "저장 또는 정리를 완료하지 못했습니다. 결과 위치를 확인하세요.",
        ArchiveError.OutputConflict => "저장할 이름이 기존 파일과 충돌합니다. 다른 이름이나 위치를 선택하세요.",
        ArchiveError.BudgetExceeded => "안전한 처리 한도를 초과했습니다. 더 작은 파일로 다시 시도하세요.",
        ArchiveError.PasswordAttemptsExceeded => "암호 입력 횟수 한도에 도달했습니다. 암호를 확인한 뒤 다시 여세요.",
        ArchiveError.InvalidPasswordResponse => "암호 요청을 연결하지 못했습니다. 파일을 다시 여세요.",
        ArchiveError.IntegrityFailure => "일부 데이터가 검증을 통과하지 못했습니다. 정상 파일로 간주하지 마세요.",
        ArchiveError.Cancelled => "중지 후 정리를 완료하지 못했습니다. 결과 위치를 확인하세요.",
        _ => "작업에 실패했습니다. 파일 상태와 저장 위치를 확인하고 다시 시도하세요."
    };
}
