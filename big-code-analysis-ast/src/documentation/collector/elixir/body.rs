//! One forward pass over the statements of a `defmodule … do … end` body.
//! It reads the tree and the source and writes nothing to the index.

use std::collections::HashMap;
use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};
use std::ops::Range;

use super::super::*;
use super::syntax::{
    Attribute, Gap, Kind, Word, at_operator, attribute, children, covers, definition, kind, node,
    plain, same, step, word,
};

pub(super) struct Carrier {
    pub(super) node: NodeId,
    // The bytes between the quotes of a plain string value.
    pub(super) text: Option<Range<usize>>,
}

// The clauses of one kind, name and source-written arity, in source order.
pub(super) struct Group {
    pub(super) kind: Kind,
    name: Range<usize>,
    pub(super) clauses: Vec<NodeId>,
    pub(super) carriers: Vec<Carrier>,
    // The clause that took the first attribute.
    pub(super) binding: Option<NodeId>,
    // Excluded bytes lie between an attribute and the clause that took it.
    pub(super) hidden: bool,
}

#[derive(Default)]
pub(super) struct Reading {
    pub(super) groups: Vec<Group>,
    pub(super) moduledocs: Vec<Carrier>,
    // Documentation attributes that no owner of this unit takes.
    pub(super) unowned: Vec<Carrier>,
    // Documentation attributes that hold a syntax error or are joined to the
    // statement before them.
    pub(super) recovered: Vec<NodeId>,
    // Definitions that are not read.
    pub(super) pending: Vec<NodeId>,
}

enum Reads {
    Sets(Attribute),
    Defines(Kind),
    Unread,
    Other,
}

struct Reader<'i, 's, B> {
    index: &'i DocumentationIndex<'s>,
    budget: &'i B,
    by_key: HashMap<(Kind, usize, u64), Vec<usize>>,
    hasher: RandomState,
    // The `@doc` attributes that wait for a definition, and whether
    // excluded bytes lie behind the first of them.
    run: Vec<Carrier>,
    barrier: bool,
    found: Reading,
}

// `gaps` says how each child of the body stands to the statement before it.
pub(super) fn read<B: WorkBudget>(
    index: &DocumentationIndex<'_>,
    body: NodeId,
    gaps: &[Option<Gap>],
    budget: &B,
) -> Result<Reading, SourceError> {
    let mut reader = Reader {
        index,
        budget,
        by_key: HashMap::new(),
        hasher: RandomState::new(),
        run: Vec::new(),
        barrier: false,
        found: Reading::default(),
    };
    for (at, &child) in children(&index.canonical, body).iter().enumerate() {
        step(budget)?;
        reader.child(child, gaps.get(at).copied().flatten())?;
    }
    reader.release();
    Ok(reader.found)
}

impl<B: WorkBudget> Reader<'_, '_, B> {
    fn child(&mut self, child: NodeId, gap: Option<Gap>) -> Result<(), SourceError> {
        let statement = node(&self.index.canonical, child);
        let recovered = statement.is_error() || statement.is_missing();
        if (statement.kind() == "comment" || !statement.is_named()) && !recovered {
            // A comment, `do`, `end` or `;`.
            return Ok(());
        }
        // Excluded bytes before the statement bar an attribute that waits.
        self.barrier |= gap == Some(Gap::Hidden) && !self.run.is_empty();
        let included = !recovered && covers(self.index, statement.byte_range(), self.budget)?;
        let reads = if included {
            self.reads(child)?
        } else {
            Reads::Other
        };
        self.apply(
            child,
            reads,
            included && gap != Some(Gap::Joined) && !statement.has_error(),
        )
    }

    fn reads(&self, child: NodeId) -> Result<Reads, SourceError> {
        let canonical = &self.index.canonical;
        if at_operator(canonical, child, self.budget).map_err(Into::into)? {
            let sets = attribute(self.index, child, self.budget).map_err(Into::into)?;
            return Ok(Reads::Sets(sets));
        }
        let word = if kind(canonical, child) == "call" {
            word(self.index, child, self.budget)?
        } else {
            Word::Other
        };
        Ok(match word {
            Word::Definition(kind) => Reads::Defines(kind),
            Word::Unread => Reads::Unread,
            _ => Reads::Other,
        })
    }

    fn apply(&mut self, child: NodeId, reads: Reads, sound: bool) -> Result<(), SourceError> {
        let carrier = |text| Carrier { node: child, text };
        match (reads, sound) {
            (Reads::Sets(Attribute::Doc), true) => {
                let text = self.plain(child)?;
                self.run.push(carrier(text));
            }
            (Reads::Sets(Attribute::Moduledoc), true) => {
                let text = self.plain(child)?;
                self.found.moduledocs.push(carrier(text));
            }
            (Reads::Sets(Attribute::Typedoc), true) => self.found.unowned.push(carrier(None)),
            // Any other attribute is read past.
            (Reads::Sets(Attribute::Other), true) => {}
            (Reads::Defines(kind), true) => return self.define(child, kind),
            (Reads::Sets(Attribute::Doc | Attribute::Moduledoc | Attribute::Typedoc), false) => {
                self.found.recovered.push(child);
                self.release();
            }
            (Reads::Defines(_), false) | (Reads::Unread, _) => {
                self.found.pending.push(child);
                self.release();
            }
            // Any other statement ends the run without an owner.
            _ => self.release(),
        }
        Ok(())
    }

    fn plain(&self, statement: NodeId) -> Result<Option<Range<usize>>, SourceError> {
        plain(&self.index.canonical, statement, self.budget).map_err(Into::into)
    }

    fn release(&mut self) {
        self.found.unowned.append(&mut self.run);
        self.barrier = false;
    }

    fn define(&mut self, child: NodeId, kind: Kind) -> Result<(), SourceError> {
        let key = definition(self.index, child, self.budget).map_err(Into::into)?;
        let Some((name, arity)) = key else {
            self.found.pending.push(child);
            self.release();
            return Ok(());
        };
        let group = self.group(kind, name, arity)?;
        let group = &mut self.found.groups[group];
        group.clauses.push(child);
        if !self.run.is_empty() {
            group.binding.get_or_insert(child);
            group.hidden |= self.barrier;
            group.carriers.append(&mut self.run);
            self.barrier = false;
        }
        Ok(())
    }

    // The group of the same kind, name and arity, or a new one.
    fn group(
        &mut self,
        kind: Kind,
        name: Range<usize>,
        arity: usize,
    ) -> Result<usize, SourceError> {
        let source = self.index.source.bytes();
        let mut hash = self.hasher.build_hasher();
        for &byte in &source[name.clone()] {
            self.budget.bytes(1).map_err(Into::into)?;
            hash.write_u8(byte);
        }
        let bucket = self.by_key.entry((kind, arity, hash.finish())).or_default();
        for &group in bucket.iter() {
            step(self.budget)?;
            let known = self.found.groups[group].name.clone();
            if same(source, known, name.clone(), self.budget).map_err(Into::into)? {
                return Ok(group);
            }
        }
        bucket.push(self.found.groups.len());
        self.found.groups.push(Group {
            kind,
            name,
            clauses: Vec::new(),
            carriers: Vec::new(),
            binding: None,
            hidden: false,
        });
        Ok(self.found.groups.len() - 1)
    }
}
