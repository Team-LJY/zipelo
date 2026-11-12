use std::{cell::RefCell, collections::VecDeque, path::PathBuf, rc::Rc};
use zipelo_ui_memory_app::archive_dialog::{DialogRequest, Dialogs, Purpose};

#[derive(Default)]
pub struct DialogState {
    pub requests: Vec<DialogRequest>,
    pub answers: VecDeque<Option<Vec<PathBuf>>>,
    pub save_name: Option<String>,
    pub cancel_save: bool,
}

#[derive(Clone, Default)]
pub struct TestDialogs(pub Rc<RefCell<DialogState>>);

impl Dialogs for TestDialogs {
    fn choose(&mut self, request: DialogRequest) -> Result<Option<Vec<PathBuf>>, String> {
        let mut state = self.0.borrow_mut();
        state.requests.push(request.clone());
        if let Some(answer) = state.answers.pop_front() {
            return Ok(answer);
        }
        if request.purpose == Purpose::SaveZip && !state.cancel_save {
            let name = state.save_name.take().unwrap_or(request.file_name);
            return Ok(Some(vec![request.directory.join(name)]));
        }
        Ok(None)
    }
}
