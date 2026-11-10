use crate::{
    core,
    runtime::{Action, Completion, DEFAULT_CAPACITY, Outcome, Task},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CloseState {
    Open,
    Confirming,
    Waiting,
    Allowed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Idle,
    Running,
    CancelRequested,
    Completed,
    Cancelled,
    NativeFailure,
    DeliveryFailure,
    Panicked,
}

pub struct Controller {
    pub task: Option<Task>,
    receiver: Option<core::Receiver>,
    pub terminal: Option<Completion>,
    pub terminal_delivery: Option<core::Delivery>,
    terminal_accepted: bool,
    pub close: CloseState,
    pub status: Status,
    pub generation: u64,
    pub progress: Option<core::Progress>,
    pub delivery_error: Option<core::DeliveryError>,
    pub selected: Option<(core::OperationId, u64, usize)>,
    pub spawn_error: Option<crate::runtime::StartFailure>,
    pub panic: Option<Box<dyn std::any::Any + Send>>,
}

impl Default for Controller {
    fn default() -> Self {
        Self {
            task: None,
            receiver: None,
            terminal: None,
            terminal_delivery: None,
            terminal_accepted: false,
            close: CloseState::Open,
            status: Status::Idle,
            generation: 0,
            progress: None,
            delivery_error: None,
            selected: None,
            spawn_error: None,
            panic: None,
        }
    }
}

impl Controller {
    pub fn busy(&self) -> bool {
        self.task.as_ref().is_some_and(|task| !task.joined())
    }
    pub fn start(&mut self, action: Action) -> bool {
        self.start_with(action, DEFAULT_CAPACITY)
    }
    pub fn start_with(&mut self, action: Action, capacity: usize) -> bool {
        if self.busy() || self.close != CloseState::Open {
            return false;
        }
        let Some(generation) = self.generation.checked_add(1) else {
            return false;
        };
        let task = match Task::spawn(action, capacity) {
            Ok(task) => task,
            Err(error) => {
                self.spawn_error = Some(error);
                return false;
            }
        };
        self.receiver = Some(core::Receiver::new(task.id));
        self.task = Some(task);
        self.generation = generation;
        self.selected = None;
        self.terminal = None;
        self.terminal_delivery = None;
        self.terminal_accepted = false;
        self.delivery_error = None;
        self.panic = None;
        self.spawn_error = None;
        self.progress = None;
        self.status = Status::Running;
        true
    }
    pub fn cancel(&mut self) -> bool {
        if !self.busy() {
            return false;
        }
        let first = self.task.as_ref().unwrap().cancellation.request();
        if self.status == Status::Running {
            self.status = Status::CancelRequested;
        }
        first
    }
    pub fn request_close(&mut self) {
        if self.close == CloseState::Open {
            self.close = if self.busy() {
                CloseState::Confirming
            } else {
                CloseState::Allowed
            };
        }
    }
    pub fn keep_open(&mut self) {
        if self.close != CloseState::Allowed {
            self.close = CloseState::Open;
        }
    }
    pub fn confirm_close(&mut self) {
        if self.close == CloseState::Confirming {
            self.close = if self.busy() {
                CloseState::Waiting
            } else {
                CloseState::Allowed
            };
            self.cancel();
        }
    }
    pub fn poll(&mut self) {
        let Some(task) = self.task.as_mut() else {
            return;
        };
        if let Some(events) = &task.events {
            // 기본 큐 한도만큼만 처리하며 frame에서 수신을 기다리지 않음.
            for event in events.try_iter().take(DEFAULT_CAPACITY) {
                match event.validate(self.receiver.as_mut().unwrap()) {
                    Ok(core::Delivery::Accepted) => {
                        self.progress = event.progress().or(self.progress)
                    }
                    Ok(core::Delivery::IgnoredStale) => {}
                    Err(error) => {
                        self.delivery_error.get_or_insert(error);
                        task.cancellation.request();
                    }
                }
            }
        }
        if self.terminal.is_none() {
            if let Some(done) = &task.completion {
                self.terminal = done.try_recv().ok();
            }
        }
        if let Some(join) = task.poll_join() {
            match join {
                Ok(Some(completion)) => self.terminal = Some(completion),
                Ok(None) => {
                    if self.terminal.is_none() {
                        if let Some(done) = &task.completion {
                            self.terminal = done.try_recv().ok();
                        }
                    }
                }
                Err(panic) => {
                    self.panic = Some(panic);
                    self.status = Status::Panicked;
                }
            }
            if let Some(events) = &task.events {
                for event in events.try_iter() {
                    match event.validate(self.receiver.as_mut().unwrap()) {
                        Ok(core::Delivery::Accepted) => {
                            self.progress = event.progress().or(self.progress)
                        }
                        Ok(core::Delivery::IgnoredStale) => {}
                        Err(error) => {
                            self.delivery_error.get_or_insert(error);
                        }
                    }
                }
            }
            if let Some(completion) = &self.terminal {
                self.terminal_accepted = false;
                match self
                    .receiver
                    .as_mut()
                    .map(|receiver| completion.outcome.validate(receiver))
                {
                    Some(Ok(core::Delivery::Accepted)) => {
                        self.terminal_delivery = Some(core::Delivery::Accepted);
                        self.terminal_accepted = true;
                    }
                    Some(Ok(core::Delivery::IgnoredStale)) => {
                        self.terminal_delivery = Some(core::Delivery::IgnoredStale);
                    }
                    Some(Err(error)) => {
                        self.delivery_error.get_or_insert(error);
                    }
                    None => {}
                }
                self.status = if self.panic.is_some() {
                    Status::Panicked
                } else if !self.terminal_accepted
                    || completion.delivery.is_some()
                    || self.delivery_error.is_some()
                {
                    Status::DeliveryFailure
                } else if completion.outcome.is_ok() {
                    Status::Completed
                } else if completion.outcome.cancelled() {
                    Status::Cancelled
                } else {
                    Status::NativeFailure
                };
            } else if self.panic.is_none() {
                self.status = Status::DeliveryFailure;
            }
            if self.close == CloseState::Waiting {
                self.close = CloseState::Allowed;
            }
        }
    }
    pub fn rows(&self) -> Vec<&str> {
        if self.status != Status::Completed || !self.terminal_accepted {
            return Vec::new();
        }
        match self.terminal.as_ref().map(|c| &c.outcome) {
            Some(Outcome::Ustar(t)) => t
                .result()
                .map(|c| c.value().entries.iter().map(|e| e.name.as_str()).collect())
                .unwrap_or_default(),
            Some(Outcome::Zip(_)) => vec!["생성한 sample.zip"],
            Some(Outcome::Gzip(_)) => vec!["검증한 GZip 메모리 내용"],
            None => Vec::new(),
        }
    }
    pub fn select(&mut self, operation: core::OperationId, generation: u64, index: usize) -> bool {
        if self.task.as_ref().map(|t| t.id) != Some(operation)
            || generation != self.generation
            || index >= self.rows().len()
        {
            return false;
        }
        self.selected = Some((operation, generation, index));
        true
    }
    pub fn selected_bytes(&self) -> Option<&[u8]> {
        if self.status != Status::Completed || !self.terminal_accepted {
            return None;
        }
        let (operation, generation, index) = self.selected?;
        if self.task.as_ref()?.id != operation || generation != self.generation {
            return None;
        }
        match &self.terminal.as_ref()?.outcome {
            Outcome::Zip(t) => t.result().ok().map(|v| v.value().as_slice()),
            Outcome::Gzip(t) => t.result().ok()?.value().bytes.as_deref(),
            Outcome::Ustar(t) => {
                let value = t.result().ok()?.value();
                let id = &value.entries.get(index)?.id;
                value
                    .files
                    .iter()
                    .find(|f| &f.id == id)
                    .map(|f| f.bytes.as_slice())
            }
        }
    }
    pub fn preview(&self) -> String {
        let Some(bytes) = self.selected_bytes() else {
            return "목록에서 결과를 선택하세요".into();
        };
        if let Ok(text) = std::str::from_utf8(bytes) {
            text.chars().take(2048).collect()
        } else {
            format!("바이너리 {} bytes", bytes.len())
        }
    }
    pub fn status_text(&self) -> &'static str {
        match self.status {
            Status::Idle => "합성 작업을 선택하세요",
            Status::Running => "메모리 처리 중",
            Status::CancelRequested => "취소 요청됨 / 반환과 종료 대기",
            Status::Completed => "메모리 처리 완료",
            Status::Cancelled => "작업이 취소 결과를 반환함",
            Status::NativeFailure => "메모리 작업 실패",
            Status::DeliveryFailure => "UI 전달 실패 / 입력 파일 손상이나 사용자 취소가 아님",
            Status::Panicked => "작업 스레드 panic / 완료 결과 없음",
        }
    }
}
