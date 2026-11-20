#[cfg(test)]
use super::{Check, Parsed};
use big_code_analysis_ast::documentation::CollectionError;
#[cfg(test)]
use big_code_analysis_ast::{
    cancel::ParseCancel,
    documentation::{DocumentationContext, DocumentationOptions, collect},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Event {
    Parse,
    #[cfg(test)]
    IncludedParse(usize),
    Collection(Result<(), CollectionError>),
    Repeat,
    CloneRejected,
    ShiftRejected,
    EditRejected,
    #[cfg(test)]
    ActiveWitness,
    OwnerQuery,
    NodeProjection,
    NodeOwners,
    NodeAssociations,
}

#[cfg(test)]
thread_local! {
    static RECORDING: std::cell::RefCell<Option<Recording>> = const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
struct Recording {
    events: [Option<Event>; 64],
    length: usize,
}

pub(super) fn record(event: Event) {
    #[cfg(test)]
    RECORDING.with_borrow_mut(|recording| {
        if let Some(recording) = recording {
            assert!(
                recording.length < recording.events.len(),
                "bounded operation recorder overflow"
            );
            recording.events[recording.length] = Some(event);
            recording.length += 1;
        }
    });
    #[cfg(not(test))]
    let _ = event;
}

pub(super) fn observed<T>(event: Event, result: T) -> T {
    record(event);
    result
}

#[cfg(test)]
pub(super) fn active_witness(
    flag: &ParseCancel,
    parsed: &Parsed<'_>,
    options: DocumentationOptions,
) -> Check {
    if RECORDING.with_borrow(Option::is_some) {
        flag.cancel();
        let result = collect(
            parsed.language,
            parsed.tree,
            parsed.source,
            &DocumentationContext::default(),
            &options,
        );
        super::require(
            result.err() == Some(CollectionError::Cancelled),
            "active scope installation witness",
        )?;
        record(Event::ActiveWitness);
    }
    Ok(())
}

#[cfg(test)]
pub(super) fn capture<T>(action: impl FnOnce() -> T) -> (T, Vec<Event>) {
    struct Restore(Option<Recording>);
    impl Drop for Restore {
        fn drop(&mut self) {
            RECORDING.with_borrow_mut(|current| *current = self.0.take());
        }
    }
    let previous = RECORDING.with_borrow_mut(|current| {
        current.replace(Recording {
            events: [None; 64],
            length: 0,
        })
    });
    let _restore = Restore(previous);
    let result = action();
    let events = RECORDING.with_borrow(|current| {
        let current = current.as_ref().unwrap();
        current.events[..current.length]
            .iter()
            .map(|event| event.unwrap())
            .collect()
    });
    (result, events)
}
