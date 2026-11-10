use std::sync::mpsc::{SyncSender, TrySendError};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeliveryKind {
    Full,
    Disconnected,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeliveryFault {
    pub kind: DeliveryKind,
    pub first_missing_sequence: u64,
}

pub struct Transport<T> {
    sender: SyncSender<T>,
    pub fault: Option<DeliveryFault>,
}

impl<T> Transport<T> {
    pub fn new(sender: SyncSender<T>) -> Self {
        Self {
            sender,
            fault: None,
        }
    }

    pub fn forward<E>(
        &mut self,
        sequence: u64,
        value: T,
        original: Result<(), E>,
        stop: impl FnOnce() -> E,
    ) -> Result<(), E> {
        if self.fault.is_none() {
            self.fault = match self.sender.try_send(value) {
                Ok(()) => None,
                Err(TrySendError::Full(_)) => Some(DeliveryFault {
                    kind: DeliveryKind::Full,
                    first_missing_sequence: sequence,
                }),
                Err(TrySendError::Disconnected(_)) => Some(DeliveryFault {
                    kind: DeliveryKind::Disconnected,
                    first_missing_sequence: sequence,
                }),
            };
        }
        // 원래 observer 오류를 전달 오류보다 먼저 반환함.
        original?;
        if self.fault.is_some() {
            Err(stop())
        } else {
            Ok(())
        }
    }
}
