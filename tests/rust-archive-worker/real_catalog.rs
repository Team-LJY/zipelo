use super::*;

pub(super) fn worker_catalog(
    bytes: &[u8],
    limits: Limits,
    decode: bool,
    password: Option<&[u8]>,
) -> Result<archive::Catalog, ErrorCode> {
    let scratch = Scratch::new();
    fs::write(scratch.0.join("input"), bytes).unwrap();
    let events = execute(Start {
        action: Action::List {
            archive: scratch.path("input"),
        },
        limits,
    });
    if terminal(&events).outcome != Outcome::Completed {
        return Err(terminal(&events).error);
    }
    let list_id = events
        .iter()
        .find_map(|event| match event {
            Message::ListEnd { list_id, .. } => Some(list_id.clone()),
            _ => None,
        })
        .expect("작업자의 목록 식별자가 필요함");
    let entries: Vec<_> = events
        .into_iter()
        .filter_map(|event| match event {
            Message::Entry(entry) => Some(entry),
            _ => None,
        })
        .collect();
    if decode {
        let builder = fs::DirBuilder::new();
        #[cfg(unix)]
        let mut builder = builder;
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder.create(scratch.0.join("output")).unwrap();
        let action = Action::Extract {
            archive: scratch.path("input"),
            destination: scratch.path("output"),
            selection: Selection::All,
            gzip_output_name: (list_id.format == Format::Gzip).then(|| entries[0].name.clone()),
            list_id: Some(list_id.clone()),
        };
        let start = Start { action, limits };
        let extracted = if let Some(password) = password {
            interactive(start, |id, _, _| password_reply(id, password))
        } else {
            execute(start)
        };
        if terminal(&extracted).outcome != Outcome::Completed {
            return Err(terminal(&extracted).error);
        }
    }
    let members = entries
        .into_iter()
        .map(|entry| {
            let data = if decode && entry.kind == EntryKind::File {
                fs::read(scratch.0.join("output").join(&entry.name)).unwrap()
            } else {
                Vec::new()
            };
            archive::Member { entry, data }
        })
        .collect();
    Ok(archive::Catalog {
        format: list_id.format,
        members,
    })
}
