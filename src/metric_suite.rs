//! The metric half of the per-language parser contract.
//!
//! [`ParserTrait`] answers "what is this node?" through its `Checker` /
//! `Getter` classifiers and knows nothing about metrics. [`MetricSuite`]
//! layers the thirteen per-metric `compute` implementations on top, keyed
//! on the same `*Code` tag, so the metric walks can bound on one trait
//! while the parse layer stays free of them (#1376). The blanket impl
//! below is the only implementor: every `Parser<T>` whose tag implements
//! every metric trait is a `MetricSuite`, which is true of all of them.

use crate::abc::Abc;
use crate::alterator::Alterator;
use crate::checker::Checker;
use crate::cognitive::Cognitive;
use crate::cyclomatic::Cyclomatic;
use crate::getter::Getter;
use crate::halstead::Halstead;
use crate::loc::Loc;
use crate::mi::Mi;
use crate::nargs::NArgs;
use crate::nexits::Exit;
use crate::nom::Nom;
use crate::npa::Npa;
use crate::npm::Npm;
use crate::parser::Parser;
use crate::tokens::Tokens;
use crate::traits::{LanguageInfo, ParserTrait};
use crate::wmc::Wmc;

/// Per-language metric implementations reachable from a parser.
///
/// Each associated type is the `*Code` tag itself; the walk calls
/// `T::Cognitive::compute(...)` and so on. Bound a walk on this trait
/// when it computes a metric, and on [`ParserTrait`] alone when it only
/// classifies nodes.
pub(crate) trait MetricSuite: ParserTrait {
    type Cognitive: Cognitive;
    type Cyclomatic: Cyclomatic;
    type Halstead: Halstead;
    type Loc: Loc;
    type Nom: Nom;
    type Mi: Mi;
    type NArgs: NArgs;
    type Exit: Exit;
    type Wmc: Wmc;
    type Abc: Abc;
    type Npm: Npm;
    type Npa: Npa;
    type Tokens: Tokens;
}

impl<T> MetricSuite for Parser<T>
where
    T: 'static
        + LanguageInfo
        + Alterator
        + Checker
        + Getter
        + Abc
        + Cognitive
        + Cyclomatic
        + Exit
        + Halstead
        + Loc
        + Mi
        + NArgs
        + Nom
        + Npa
        + Npm
        + Tokens
        + Wmc,
{
    type Cognitive = T;
    type Cyclomatic = T;
    type Halstead = T;
    type Loc = T;
    type Nom = T;
    type Mi = T;
    type NArgs = T;
    type Exit = T;
    type Wmc = T;
    type Abc = T;
    type Npm = T;
    type Npa = T;
    type Tokens = T;
}
