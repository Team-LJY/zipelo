pub use core_memory as core;

mod transport {
    include!("../../experiments/rust-ui-memory-app/src/transport.rs");
}

mod runtime {
    include!("../../experiments/rust-ui-memory-app/src/runtime.rs");

    // 원본 메서드는 바꾸지 않고 시험 채널과 실제 핸들만 조립함.
    pub fn assemble(
        work: impl FnOnce(
            core::Operation,
            SyncSender<ProgressEvent>,
            SyncSender<Completion>,
        ) -> Option<Completion>
        + Send
        + 'static,
    ) -> Task {
        let (op, cancellation) = core::Operation::start().unwrap();
        let id = op.id();
        let (sender, events) = mpsc::sync_channel(DEFAULT_CAPACITY);
        let (done, completion) = mpsc::sync_channel(1);
        let handle = thread::spawn(move || work(op, sender, done));
        Task {
            id,
            cancellation,
            events: Some(events),
            completion: Some(completion),
            handle: Some(handle),
        }
    }
}

mod controller {
    include!("../../experiments/rust-ui-memory-app/src/controller.rs");

    // 비공개 Receiver를 같은 실제 작업 ID로 초기화하는 시험 경계임.
    pub fn attach(task: Task) -> Controller {
        Controller {
            receiver: Some(core::Receiver::new(task.id)),
            task: Some(task),
            generation: 1,
            status: Status::Running,
            ..Controller::default()
        }
    }
}
