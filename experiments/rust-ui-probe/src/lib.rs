pub const ROW_COUNT: usize = 100_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Navigation {
    Previous,
    Next,
    First,
    Last,
    PageUp(usize),
    PageDown(usize),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Selection {
    len: usize,
    selected: Option<usize>,
}

impl Selection {
    pub fn new(len: usize) -> Self {
        Self {
            len,
            selected: None,
        }
    }

    pub fn selected(&self) -> Option<usize> {
        self.selected
    }

    pub fn select(&mut self, index: usize) -> bool {
        if index >= self.len {
            return false;
        }
        self.selected = Some(index);
        true
    }

    pub fn navigate(&mut self, key: Navigation) -> Option<usize> {
        if self.len == 0 {
            return None;
        }
        let current = self.selected.unwrap_or(0);
        let last = self.len - 1;
        self.selected = Some(match key {
            Navigation::First => 0,
            Navigation::Last => last,
            Navigation::Previous => current.saturating_sub(1),
            Navigation::Next if self.selected.is_none() => 0,
            Navigation::Next => current.saturating_add(1).min(last),
            Navigation::PageUp(n) => current.saturating_sub(n),
            Navigation::PageDown(n) => current.saturating_add(n).min(last),
        });
        self.selected
    }
}

pub fn row_id(index: usize) -> Option<u64> {
    if index >= ROW_COUNT {
        return None;
    }
    Some(index as u64 + 1)
}

pub fn row_name(index: usize) -> Option<String> {
    row_id(index).map(|id| format!("자료/{id:06}_한글 합성 문서.txt"))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Idle,
    Running,
    CancelRequested,
    MockCancelled,
    MockCompleted,
    MockFailed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModelError {
    Busy,
    EmptyWork,
    IdentityExhausted,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Job {
    serial: u64,
    phase: Phase,
    done: u64,
    total: u64,
}

impl Default for Job {
    fn default() -> Self {
        Self {
            serial: 0,
            phase: Phase::Idle,
            done: 0,
            total: 0,
        }
    }
}

impl Job {
    pub fn phase(&self) -> Phase {
        self.phase
    }
    pub fn id(&self) -> u64 {
        self.serial
    }
    pub fn done(&self) -> u64 {
        self.done
    }
    pub fn total(&self) -> u64 {
        self.total
    }
    pub fn busy(&self) -> bool {
        matches!(self.phase, Phase::Running | Phase::CancelRequested)
    }

    pub fn start(&mut self, total: u64) -> Result<u64, ModelError> {
        if self.busy() {
            return Err(ModelError::Busy);
        }
        if total == 0 {
            return Err(ModelError::EmptyWork);
        }
        let next = self
            .serial
            .checked_add(1)
            .ok_or(ModelError::IdentityExhausted)?;
        self.serial = next;
        self.done = 0;
        self.total = total;
        self.phase = Phase::Running;
        Ok(next)
    }

    pub fn progress(&mut self, id: u64, done: u64) -> bool {
        if id != self.serial
            || self.phase != Phase::Running
            || done < self.done
            || done > self.total
        {
            return false;
        }
        self.done = done;
        true
    }

    pub fn complete(&mut self, id: u64) -> bool {
        if id != self.serial || self.phase != Phase::Running || self.done != self.total {
            return false;
        }
        self.phase = Phase::MockCompleted;
        true
    }

    pub fn request_cancel(&mut self) -> bool {
        if self.phase != Phase::Running {
            return false;
        }
        self.phase = Phase::CancelRequested;
        true
    }

    pub fn acknowledge_mock_cancel(&mut self, id: u64) -> bool {
        if id != self.serial || self.phase != Phase::CancelRequested {
            return false;
        }
        self.phase = Phase::MockCancelled;
        true
    }

    pub fn fail(&mut self, id: u64) -> bool {
        if id != self.serial || !self.busy() {
            return false;
        }
        self.phase = Phase::MockFailed;
        true
    }
}
