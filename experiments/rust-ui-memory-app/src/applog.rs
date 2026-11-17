//! 앱 로그. 표준 오류로 한 줄씩 남김.
//!
//! 디버그 빌드(debug_assertions)는 모든 단계(trace까지)를 자세히 남기고,
//! 배포(릴리스) 빌드는 오류만 남겨 로그를 최소화함. 문제를 조사할 때는
//! ZIPELO_LOG 환경 변수(off, error, warn, info, debug, trace)로 단계를 바꿀 수 있음.
//! 암호와 파일 내용은 어느 단계에서도 남기지 않음.

use std::sync::OnceLock;
use std::time::Instant;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Level {
    Off = 0,
    Error = 1,
    Warn = 2,
    Info = 3,
    Debug = 4,
    Trace = 5,
}

impl Level {
    fn label(self) -> &'static str {
        match self {
            Level::Off => "OFF",
            Level::Error => "ERROR",
            Level::Warn => "WARN",
            Level::Info => "INFO",
            Level::Debug => "DEBUG",
            Level::Trace => "TRACE",
        }
    }

    /// 빌드 종류에 따른 기본 단계.
    pub const fn default_for_build() -> Self {
        if cfg!(debug_assertions) {
            Level::Trace
        } else {
            Level::Error
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        Some(match text.trim().to_ascii_lowercase().as_str() {
            "off" | "none" | "0" => Level::Off,
            "error" | "1" => Level::Error,
            "warn" | "warning" | "2" => Level::Warn,
            "info" | "3" => Level::Info,
            "debug" | "4" => Level::Debug,
            "trace" | "all" | "5" => Level::Trace,
            _ => return None,
        })
    }
}

struct State {
    level: Level,
    started: Instant,
}

fn state() -> &'static State {
    static STATE: OnceLock<State> = OnceLock::new();
    STATE.get_or_init(|| State {
        level: std::env::var("ZIPELO_LOG")
            .ok()
            .and_then(|value| Level::parse(&value))
            .unwrap_or(Level::default_for_build()),
        started: Instant::now(),
    })
}

/// 현재 로그 단계.
pub fn level() -> Level {
    state().level
}

/// 이 단계의 로그를 남기는지. 메시지를 만드는 비용을 아낄 때 씀.
pub fn enabled(level: Level) -> bool {
    level != Level::Off && level <= state().level
}

#[doc(hidden)]
pub fn write(level: Level, target: &str, message: std::fmt::Arguments<'_>) {
    if !enabled(level) {
        return;
    }
    let elapsed = state().started.elapsed();
    let thread = std::thread::current();
    let line = format!(
        "[{:>5}.{:03}s {:<5} {target} {}] {message}\n",
        elapsed.as_secs(),
        elapsed.subsec_millis(),
        level.label(),
        thread.name().unwrap_or("-"),
    );
    // 표준 오류를 쓸 수 없어도(Windows GUI 등) 앱 동작에는 영향을 주지 않음.
    // eprint!는 시험 실행 중에는 시험별로 출력을 모아 실패한 시험만 보여 줌.
    if std::panic::catch_unwind(|| eprint!("{line}")).is_err() {
        // 표준 오류가 닫혀 있어 eprint!가 실패해도 무시함.
    }
}

#[macro_export]
macro_rules! log_at {
    ($level:expr, $($arg:tt)+) => {
        if $crate::applog::enabled($level) {
            $crate::applog::write($level, module_path!(), format_args!($($arg)+));
        }
    };
}

#[macro_export]
macro_rules! log_error { ($($arg:tt)+) => { $crate::log_at!($crate::applog::Level::Error, $($arg)+) }; }
#[macro_export]
macro_rules! log_warn { ($($arg:tt)+) => { $crate::log_at!($crate::applog::Level::Warn, $($arg)+) }; }
#[macro_export]
macro_rules! log_info { ($($arg:tt)+) => { $crate::log_at!($crate::applog::Level::Info, $($arg)+) }; }
#[macro_export]
macro_rules! log_debug { ($($arg:tt)+) => { $crate::log_at!($crate::applog::Level::Debug, $($arg)+) }; }
#[macro_export]
macro_rules! log_trace { ($($arg:tt)+) => { $crate::log_at!($crate::applog::Level::Trace, $($arg)+) }; }

/// Debug 표현을 최대 `max`자까지만 만들어 돌려줌. 큰 값(미리보기 데이터 등)을
/// 끝까지 문자열로 만들지 않도록 한도에 닿으면 바로 멈춤.
pub fn brief(value: &impl std::fmt::Debug, max: usize) -> String {
    struct Bounded {
        text: String,
        count: usize,
        max: usize,
        cut: bool,
    }
    impl std::fmt::Write for Bounded {
        fn write_str(&mut self, s: &str) -> std::fmt::Result {
            for c in s.chars() {
                if self.count >= self.max {
                    self.cut = true;
                    return Err(std::fmt::Error);
                }
                self.text.push(c);
                self.count += 1;
            }
            Ok(())
        }
    }
    let mut out = Bounded {
        text: String::new(),
        count: 0,
        max,
        cut: false,
    };
    let _ = std::fmt::Write::write_fmt(&mut out, format_args!("{value:?}"));
    if out.cut {
        out.text.push('…');
    }
    out.text
}

#[cfg(test)]
#[path = "../../../tests/rust-app-ui/applog.rs"]
mod tests;
