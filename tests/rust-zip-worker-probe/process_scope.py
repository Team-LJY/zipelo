import os
from pathlib import Path
import signal
import subprocess
import time


class ProcessScope:
    def __init__(self):
        self.job = None
        self.process = None
        self.sampled_peak_rss = 0
        self.memory_observed = False
        if os.name == "nt":
            self._windows_init()
        elif not Path("/proc/self/stat").exists():
            raise RuntimeError("Linux /proc 기반 소유 프로세스 확인이 필요함")

    def _windows_init(self):
        import ctypes
        from ctypes import wintypes

        self.ctypes = ctypes
        self.kernel = ctypes.WinDLL("kernel32", use_last_error=True)
        self.psapi = ctypes.WinDLL("psapi", use_last_error=True)
        self.kernel.CreateJobObjectW.argtypes = [ctypes.c_void_p, wintypes.LPCWSTR]
        self.kernel.CreateJobObjectW.restype = wintypes.HANDLE
        self.kernel.SetInformationJobObject.argtypes = [wintypes.HANDLE, ctypes.c_int, ctypes.c_void_p, wintypes.DWORD]
        self.kernel.QueryInformationJobObject.argtypes = [
            wintypes.HANDLE, ctypes.c_int, ctypes.c_void_p, wintypes.DWORD, ctypes.c_void_p
        ]
        self.kernel.AssignProcessToJobObject.argtypes = [wintypes.HANDLE, wintypes.HANDLE]
        self.kernel.TerminateJobObject.argtypes = [wintypes.HANDLE, wintypes.UINT]
        self.kernel.CloseHandle.argtypes = [wintypes.HANDLE]
        self.kernel.OpenProcess.argtypes = [wintypes.DWORD, wintypes.BOOL, wintypes.DWORD]
        self.kernel.OpenProcess.restype = wintypes.HANDLE

        class BasicLimits(ctypes.Structure):
            _fields_ = [
                ("per_process", ctypes.c_int64), ("per_job", ctypes.c_int64),
                ("flags", wintypes.DWORD), ("min_working_set", ctypes.c_size_t),
                ("max_working_set", ctypes.c_size_t), ("active_processes", wintypes.DWORD),
                ("affinity", ctypes.c_size_t), ("priority", wintypes.DWORD),
                ("scheduling", wintypes.DWORD),
            ]

        class IoCounters(ctypes.Structure):
            _fields_ = [(name, ctypes.c_uint64) for name in
                        ("read_ops", "write_ops", "other_ops", "read_bytes", "write_bytes", "other_bytes")]

        class ExtendedLimits(ctypes.Structure):
            _fields_ = [
                ("basic", BasicLimits), ("io", IoCounters),
                ("process_memory", ctypes.c_size_t), ("job_memory", ctypes.c_size_t),
                ("peak_process", ctypes.c_size_t), ("peak_job", ctypes.c_size_t),
            ]

        class MemoryCounters(ctypes.Structure):
            _fields_ = [("cb", wintypes.DWORD), ("faults", wintypes.DWORD)] + [
                (name, ctypes.c_size_t) for name in
                ("peak_rss", "rss", "peak_paged", "paged", "peak_nonpaged", "nonpaged",
                 "pagefile", "peak_pagefile", "private")
            ]

        self.MemoryCounters = MemoryCounters
        self.psapi.GetProcessMemoryInfo.argtypes = [wintypes.HANDLE, ctypes.c_void_p, wintypes.DWORD]
        self.job = self.kernel.CreateJobObjectW(None, None)
        if not self.job:
            raise OSError("시험 Job 생성 실패")
        limits = ExtendedLimits()
        limits.basic.flags = 0x2000
        if not self.kernel.SetInformationJobObject(self.job, 9, ctypes.byref(limits), ctypes.sizeof(limits)):
            self.close()
            raise OSError("시험 Job 종료 정책 설정 실패")

    def start(self, command, cwd):
        self.process = subprocess.Popen(
            command, cwd=cwd, stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL, start_new_session=os.name != "nt"
        )
        if self.job is not None:
            if not self.kernel.AssignProcessToJobObject(self.job, int(self.process._handle)):
                self.process.kill()
                self.process.wait(timeout=5)
                raise OSError("시험 Job 할당 실패")
        return self.process

    def members(self):
        if self.process is None:
            return []
        if self.job is not None:
            buffer = self.ctypes.create_string_buffer(8 + 256 * self.ctypes.sizeof(self.ctypes.c_size_t))
            if not self.kernel.QueryInformationJobObject(self.job, 3, buffer, len(buffer), None):
                raise OSError("소유 Job 프로세스 조회 실패")
            assigned = self.ctypes.c_uint32.from_buffer(buffer, 0).value
            listed = self.ctypes.c_uint32.from_buffer(buffer, 4).value
            if assigned > 256 or listed > 256 or assigned != listed:
                raise RuntimeError("소유 프로세스 조회 한도 초과")
            return [
                self.ctypes.c_size_t.from_buffer(buffer, 8 + i * self.ctypes.sizeof(self.ctypes.c_size_t)).value
                for i in range(listed)
            ]
        members = []
        for path in Path("/proc").iterdir():
            if not path.name.isdecimal():
                continue
            try:
                text = (path / "stat").read_text()
                fields = text[text.rindex(")") + 2:].split()
                if int(fields[3]) == self.process.pid:
                    members.append(int(path.name))
            except FileNotFoundError:
                continue
            except ProcessLookupError:
                continue
        return members

    def sample(self):
        total = 0
        measured = False
        for pid in self.members():
            if self.job is not None:
                handle = self.kernel.OpenProcess(0x0410, False, pid)
                if not handle:
                    continue
                try:
                    counters = self.MemoryCounters()
                    counters.cb = self.ctypes.sizeof(counters)
                    if self.psapi.GetProcessMemoryInfo(handle, self.ctypes.byref(counters), counters.cb):
                        total += counters.rss
                        measured = True
                finally:
                    self.kernel.CloseHandle(handle)
            else:
                try:
                    total += int(Path("/proc", str(pid), "statm").read_text().split()[1]) * os.sysconf("SC_PAGE_SIZE")
                    measured = True
                except (FileNotFoundError, ProcessLookupError):
                    pass
        self.memory_observed |= measured
        self.sampled_peak_rss = max(self.sampled_peak_rss, total)

    def terminate(self):
        if self.process is None:
            return
        if self.job is not None:
            if not self.kernel.TerminateJobObject(self.job, 124):
                raise OSError("소유 Job 종료 요청 실패")
        else:
            # 이 실행에서 만든 새 세션의 프로세스 그룹만 종료함.
            groups = set()
            for pid in self.members():
                try:
                    groups.add(os.getpgid(pid))
                except ProcessLookupError:
                    pass
            for group in groups:
                try:
                    os.killpg(group, signal.SIGKILL)
                except ProcessLookupError:
                    pass

    def empty(self):
        if self.process is not None:
            self.process.poll()
        return not self.members()

    def close(self):
        if self.job is not None:
            self.kernel.CloseHandle(self.job)
            self.job = None


def bounded(command, cwd, gate, seconds=75):
    scope = ProcessScope()
    started = time.monotonic()
    timed_out = False
    exited = False
    forced = False
    try:
        process = scope.start(command, cwd)
        with Path(gate).open("x", encoding="ascii") as output:
            output.write("1")
        while process.poll() is None and time.monotonic() - started < seconds:
            scope.sample()
            time.sleep(0.05)
        timed_out = process.poll() is None
        if timed_out or not scope.empty():
            forced = True
            scope.terminate()
        stop_deadline = time.monotonic() + 5
        while time.monotonic() < stop_deadline and not scope.empty():
            time.sleep(0.05)
        exited = scope.empty()
        return {
            "exitCode": process.poll(),
            "externalTimeout": timed_out,
            "forcedTermination": forced,
            "ownedProcessesExited": exited,
            "elapsedSeconds": time.monotonic() - started,
            "sampledPeakRssBytes": scope.sampled_peak_rss if scope.memory_observed else None,
            "memoryMetric": "sum_of_concurrent_process_rss_sampled_50ms",
        }
    finally:
        try:
            if not exited:
                scope.terminate()
                stop_deadline = time.monotonic() + 5
                while time.monotonic() < stop_deadline and not scope.empty():
                    time.sleep(0.05)
                if not scope.empty():
                    raise RuntimeError("소유 프로세스 종료 미확인")
        finally:
            scope.close()
