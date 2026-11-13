# Linux 게시 부모 동일성

> 과거 C# 구현 기록: C#은 v0.1에서 중단하고 Rust로 대체했다. PR103(f1d9fa5)에서 C# 소스와 전용 시험 및 .NET 설정을 제거했다. 아래 본문의 "현재", 미구현 상태, API와 검증 결과는 작성 당시 C# 스냅샷에 관한 내용이며, 현재 Rust 제품의 지원 범위나 검증 상태를 뜻하지 않는다. 삭제된 경로와 실행 명령은 현재 main에서 사용할 수 없다.
>
> 현재 기준은 [기획서](PLANS.md), [UI 설계](ui-design-v0.1.md), [릴리스 노트](release-notes-v0.1.md)다. 앱은 [Rust UI](../experiments/rust-ui-memory-app/), 압축 처리는 [Rust 작업자](../experiments/rust-archive-worker/README.md)를 사용한다. 과거 검사 재현과 소스 보존 안내는 [기획서의 과거 기록](PLANS.md#7-c-중단과-과거-검증-기록)을 따른다.

## 보존된 당시 기록

기준 main: `626483e8935aaa03d5a34feebc61be2f9180b80d`.
수정 범위는 공용 `LinuxFolderPublication`과 이 문서임.
v1 공개 API, Windows 구현, PR #10 스트리밍 제품 파일, UI, 테스트는 변경하지 않음.
PR #10과 결합하면 같은 공용 구현을 사용하는 v2에도 적용됨.

## 게시 전 검사

기존 구현은 부모 디렉터리를 연 fd에 대해 device/inode와 소유권/권한을 검사했음.
부모를 다른 이름으로 옮겨도 열린 fd는 같은 객체를 계속 가리키므로, 기존 검사는 원래 경로가 교체된 사실을 검출하지 못했음.

초기 ext4/XFS 및 mountinfo 검증을 통과한 부모의 mount ID와 기존 device/inode를 보관함.
모든 출력 디렉터리의 fsync가 끝난 뒤, 최종 rename 직전에 처음 받은 절대 부모 경로를
`openat2(O_RDONLY | O_DIRECTORY | O_CLOEXEC, RESOLVE_NO_SYMLINKS)`로 다시 엶.
심볼릭 링크와 magic link를 모든 경로 구성요소에서 거절하며 문자열 경로 비교나 링크 추적 대체 경로를 쓰지 않음.

고정 부모와 재개방 부모의 statx device/inode/mount ID를 초기 값과 비교하고,
양쪽의 디렉터리 종류, 현재 사용자 소유권, group/other 쓰기 금지와 파일시스템 magic을 재검사함.
statx mount ID 지원/값이 없으면 기존 방식대로 거절함.
mountinfo와 같은 일반 mount ID를 사용하며 고정 부모 fd는 작업 동안 유지됨.

경로가 이미 사라졌거나 디렉터리가 아니거나 링크가 되면 open 실패로 게시하지 않음.
다른 inode/device/mount 또는 보호 조건 불일치는 게시 전에 거절함.
동일성 불일치는 UnsafePath, open/stat 오류는 기존 네이티브 오류 매핑을 유지함.
기존 부모가 이동된 상태에서도 이번에 만든 임시 트리 정리는 고정 부모 fd 기준으로 수행하므로,
대체 부모나 외부 sentinel을 경로 기반으로 삭제하지 않음. 정리 실패 보고도 유지함.

최종 게시에는 원래 고정 fd와 RENAME_NOREPLACE를 계속 사용함.
재개방 핸들을 새 게시 대상으로 전환하지 않으며, 기존 대상 덮어쓰기나 권한 변경을 하지 않음.

## 보장 한계

이 검사는 재개방 시점에 이미 완료되어 유지되는 부모 경로 교체를 검출함.
동일성 검사와 rename은 별도 syscall이므로 원자적 경로 고정 또는 경합 완전 차단이 아님.
같은 사용자 권한으로 상위 디렉터리를 수정할 수 있는 프로세스가 검사 이후 부모나 조상을 이동하면,
고정 fd가 가리키는 이동된 부모에 게시될 가능성은 여전히 남음.
교체 후 검사 전에 원래 상태로 복원하는 이동 이력도 검출하지 않음.
0700 디렉터리와 fd 보유는 같은 사용자 프로세스의 rename을 막는 잠금이 아님.

따라서 성공은 검사 시점의 부모 동일성 및 고정 객체에서의 덮어쓰기 금지 게시를 뜻하며,
반환 시점이나 그 이후에도 사용자 문자열 경로가 같은 객체를 가리킨다는 보장이 아님.
강한 경로 안정성이 필요하면 호출 환경에서 동일 사용자/권한 보유자의 동시 이름 공간 변경을 배제해야 함.
이 수정은 권한을 바꾸거나 OS 격리를 도입하지 않으며, 게시 후 경로 기반 롤백으로 경합을 숨기지 않음.

## 검증 상태와 수용 기준

로컬 SDK 부재로 수정한 C# 컴파일과 실행은 미검증임.
정적 검토와 원격 내용 대조만 실행 검증과 분리해 보고함.
독립 PR #12에서 기존 부모 교체 실패 기대값을 유지하며 다음 항목을 실제로 확인해야 함.

- v1 공용 게시 및 PR #10 v2에서 게시 검사 전에 부모 이동/대체를 완료하면 오류, 양쪽 경로 미게시, 외부 sentinel 보존.
- 부모 삭제, 부모 또는 조상의 링크 대체, 다른 디렉터리 대체를 거절함.
- 정상 경로, 기존 대상 충돌, 취소, 고정 핸들 기반 소유 임시 트리 정리와 정리 실패 보고.
- 기존 144개 회귀와 Windows 경로의 비변경 확인.
- 검사와 rename 사이의 결정적 경합 및 mount 교체는 별도 검증 항목이며 완료로 간주하지 않음.

## 시스템 호출 근거

- [openat2](https://man7.org/linux/man-pages/man2/openat2.2.html): RESOLVE_NO_SYMLINKS의 전체 경로 구성요소 적용 및 magic link 차단.
- [statx](https://man7.org/linux/man-pages/man2/statx.2.html): fd 대상 조회와 device/inode/mount ID.
- [rename](https://man7.org/linux/man-pages/man2/rename.2.html): 열린 fd 유지, dirfd 상대 게시와 RENAME_NOREPLACE.
