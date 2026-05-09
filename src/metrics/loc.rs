use std::collections::HashSet;

use crate::checker::Checker;
use serde::Serialize;
use serde::ser::{SerializeStruct, Serializer};
use std::fmt;

use crate::macros::implement_metric_trait;
use crate::*;

/// The `SLoc` metric suite.
#[derive(Debug, Clone)]
pub struct Sloc {
    start: usize,
    end: usize,
    unit: bool,
    sloc_min: usize,
    sloc_max: usize,
}

impl Default for Sloc {
    fn default() -> Self {
        Self {
            start: 0,
            end: 0,
            unit: false,
            sloc_min: usize::MAX,
            sloc_max: 0,
        }
    }
}

impl Sloc {
    #[inline(always)]
    pub fn sloc(&self) -> f64 {
        // This metric counts the number of lines in a file
        // The if construct is needed to count the line of code that represents
        // the function signature in a function space
        let sloc = if self.unit {
            self.end - self.start
        } else {
            (self.end - self.start) + 1
        };
        sloc as f64
    }

    /// The `Sloc` metric minimum value.
    #[inline(always)]
    pub fn sloc_min(&self) -> f64 {
        self.sloc_min as f64
    }

    /// The `Sloc` metric maximum value.
    #[inline(always)]
    pub fn sloc_max(&self) -> f64 {
        self.sloc_max as f64
    }

    #[inline(always)]
    pub fn merge(&mut self, other: &Sloc) {
        self.sloc_min = self.sloc_min.min(other.sloc() as usize);
        self.sloc_max = self.sloc_max.max(other.sloc() as usize);
    }

    #[inline(always)]
    pub(crate) fn compute_minmax(&mut self) {
        if self.sloc_min == usize::MAX {
            self.sloc_min = self.sloc_min.min(self.sloc() as usize);
            self.sloc_max = self.sloc_max.max(self.sloc() as usize);
        }
    }
}

/// The `PLoc` metric suite.
#[derive(Debug, Clone)]
pub struct Ploc {
    lines: HashSet<usize>,
    ploc_min: usize,
    ploc_max: usize,
}

impl Default for Ploc {
    fn default() -> Self {
        Self {
            lines: HashSet::default(),
            ploc_min: usize::MAX,
            ploc_max: 0,
        }
    }
}

impl Ploc {
    #[inline(always)]
    pub fn ploc(&self) -> f64 {
        // This metric counts the number of instruction lines in a code
        // https://en.wikipedia.org/wiki/Source_lines_of_code
        self.lines.len() as f64
    }

    /// The `Ploc` metric minimum value.
    #[inline(always)]
    pub fn ploc_min(&self) -> f64 {
        self.ploc_min as f64
    }

    /// The `Ploc` metric maximum value.
    #[inline(always)]
    pub fn ploc_max(&self) -> f64 {
        self.ploc_max as f64
    }

    #[inline(always)]
    pub fn merge(&mut self, other: &Ploc) {
        // Merge ploc lines
        for l in other.lines.iter() {
            self.lines.insert(*l);
        }

        self.ploc_min = self.ploc_min.min(other.ploc() as usize);
        self.ploc_max = self.ploc_max.max(other.ploc() as usize);
    }

    #[inline(always)]
    pub(crate) fn compute_minmax(&mut self) {
        if self.ploc_min == usize::MAX {
            self.ploc_min = self.ploc_min.min(self.ploc() as usize);
            self.ploc_max = self.ploc_max.max(self.ploc() as usize);
        }
    }
}

/// The `CLoc` metric suite.
#[derive(Debug, Clone)]
pub struct Cloc {
    only_comment_lines: usize,
    code_comment_lines: usize,
    comment_line_end: Option<usize>,
    cloc_min: usize,
    cloc_max: usize,
}

impl Default for Cloc {
    fn default() -> Self {
        Self {
            only_comment_lines: 0,
            code_comment_lines: 0,
            comment_line_end: Option::default(),
            cloc_min: usize::MAX,
            cloc_max: 0,
        }
    }
}

impl Cloc {
    #[inline(always)]
    pub fn cloc(&self) -> f64 {
        // Comments are counted regardless of their placement
        // https://en.wikipedia.org/wiki/Source_lines_of_code
        (self.only_comment_lines + self.code_comment_lines) as f64
    }

    /// The `Cloc` metric minimum value.
    #[inline(always)]
    pub fn cloc_min(&self) -> f64 {
        self.cloc_min as f64
    }

    /// The `Cloc` metric maximum value.
    #[inline(always)]
    pub fn cloc_max(&self) -> f64 {
        self.cloc_max as f64
    }

    #[inline(always)]
    pub fn merge(&mut self, other: &Cloc) {
        // Merge cloc lines
        self.only_comment_lines += other.only_comment_lines;
        self.code_comment_lines += other.code_comment_lines;

        self.cloc_min = self.cloc_min.min(other.cloc() as usize);
        self.cloc_max = self.cloc_max.max(other.cloc() as usize);
    }

    #[inline(always)]
    pub(crate) fn compute_minmax(&mut self) {
        if self.cloc_min == usize::MAX {
            self.cloc_min = self.cloc_min.min(self.cloc() as usize);
            self.cloc_max = self.cloc_max.max(self.cloc() as usize);
        }
    }
}

/// The `LLoc` metric suite.
#[derive(Debug, Clone)]
pub struct Lloc {
    logical_lines: usize,
    lloc_min: usize,
    lloc_max: usize,
}

impl Default for Lloc {
    fn default() -> Self {
        Self {
            logical_lines: 0,
            lloc_min: usize::MAX,
            lloc_max: 0,
        }
    }
}

impl Lloc {
    #[inline(always)]
    pub fn lloc(&self) -> f64 {
        // This metric counts the number of statements in a code
        // https://en.wikipedia.org/wiki/Source_lines_of_code
        self.logical_lines as f64
    }

    /// The `Lloc` metric minimum value.
    #[inline(always)]
    pub fn lloc_min(&self) -> f64 {
        self.lloc_min as f64
    }

    /// The `Lloc` metric maximum value.
    #[inline(always)]
    pub fn lloc_max(&self) -> f64 {
        self.lloc_max as f64
    }

    #[inline(always)]
    pub fn merge(&mut self, other: &Lloc) {
        // Merge lloc lines
        self.logical_lines += other.logical_lines;
        self.lloc_min = self.lloc_min.min(other.lloc() as usize);
        self.lloc_max = self.lloc_max.max(other.lloc() as usize);
    }

    #[inline(always)]
    pub(crate) fn compute_minmax(&mut self) {
        if self.lloc_min == usize::MAX {
            self.lloc_min = self.lloc_min.min(self.lloc() as usize);
            self.lloc_max = self.lloc_max.max(self.lloc() as usize);
        }
    }
}

/// The `Loc` metric suite.
#[derive(Debug, Clone)]
pub struct Stats {
    sloc: Sloc,
    ploc: Ploc,
    cloc: Cloc,
    lloc: Lloc,
    space_count: usize,
    blank_min: usize,
    blank_max: usize,
}

impl Default for Stats {
    fn default() -> Self {
        Self {
            sloc: Sloc::default(),
            ploc: Ploc::default(),
            cloc: Cloc::default(),
            lloc: Lloc::default(),
            space_count: 1,
            blank_min: usize::MAX,
            blank_max: 0,
        }
    }
}

impl Serialize for Stats {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut st = serializer.serialize_struct("loc", 20)?;
        st.serialize_field("sloc", &self.sloc())?;
        st.serialize_field("ploc", &self.ploc())?;
        st.serialize_field("lloc", &self.lloc())?;
        st.serialize_field("cloc", &self.cloc())?;
        st.serialize_field("blank", &self.blank())?;
        st.serialize_field("sloc_average", &self.sloc_average())?;
        st.serialize_field("ploc_average", &self.ploc_average())?;
        st.serialize_field("lloc_average", &self.lloc_average())?;
        st.serialize_field("cloc_average", &self.cloc_average())?;
        st.serialize_field("blank_average", &self.blank_average())?;
        st.serialize_field("sloc_min", &self.sloc_min())?;
        st.serialize_field("sloc_max", &self.sloc_max())?;
        st.serialize_field("cloc_min", &self.cloc_min())?;
        st.serialize_field("cloc_max", &self.cloc_max())?;
        st.serialize_field("ploc_min", &self.ploc_min())?;
        st.serialize_field("ploc_max", &self.ploc_max())?;
        st.serialize_field("lloc_min", &self.lloc_min())?;
        st.serialize_field("lloc_max", &self.lloc_max())?;
        st.serialize_field("blank_min", &self.blank_min())?;
        st.serialize_field("blank_max", &self.blank_max())?;
        st.end()
    }
}

impl fmt::Display for Stats {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "sloc: {}, ploc: {}, lloc: {}, cloc: {}, blank: {}, sloc_average: {}, ploc_average: {}, lloc_average: {}, cloc_average: {}, blank_average: {}, sloc_min: {}, sloc_max: {}, cloc_min: {}, cloc_max: {}, ploc_min: {}, ploc_max: {}, lloc_min: {}, lloc_max: {}, blank_min: {}, blank_max: {}",
            self.sloc(),
            self.ploc(),
            self.lloc(),
            self.cloc(),
            self.blank(),
            self.sloc_average(),
            self.ploc_average(),
            self.lloc_average(),
            self.cloc_average(),
            self.blank_average(),
            self.sloc_min(),
            self.sloc_max(),
            self.cloc_min(),
            self.cloc_max(),
            self.ploc_min(),
            self.ploc_max(),
            self.lloc_min(),
            self.lloc_max(),
            self.blank_min(),
            self.blank_max(),
        )
    }
}

impl Stats {
    /// Merges a second `Loc` metric suite into the first one
    pub fn merge(&mut self, other: &Stats) {
        self.sloc.merge(&other.sloc);
        self.ploc.merge(&other.ploc);
        self.cloc.merge(&other.cloc);
        self.lloc.merge(&other.lloc);

        // Count spaces
        self.space_count += other.space_count;

        // min and max

        self.blank_min = self.blank_min.min(other.blank() as usize);
        self.blank_max = self.blank_max.max(other.blank() as usize);
    }

    /// The `Sloc` metric.
    ///
    /// Counts the number of lines in a scope
    #[inline(always)]
    pub fn sloc(&self) -> f64 {
        self.sloc.sloc()
    }

    /// The `Ploc` metric.
    ///
    /// Counts the number of instruction lines in a scope
    #[inline(always)]
    pub fn ploc(&self) -> f64 {
        self.ploc.ploc()
    }

    /// The `Lloc` metric.
    ///
    /// Counts the number of statements in a scope
    #[inline(always)]
    pub fn lloc(&self) -> f64 {
        self.lloc.lloc()
    }

    /// The `Cloc` metric.
    ///
    /// Counts the number of comments in a scope
    #[inline(always)]
    pub fn cloc(&self) -> f64 {
        self.cloc.cloc()
    }

    /// The `Blank` metric.
    ///
    /// Counts the number of blank lines in a scope
    #[inline(always)]
    pub fn blank(&self) -> f64 {
        self.sloc() - self.ploc() - self.cloc.only_comment_lines as f64
    }

    /// The `Sloc` metric average value.
    ///
    /// This value is computed dividing the `Sloc` value for the number of spaces
    #[inline(always)]
    pub fn sloc_average(&self) -> f64 {
        self.sloc() / self.space_count as f64
    }

    /// The `Ploc` metric average value.
    ///
    /// This value is computed dividing the `Ploc` value for the number of spaces
    #[inline(always)]
    pub fn ploc_average(&self) -> f64 {
        self.ploc() / self.space_count as f64
    }

    /// The `Lloc` metric average value.
    ///
    /// This value is computed dividing the `Lloc` value for the number of spaces
    #[inline(always)]
    pub fn lloc_average(&self) -> f64 {
        self.lloc() / self.space_count as f64
    }

    /// The `Cloc` metric average value.
    ///
    /// This value is computed dividing the `Cloc` value for the number of spaces
    #[inline(always)]
    pub fn cloc_average(&self) -> f64 {
        self.cloc() / self.space_count as f64
    }

    /// The `Blank` metric average value.
    ///
    /// This value is computed dividing the `Blank` value for the number of spaces
    #[inline(always)]
    pub fn blank_average(&self) -> f64 {
        self.blank() / self.space_count as f64
    }

    /// The `Sloc` metric minimum value.
    #[inline(always)]
    pub fn sloc_min(&self) -> f64 {
        self.sloc.sloc_min()
    }

    /// The `Sloc` metric maximum value.
    #[inline(always)]
    pub fn sloc_max(&self) -> f64 {
        self.sloc.sloc_max()
    }

    /// The `Cloc` metric minimum value.
    #[inline(always)]
    pub fn cloc_min(&self) -> f64 {
        self.cloc.cloc_min()
    }

    /// The `Cloc` metric maximum value.
    #[inline(always)]
    pub fn cloc_max(&self) -> f64 {
        self.cloc.cloc_max()
    }

    /// The `Ploc` metric minimum value.
    #[inline(always)]
    pub fn ploc_min(&self) -> f64 {
        self.ploc.ploc_min()
    }

    /// The `Ploc` metric maximum value.
    #[inline(always)]
    pub fn ploc_max(&self) -> f64 {
        self.ploc.ploc_max()
    }

    /// The `Lloc` metric minimum value.
    #[inline(always)]
    pub fn lloc_min(&self) -> f64 {
        self.lloc.lloc_min()
    }

    /// The `Lloc` metric maximum value.
    #[inline(always)]
    pub fn lloc_max(&self) -> f64 {
        self.lloc.lloc_max()
    }

    /// The `Blank` metric minimum value.
    #[inline(always)]
    pub fn blank_min(&self) -> f64 {
        self.blank_min as f64
    }

    /// The `Blank` metric maximum value.
    #[inline(always)]
    pub fn blank_max(&self) -> f64 {
        self.blank_max as f64
    }

    #[inline(always)]
    pub(crate) fn compute_minmax(&mut self) {
        self.sloc.compute_minmax();
        self.ploc.compute_minmax();
        self.cloc.compute_minmax();
        self.lloc.compute_minmax();

        if self.blank_min == usize::MAX {
            self.blank_min = self.blank_min.min(self.blank() as usize);
            self.blank_max = self.blank_max.max(self.blank() as usize);
        }
    }

    pub(crate) fn init_unit_span(&mut self, start: usize, end: usize) {
        self.sloc.start = start;
        self.sloc.end = end;
        self.sloc.unit = true;
    }
}

pub trait Loc
where
    Self: Checker,
{
    fn compute(node: &Node, stats: &mut Stats, is_func_space: bool, is_unit: bool);
}

#[inline(always)]
fn init(node: &Node, stats: &mut Stats, is_func_space: bool, is_unit: bool) -> (usize, usize) {
    let start = node.start_row();
    let end = node.end_row();

    if is_func_space {
        stats.sloc.start = start;
        stats.sloc.end = end;
        stats.sloc.unit = is_unit;
    }
    (start, end)
}

#[inline(always)]
// Discriminates among the comments that are *after* a code line and
// the ones that are on an independent line.
// This difference is necessary in order to avoid having
// a wrong count for the blank metric.
fn add_cloc_lines(stats: &mut Stats, start: usize, end: usize) {
    let comment_diff = end - start;
    let is_comment_after_code_line = stats.ploc.lines.contains(&start);
    if is_comment_after_code_line && comment_diff == 0 {
        // A comment is *entirely* next to a code line
        stats.cloc.code_comment_lines += 1;
    } else if is_comment_after_code_line && comment_diff > 0 {
        // A block comment that starts next to a code line and ends on
        // independent lines.
        stats.cloc.code_comment_lines += 1;
        stats.cloc.only_comment_lines += comment_diff;
    } else {
        // A comment on an independent line AND
        // a block comment on independent lines OR
        // a comment *before* a code line
        stats.cloc.only_comment_lines += (end - start) + 1;
        // Save line end of a comment to check whether
        // a comment *before* a code line is considered
        stats.cloc.comment_line_end = Some(end);
    }
}

#[inline(always)]
// Detects the comments that are on a code line but *before* the code part.
// This difference is necessary in order to avoid having
// a wrong count for the blank metric.
fn check_comment_ends_on_code_line(stats: &mut Stats, start_code_line: usize) {
    if let Some(end) = stats.cloc.comment_line_end
        && end == start_code_line
        && !stats.ploc.lines.contains(&start_code_line)
    {
        // Comment entirely *before* a code line
        stats.cloc.only_comment_lines -= 1;
        stats.cloc.code_comment_lines += 1;
    }
}

impl Loc for PythonCode {
    fn compute(node: &Node, stats: &mut Stats, is_func_space: bool, is_unit: bool) {
        use Python::*;

        let (start, end) = init(node, stats, is_func_space, is_unit);

        match node.kind_id().into() {
            StringStart | StringEnd | StringContent | Block | Module => {}
            Comment => {
                add_cloc_lines(stats, start, end);
            }
            String => {
                let Some(parent) = node.parent() else { return };
                if let ExpressionStatement = parent.kind_id().into() {
                    add_cloc_lines(stats, start, end);
                } else if parent.start_row() != start {
                    check_comment_ends_on_code_line(stats, start);
                    stats.ploc.lines.insert(start);
                }
            }
            Statement
            | SimpleStatements
            | ImportStatement
            | FutureImportStatement
            | ImportFromStatement
            | PrintStatement
            | AssertStatement
            | ReturnStatement
            | DeleteStatement
            | RaiseStatement
            | PassStatement
            | BreakStatement
            | ContinueStatement
            | IfStatement
            | ForStatement
            | WhileStatement
            | TryStatement
            | WithStatement
            | GlobalStatement
            | NonlocalStatement
            | ExecStatement
            | ExpressionStatement => {
                stats.lloc.logical_lines += 1;
            }
            _ => {
                check_comment_ends_on_code_line(stats, start);
                stats.ploc.lines.insert(start);
            }
        }
    }
}

impl Loc for MozjsCode {
    fn compute(node: &Node, stats: &mut Stats, is_func_space: bool, is_unit: bool) {
        use Mozjs::*;

        let (start, end) = init(node, stats, is_func_space, is_unit);

        match node.kind_id().into() {
            String | DQUOTE | Program => {}
            Comment => {
                add_cloc_lines(stats, start, end);
            }
            ExpressionStatement | ExportStatement | ImportStatement | StatementBlock
            | IfStatement | SwitchStatement | ForStatement | ForInStatement | WhileStatement
            | DoStatement | TryStatement | WithStatement | BreakStatement | ContinueStatement
            | DebuggerStatement | ReturnStatement | ThrowStatement | EmptyStatement
            | StatementIdentifier => {
                stats.lloc.logical_lines += 1;
            }
            _ => {
                check_comment_ends_on_code_line(stats, start);
                stats.ploc.lines.insert(start);
            }
        }
    }
}

impl Loc for JavascriptCode {
    fn compute(node: &Node, stats: &mut Stats, is_func_space: bool, is_unit: bool) {
        use Javascript::*;

        let (start, end) = init(node, stats, is_func_space, is_unit);

        match node.kind_id().into() {
            String | DQUOTE | Program => {}
            Comment => {
                add_cloc_lines(stats, start, end);
            }
            ExpressionStatement | ExportStatement | ImportStatement | StatementBlock
            | IfStatement | SwitchStatement | ForStatement | ForInStatement | WhileStatement
            | DoStatement | TryStatement | WithStatement | BreakStatement | ContinueStatement
            | DebuggerStatement | ReturnStatement | ThrowStatement | EmptyStatement
            | StatementIdentifier => {
                stats.lloc.logical_lines += 1;
            }
            _ => {
                check_comment_ends_on_code_line(stats, start);
                stats.ploc.lines.insert(start);
            }
        }
    }
}

impl Loc for TypescriptCode {
    fn compute(node: &Node, stats: &mut Stats, is_func_space: bool, is_unit: bool) {
        use Typescript::*;

        let (start, end) = init(node, stats, is_func_space, is_unit);

        match node.kind_id().into() {
            String | DQUOTE | Program => {}
            Comment => {
                add_cloc_lines(stats, start, end);
            }
            ExpressionStatement | ExportStatement | ImportStatement | StatementBlock
            | IfStatement | SwitchStatement | ForStatement | ForInStatement | WhileStatement
            | DoStatement | TryStatement | WithStatement | BreakStatement | ContinueStatement
            | DebuggerStatement | ReturnStatement | ThrowStatement | EmptyStatement
            | StatementIdentifier => {
                stats.lloc.logical_lines += 1;
            }
            _ => {
                check_comment_ends_on_code_line(stats, start);
                stats.ploc.lines.insert(start);
            }
        }
    }
}

impl Loc for TsxCode {
    fn compute(node: &Node, stats: &mut Stats, is_func_space: bool, is_unit: bool) {
        use Tsx::*;

        let (start, end) = init(node, stats, is_func_space, is_unit);

        match node.kind_id().into() {
            String | DQUOTE | Program => {}
            Comment => {
                add_cloc_lines(stats, start, end);
            }
            ExpressionStatement | ExportStatement | ImportStatement | StatementBlock
            | IfStatement | SwitchStatement | ForStatement | ForInStatement | WhileStatement
            | DoStatement | TryStatement | WithStatement | BreakStatement | ContinueStatement
            | DebuggerStatement | ReturnStatement | ThrowStatement | EmptyStatement
            | StatementIdentifier => {
                stats.lloc.logical_lines += 1;
            }
            _ => {
                check_comment_ends_on_code_line(stats, start);
                stats.ploc.lines.insert(start);
            }
        }
    }
}

impl Loc for RustCode {
    fn compute(node: &Node, stats: &mut Stats, is_func_space: bool, is_unit: bool) {
        use Rust::*;

        let (start, end) = init(node, stats, is_func_space, is_unit);

        match node.kind_id().into() {
            StringLiteral
            | RawStringLiteral
            | Block
            | SourceFile
            | SLASH
            | SLASHSLASH
            | SLASHSTAR
            | STARSLASH
            | OuterDocCommentMarker
            | OuterDocCommentMarker2
            | DocComment
            | InnerDocCommentMarker
            | BANG => {}
            BlockComment => {
                add_cloc_lines(stats, start, end);
            }
            LineComment => {
                // Exclude the last line for `LineComment` containing a `DocComment`,
                // since the `DocComment` includes the newline,
                // as explained here: https://github.com/tree-sitter/tree-sitter-rust/blob/2eaf126458a4d6a69401089b6ba78c5e5d6c1ced/src/scanner.c#L194-L195
                let end = if node.is_child(DocComment as u16) {
                    end - 1
                } else {
                    end
                };
                add_cloc_lines(stats, start, end);
            }
            Statement
            | EmptyStatement
            | ExpressionStatement
            | LetDeclaration
            | AssignmentExpression
            | CompoundAssignmentExpr => {
                stats.lloc.logical_lines += 1;
            }
            _ => {
                check_comment_ends_on_code_line(stats, start);
                stats.ploc.lines.insert(start);
            }
        }
    }
}

impl Loc for CppCode {
    fn compute(node: &Node, stats: &mut Stats, is_func_space: bool, is_unit: bool) {
        use Cpp::*;

        let (start, end) = init(node, stats, is_func_space, is_unit);

        match node.kind_id().into() {
            RawStringLiteral | StringLiteral | DeclarationList | FieldDeclarationList
            | TranslationUnit => {}
            Comment => {
                add_cloc_lines(stats, start, end);
            }
            WhileStatement | SwitchStatement | CaseStatement | IfStatement | ForStatement
            | ReturnStatement | BreakStatement | ContinueStatement | GotoStatement
            | ThrowStatement | TryStatement | TryStatement2 | ExpressionStatement
            | ExpressionStatement2 | LabeledStatement | StatementIdentifier => {
                stats.lloc.logical_lines += 1;
            }
            Declaration => {
                if node.count_specific_ancestors::<CppParser>(
                    |node| {
                        matches!(
                            node.kind_id().into(),
                            WhileStatement | ForStatement | IfStatement
                        )
                    },
                    |node| node.kind_id() == CompoundStatement,
                ) == 0
                {
                    stats.lloc.logical_lines += 1;
                }
            }
            _ => {
                check_comment_ends_on_code_line(stats, start);
                stats.ploc.lines.insert(start);

                // As reported here: https://github.com/tree-sitter/tree-sitter-cpp/issues/276
                // `tree-sitter-cpp` doesn't expand macros, providing a single `PreprocArg` node for the entire macro argument.
                // Therefore, all lines from `start_row` to `end_row` must be added to PLOC to account for the unexpanded macro content
                if let PreprocArg = node.kind_id().into() {
                    (node.start_row() + 1..=node.end_row()).for_each(|line| {
                        stats.ploc.lines.insert(line);
                    });
                }
            }
        }
    }
}

impl Loc for JavaCode {
    fn compute(node: &Node, stats: &mut Stats, is_func_space: bool, is_unit: bool) {
        use Java::*;

        let (start, end) = init(node, stats, is_func_space, is_unit);
        let kind_id: Java = node.kind_id().into();
        // LLOC in Java is counted for statements only
        // https://docs.oracle.com/javase/tutorial/java/nutsandbolts/expressions.html
        match kind_id {
            Program => {}
            LineComment | BlockComment => {
                add_cloc_lines(stats, start, end);
            }
            AssertStatement | BreakStatement | ContinueStatement | DoStatement
            | EnhancedForStatement | ExpressionStatement | ForStatement | IfStatement
            | ReturnStatement | SwitchExpression | ThrowStatement | TryStatement
            | WhileStatement => {
                stats.lloc.logical_lines += 1;
            }
            LocalVariableDeclaration => {
                if node.count_specific_ancestors::<JavaParser>(
                    |node| node.kind_id() == ForStatement,
                    |node| node.kind_id() == Block,
                ) == 0
                {
                    // The initializer, condition, and increment in a for loop are expressions.
                    // Don't count the variable declaration if in a ForStatement.
                    // https://docs.oracle.com/javase/tutorial/java/nutsandbolts/for.html
                    stats.lloc.logical_lines += 1;
                }
            }
            _ => {
                check_comment_ends_on_code_line(stats, start);
                stats.ploc.lines.insert(start);
            }
        }
    }
}

impl Loc for CsharpCode {
    fn compute(node: &Node, stats: &mut Stats, is_func_space: bool, is_unit: bool) {
        use Csharp::*;

        let (start, end) = init(node, stats, is_func_space, is_unit);
        let kind_id: Csharp = node.kind_id().into();
        match kind_id {
            CompilationUnit => {}
            Comment => {
                add_cloc_lines(stats, start, end);
            }
            BreakStatement | CheckedStatement | ContinueStatement | DoStatement
            | ExpressionStatement | FixedStatement | ForStatement | ForeachStatement
            | GotoStatement | IfStatement | LabeledStatement | LockStatement | ReturnStatement
            | SwitchStatement | ThrowStatement | TryStatement | UnsafeStatement
            | UsingStatement | WhileStatement | YieldStatement => {
                stats.lloc.logical_lines += 1;
            }
            LocalDeclarationStatement => {
                // Variable declarations inside a `for_statement` init/condition/update
                // (e.g. `for (int i = 0; i < n; i++)`) shouldn't bump LLOC; the
                // surrounding `for_statement` already counts.
                if node.count_specific_ancestors::<CsharpParser>(
                    |n| n.kind_id() == ForStatement,
                    |n| n.kind_id() == Block,
                ) == 0
                {
                    stats.lloc.logical_lines += 1;
                }
            }
            _ => {
                check_comment_ends_on_code_line(stats, start);
                stats.ploc.lines.insert(start);
            }
        }
    }
}

impl Loc for GoCode {
    fn compute(node: &Node, stats: &mut Stats, is_func_space: bool, is_unit: bool) {
        // Aliased because `Go::Go` (the `go` keyword variant) collides with
        // the bare enum name in pattern position under `use Go::*;`.
        use Go as G;

        let (start, end) = init(node, stats, is_func_space, is_unit);

        match node.kind_id().into() {
            G::SourceFile | G::RawStringLiteral | G::InterpretedStringLiteral => {}
            G::Comment => {
                add_cloc_lines(stats, start, end);
            }
            G::FallthroughStatement
            | G::BreakStatement
            | G::ContinueStatement
            | G::GotoStatement
            | G::ReturnStatement
            | G::GoStatement
            | G::DeferStatement
            | G::IfStatement
            | G::ForStatement
            | G::ExpressionSwitchStatement
            | G::TypeSwitchStatement
            | G::SelectStatement
            | G::LabeledStatement => {
                stats.lloc.logical_lines += 1;
            }
            G::ExpressionStatement
            | G::SendStatement
            | G::IncStatement
            | G::DecStatement
            | G::AssignmentStatement
            | G::ShortVarDeclaration
            | G::VarDeclaration
            | G::ConstDeclaration => {
                // Skip simple statements / declarations that appear inside a
                // for-clause init or update slot (e.g. `for i := 0; i < n; i++`);
                // the surrounding `for_statement` already counts as one
                // logical line.
                if node.count_specific_ancestors::<GoParser>(
                    |n| n.kind_id() == G::ForClause,
                    |n| n.kind_id() == G::Block,
                ) == 0
                {
                    stats.lloc.logical_lines += 1;
                }
            }
            _ => {
                check_comment_ends_on_code_line(stats, start);
                stats.ploc.lines.insert(start);
            }
        }
    }
}

impl Loc for PerlCode {
    fn compute(node: &Node, stats: &mut Stats, is_func_space: bool, is_unit: bool) {
        use Perl as P;

        let (start, end) = init(node, stats, is_func_space, is_unit);

        match node.kind_id().into() {
            P::SourceFile
            | P::Block
            | P::StandaloneBlock
            | P::HeredocBodyStatement
            | P::HeredocContent
            | P::PodContent
            | P::StringSingleQuoted
            | P::StringDoubleQuoted
            | P::StringQQuoted
            | P::StringQqQuoted
            | P::BacktickQuoted
            | P::CommandQxQuoted
            // Internal string tokens — already accounted for by the
            // parent string node's start row.
            | P::SQUOTE
            | P::DQUOTE
            | P::StringContent
            | P::StringSingleQuotedContent
            | P::StringSingleQQuotedContent
            | P::StringQqQuotedContent
            | P::StringDoubleQuotedContent
            | P::EscapeSequence
            | P::EscapeSequenceToken1
            | P::Interpolation => {}
            P::Comments | P::PodStatement => {
                add_cloc_lines(stats, start, end);
            }
            P::SingleLineStatement
            | P::IfStatement
            | P::UnlessStatement
            | P::WhileStatement
            | P::UntilStatement
            | P::ForStatement1
            | P::ForStatement2
            | P::LoopControlStatement
            | P::PackageStatement
            | P::RequireStatement
            | P::UseNoStatement
            | P::UseNoFeatureStatement
            | P::UseNoIfStatement
            | P::UseNoSubsStatement
            | P::UseConstantStatement
            | P::UseParentStatement
            | P::UseNoVersion
            | P::EllipsisStatement => {
                stats.lloc.logical_lines += 1;
            }
            P::SEMI => {
                // A `;` at top of `source_file` / a function `block` ends a
                // statement (Perl wraps simple expressions in semicolons
                // rather than emitting a dedicated statement kind), so it
                // contributes one LLOC. Then fall through to the same PLOC
                // bookkeeping the catch-all arm does.
                if let Some(parent) = node.parent()
                    && matches!(parent.kind_id().into(), P::SourceFile | P::Block)
                {
                    stats.lloc.logical_lines += 1;
                }
                check_comment_ends_on_code_line(stats, start);
                stats.ploc.lines.insert(start);
            }
            _ => {
                check_comment_ends_on_code_line(stats, start);
                stats.ploc.lines.insert(start);
            }
        }
    }
}

impl Loc for LuaCode {
    fn compute(node: &Node, stats: &mut Stats, is_func_space: bool, is_unit: bool) {
        let (start, end) = init(node, stats, is_func_space, is_unit);

        match node.kind_id().into() {
            // Skip root and string literals.
            Lua::Chunk | Lua::String => {}

            // Skip tokens that are children of comment nodes.
            // Lua's comment nodes have children: DASHDASH / LBRACKLBRACK (openers),
            // CommentContent / CommentContent2 (body), and RBRACKRBRACK (block closer).
            // Without this guard they hit the `_` arm and add their rows to `ploc`,
            // which rows are already counted in `only_comment_lines`, producing
            // negative `blank`. LBRACKLBRACK / RBRACKRBRACK also appear as children of
            // string nodes, so we guard on the parent kind to avoid skipping them there.
            Lua::DASHDASH | Lua::CommentContent | Lua::CommentContent2 => {}
            Lua::LBRACKLBRACK | Lua::RBRACKRBRACK
                if node.parent().is_some_and(|p| p.kind_id() == Lua::Comment) => {}

            Lua::Comment => {
                add_cloc_lines(stats, start, end);
            }

            // Standalone assignment (`x = 1`). Skip when nested inside a local variable
            // declaration (`local x = 1`) — the parent VariableDeclaration already counts.
            Lua::AssignmentStatement | Lua::AssignmentStatement2
                if !node.parent().is_some_and(|p| {
                    matches!(
                        p.kind_id().into(),
                        Lua::VariableDeclaration
                            | Lua::VariableDeclaration2
                            | Lua::ImplicitVariableDeclaration
                    )
                }) =>
            {
                stats.lloc.logical_lines += 1;
            }

            Lua::IfStatement
            | Lua::ForStatement
            | Lua::WhileStatement
            | Lua::RepeatStatement
            | Lua::DoStatement
            | Lua::ReturnStatement
            | Lua::BreakStatement
            | Lua::GotoStatement
            | Lua::LabelStatement
            | Lua::VariableDeclaration
            | Lua::VariableDeclaration2
            | Lua::ImplicitVariableDeclaration
            | Lua::FunctionDeclaration
            | Lua::FunctionDeclaration2
            | Lua::FunctionDeclaration3 => {
                stats.lloc.logical_lines += 1;
            }

            _ => {
                check_comment_ends_on_code_line(stats, start);
                stats.ploc.lines.insert(start);
            }
        }
    }
}

impl Loc for KotlinCode {
    fn compute(node: &Node, stats: &mut Stats, is_func_space: bool, is_unit: bool) {
        use Kotlin::*;

        let (start, end) = init(node, stats, is_func_space, is_unit);

        match node.kind_id().into() {
            SourceFile => {}
            LineComment | BlockComment => {
                add_cloc_lines(stats, start, end);
            }
            ForStatement | WhileStatement | DoWhileStatement | IfExpression | WhenExpression
            | TryExpression | ThrowExpression | ReturnExpression | Assignment
            | PropertyDeclaration => {
                stats.lloc.logical_lines += 1;
            }
            // Bare expression statements (e.g. `println(x)`) have no
            // ExpressionStatement wrapper in tree-sitter-kotlin-ng. Count
            // them as lloc when they appear as direct children of a block;
            // otherwise fall through to ploc so nested calls still count
            // as physical lines.
            CallExpression | NavigationExpression => {
                if let Some(parent) = node.parent()
                    && matches!(
                        parent.kind_id().into(),
                        Block | FunctionBody | SourceFile | CatchBlock | FinallyBlock
                    )
                {
                    stats.lloc.logical_lines += 1;
                } else {
                    check_comment_ends_on_code_line(stats, start);
                    stats.ploc.lines.insert(start);
                }
            }
            _ => {
                check_comment_ends_on_code_line(stats, start);
                stats.ploc.lines.insert(start);
            }
        }
    }
}

impl Loc for PhpCode {
    fn compute(node: &Node, stats: &mut Stats, is_func_space: bool, is_unit: bool) {
        use Php::*;

        let (start, end) = init(node, stats, is_func_space, is_unit);

        match node.kind_id().into() {
            Program => {}
            Comment => {
                add_cloc_lines(stats, start, end);
            }
            // Statement kinds that contribute one logical line each.
            ExpressionStatement
            | EchoStatement
            | EmptyStatement
            | IfStatement
            | SwitchStatement
            | ForStatement
            | ForeachStatement
            | WhileStatement
            | DoStatement
            | TryStatement
            | ReturnStatement
            | BreakStatement
            | ContinueStatement
            | GotoStatement
            | UnsetStatement
            | DeclareStatement
            | NamespaceUseDeclaration
            | GlobalDeclaration
            | FunctionStaticDeclaration
            | ConstDeclaration
            | ConstDeclaration2
            | PropertyDeclaration
            | NamedLabelStatement => {
                stats.lloc.logical_lines += 1;
            }
            _ => {
                check_comment_ends_on_code_line(stats, start);
                stats.ploc.lines.insert(start);
            }
        }
    }
}

implement_metric_trait!(Loc, PreprocCode, CcommentCode);

impl Loc for BashCode {
    fn compute(node: &Node, stats: &mut Stats, is_func_space: bool, is_unit: bool) {
        use Bash::*;

        let (start, end) = init(node, stats, is_func_space, is_unit);

        match node.kind_id().into() {
            Program => {}
            Comment => {
                add_cloc_lines(stats, start, end);
            }
            // LLOC: leaf statement nodes. Pipeline, Subshell, and
            // RedirectedStatement are excluded because they wrap inner
            // Command nodes that are already counted here.
            Command | VariableAssignment | DeclarationCommand | UnsetCommand | IfStatement
            | ForStatement | CStyleForStatement | WhileStatement | CaseStatement
            | FunctionDefinition => {
                stats.lloc.logical_lines += 1;
            }
            _ => {
                if node.child_count() == 0 {
                    stats.ploc.lines.insert(start);
                }
            }
        }
    }
}

impl Loc for TclCode {
    fn compute(node: &Node, stats: &mut Stats, is_func_space: bool, is_unit: bool) {
        let (start, end) = init(node, stats, is_func_space, is_unit);

        match node.kind_id().into() {
            Tcl::SourceFile => {}

            Tcl::Comment => {
                add_cloc_lines(stats, start, end);
            }

            Tcl::Procedure
            | Tcl::If
            | Tcl::Elseif
            | Tcl::Foreach
            | Tcl::While
            | Tcl::Set
            | Tcl::Global
            | Tcl::Namespace
            | Tcl::Try
            | Tcl::Catch
            | Tcl::Regexp => {
                stats.lloc.logical_lines += 1;
            }

            // `expr` at statement level is a logical line; inside [...] it is a
            // sub-expression and should not be counted (same semantics as Command).
            Tcl::ExprCmd
            // Commands inside [...] are sub-expressions, not top-level statements.
            | Tcl::Command
                if node
                    .parent()
                    .is_none_or(|p| p.kind_id() != Tcl::CommandSubstitution) =>
            {
                stats.lloc.logical_lines += 1;
            }

            _ => {
                check_comment_ends_on_code_line(stats, start);
                stats.ploc.lines.insert(start);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::tools::check_metrics;

    use super::*;

    /// Parses `source` with `PerlParser` and asserts the resulting tree has
    /// no `ERROR` nodes. Use alongside metric assertions whose expected
    /// values would happen to match what an error tree produces — a parse
    /// regression in tree-sitter-perl could otherwise leave such tests
    /// silently green.
    #[cfg(test)]
    fn assert_perl_parses_cleanly(source: &str) {
        use crate::traits::ParserTrait;
        // Mirror the trailing-newline normalisation `check_func_space` does
        // before handing input to the parser, so this helper sees the same
        // bytes the metric tests do.
        let path = std::path::PathBuf::from("foo.pl");
        let mut bytes = source.trim_end_matches('\n').as_bytes().to_vec();
        bytes.push(b'\n');
        let parser = PerlParser::new(bytes, &path, None);
        assert!(
            !parser.get_root().has_error(),
            "tree-sitter-perl returned an error tree for snippet:\n{source}"
        );
    }

    #[test]
    fn python_sloc() {
        check_metrics::<PythonParser>(
            "

            a = 42

            ",
            "foo.py",
            |metric| {
                // Spaces: 1
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r###"
                    {
                      "sloc": 1.0,
                      "ploc": 1.0,
                      "lloc": 1.0,
                      "cloc": 0.0,
                      "blank": 0.0,
                      "sloc_average": 1.0,
                      "ploc_average": 1.0,
                      "lloc_average": 1.0,
                      "cloc_average": 0.0,
                      "blank_average": 0.0,
                      "sloc_min": 1.0,
                      "sloc_max": 1.0,
                      "cloc_min": 0.0,
                      "cloc_max": 0.0,
                      "ploc_min": 1.0,
                      "ploc_max": 1.0,
                      "lloc_min": 1.0,
                      "lloc_max": 1.0,
                      "blank_min": 0.0,
                      "blank_max": 0.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn python_blank() {
        check_metrics::<PythonParser>(
            "
            a = 42

            b = 43

            ",
            "foo.py",
            |metric| {
                // Spaces: 1
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r###"
                    {
                      "sloc": 3.0,
                      "ploc": 2.0,
                      "lloc": 2.0,
                      "cloc": 0.0,
                      "blank": 1.0,
                      "sloc_average": 3.0,
                      "ploc_average": 2.0,
                      "lloc_average": 2.0,
                      "cloc_average": 0.0,
                      "blank_average": 1.0,
                      "sloc_min": 3.0,
                      "sloc_max": 3.0,
                      "cloc_min": 0.0,
                      "cloc_max": 0.0,
                      "ploc_min": 2.0,
                      "ploc_max": 2.0,
                      "lloc_min": 2.0,
                      "lloc_max": 2.0,
                      "blank_min": 1.0,
                      "blank_max": 1.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn rust_blank() {
        check_metrics::<RustParser>(
            "

            let a = 42;

            let b = 43;

            ",
            "foo.rs",
            |metric| {
                // Spaces: 1
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r###"
                    {
                      "sloc": 3.0,
                      "ploc": 2.0,
                      "lloc": 2.0,
                      "cloc": 0.0,
                      "blank": 1.0,
                      "sloc_average": 3.0,
                      "ploc_average": 2.0,
                      "lloc_average": 2.0,
                      "cloc_average": 0.0,
                      "blank_average": 1.0,
                      "sloc_min": 3.0,
                      "sloc_max": 3.0,
                      "cloc_min": 0.0,
                      "cloc_max": 0.0,
                      "ploc_min": 2.0,
                      "ploc_max": 2.0,
                      "lloc_min": 2.0,
                      "lloc_max": 2.0,
                      "blank_min": 1.0,
                      "blank_max": 1.0
                    }"###
                );
            },
        );

        check_metrics::<RustParser>("fn func() { /* comment */ }", "foo.rs", |metric| {
            // Spaces: 2
            insta::assert_json_snapshot!(
                metric.loc,
                @r###"
                    {
                      "sloc": 1.0,
                      "ploc": 1.0,
                      "lloc": 0.0,
                      "cloc": 1.0,
                      "blank": 0.0,
                      "sloc_average": 0.5,
                      "ploc_average": 0.5,
                      "lloc_average": 0.0,
                      "cloc_average": 0.5,
                      "blank_average": 0.0,
                      "sloc_min": 1.0,
                      "sloc_max": 1.0,
                      "cloc_min": 1.0,
                      "cloc_max": 1.0,
                      "ploc_min": 1.0,
                      "ploc_max": 1.0,
                      "lloc_min": 0.0,
                      "lloc_max": 0.0,
                      "blank_min": 0.0,
                      "blank_max": 0.0
                    }"###
            );
        });
    }

    #[test]
    fn c_blank() {
        check_metrics::<CppParser>(
            "

            int a = 42;

            int b = 43;

            ",
            "foo.c",
            |metric| {
                // Spaces: 1
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r###"
                    {
                      "sloc": 3.0,
                      "ploc": 2.0,
                      "lloc": 2.0,
                      "cloc": 0.0,
                      "blank": 1.0,
                      "sloc_average": 3.0,
                      "ploc_average": 2.0,
                      "lloc_average": 2.0,
                      "cloc_average": 0.0,
                      "blank_average": 1.0,
                      "sloc_min": 3.0,
                      "sloc_max": 3.0,
                      "cloc_min": 0.0,
                      "cloc_max": 0.0,
                      "ploc_min": 2.0,
                      "ploc_max": 2.0,
                      "lloc_min": 2.0,
                      "lloc_max": 2.0,
                      "blank_min": 1.0,
                      "blank_max": 1.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn python_no_zero_blank() {
        // Checks that the blank metric is not equal to 0 when there are some
        // comments next to code lines.
        check_metrics::<PythonParser>(
            "def ConnectToUpdateServer():
                 pool = 4

                 updateServer = -42
                 isConnected = False
                 currTry = 0
                 numRetries = 10 # Number of IPC connection retries before
                                 # giving up.
                 numTries = 20 # Number of IPC connection tries before
                               # giving up.",
            "foo.py",
            |metric| {
                // Spaces: 2
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r###"
                    {
                      "sloc": 10.0,
                      "ploc": 7.0,
                      "lloc": 6.0,
                      "cloc": 4.0,
                      "blank": 1.0,
                      "sloc_average": 5.0,
                      "ploc_average": 3.5,
                      "lloc_average": 3.0,
                      "cloc_average": 2.0,
                      "blank_average": 0.5,
                      "sloc_min": 10.0,
                      "sloc_max": 10.0,
                      "cloc_min": 4.0,
                      "cloc_max": 4.0,
                      "ploc_min": 7.0,
                      "ploc_max": 7.0,
                      "lloc_min": 6.0,
                      "lloc_max": 6.0,
                      "blank_min": 1.0,
                      "blank_max": 1.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn python_no_blank() {
        // Checks that the blank metric is equal to 0 when there are no blank
        // lines and there are comments next to code lines.
        check_metrics::<PythonParser>(
            "def ConnectToUpdateServer():
                 pool = 4
                 updateServer = -42
                 isConnected = False
                 currTry = 0
                 numRetries = 10 # Number of IPC connection retries before
                                 # giving up.
                 numTries = 20 # Number of IPC connection tries before
                               # giving up.",
            "foo.py",
            |metric| {
                // Spaces: 2
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r###"
                    {
                      "sloc": 9.0,
                      "ploc": 7.0,
                      "lloc": 6.0,
                      "cloc": 4.0,
                      "blank": 0.0,
                      "sloc_average": 4.5,
                      "ploc_average": 3.5,
                      "lloc_average": 3.0,
                      "cloc_average": 2.0,
                      "blank_average": 0.0,
                      "sloc_min": 9.0,
                      "sloc_max": 9.0,
                      "cloc_min": 4.0,
                      "cloc_max": 4.0,
                      "ploc_min": 7.0,
                      "ploc_max": 7.0,
                      "lloc_min": 6.0,
                      "lloc_max": 6.0,
                      "blank_min": 0.0,
                      "blank_max": 0.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn python_no_zero_blank_more_comments() {
        // Checks that the blank metric is not equal to 0 when there are more
        // comments next to code lines compared to the previous tests.
        check_metrics::<PythonParser>(
            "def ConnectToUpdateServer():
                 pool = 4

                 updateServer = -42
                 isConnected = False
                 currTry = 0 # Set this variable to 0
                 numRetries = 10 # Number of IPC connection retries before
                                 # giving up.
                 numTries = 20 # Number of IPC connection tries before
                               # giving up.",
            "foo.py",
            |metric| {
                // Spaces: 2
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r###"
                    {
                      "sloc": 10.0,
                      "ploc": 7.0,
                      "lloc": 6.0,
                      "cloc": 5.0,
                      "blank": 1.0,
                      "sloc_average": 5.0,
                      "ploc_average": 3.5,
                      "lloc_average": 3.0,
                      "cloc_average": 2.5,
                      "blank_average": 0.5,
                      "sloc_min": 10.0,
                      "sloc_max": 10.0,
                      "cloc_min": 5.0,
                      "cloc_max": 5.0,
                      "ploc_min": 7.0,
                      "ploc_max": 7.0,
                      "lloc_min": 6.0,
                      "lloc_max": 6.0,
                      "blank_min": 1.0,
                      "blank_max": 1.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn rust_no_zero_blank() {
        // Checks that the blank metric is not equal to 0 when there are some
        // comments next to code lines.
        check_metrics::<RustParser>(
            "fn ConnectToUpdateServer() {
              let pool = 0;

              let updateServer = -42;
              let isConnected = false;
              let currTry = 0;
              let numRetries = 10;  // Number of IPC connection retries before
                                    // giving up.
              let numTries = 20;    // Number of IPC connection tries before
                                    // giving up.
            }",
            "foo.rs",
            |metric| {
                // Spaces: 2
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r###"
                    {
                      "sloc": 11.0,
                      "ploc": 8.0,
                      "lloc": 6.0,
                      "cloc": 4.0,
                      "blank": 1.0,
                      "sloc_average": 5.5,
                      "ploc_average": 4.0,
                      "lloc_average": 3.0,
                      "cloc_average": 2.0,
                      "blank_average": 0.5,
                      "sloc_min": 11.0,
                      "sloc_max": 11.0,
                      "cloc_min": 4.0,
                      "cloc_max": 4.0,
                      "ploc_min": 8.0,
                      "ploc_max": 8.0,
                      "lloc_min": 6.0,
                      "lloc_max": 6.0,
                      "blank_min": 1.0,
                      "blank_max": 1.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn javascript_no_zero_blank() {
        // Checks that the blank metric is not equal to 0 when there are some
        // comments next to code lines.
        check_metrics::<JavascriptParser>(
            "function ConnectToUpdateServer() {
              var pool = 0;

              var updateServer = -42;
              var isConnected = false;
              var currTry = 0;
              var numRetries = 10;  // Number of IPC connection retries before
                                    // giving up.
              var numTries = 20;    // Number of IPC connection tries before
                                    // giving up.
            }",
            "foo.js",
            |metric| {
                // Spaces: 2
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r###"
                    {
                      "sloc": 11.0,
                      "ploc": 8.0,
                      "lloc": 1.0,
                      "cloc": 4.0,
                      "blank": 1.0,
                      "sloc_average": 5.5,
                      "ploc_average": 4.0,
                      "lloc_average": 0.5,
                      "cloc_average": 2.0,
                      "blank_average": 0.5,
                      "sloc_min": 11.0,
                      "sloc_max": 11.0,
                      "cloc_min": 4.0,
                      "cloc_max": 4.0,
                      "ploc_min": 8.0,
                      "ploc_max": 8.0,
                      "lloc_min": 1.0,
                      "lloc_max": 1.0,
                      "blank_min": 1.0,
                      "blank_max": 1.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn cpp_no_zero_blank() {
        // Checks that the blank metric is not equal to 0 when there are some
        // comments next to code lines.
        check_metrics::<CppParser>(
            "void ConnectToUpdateServer() {
              int pool;

              int updateServer = -42;
              bool isConnected = false;
              int currTry = 0;
              const int numRetries = 10; // Number of IPC connection retries before
                                         // giving up.
              const int numTries = 20; // Number of IPC connection tries before
                                       // giving up.
            }",
            "foo.cpp",
            |metric| {
                // Spaces: 2
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r###"
                    {
                      "sloc": 11.0,
                      "ploc": 8.0,
                      "lloc": 6.0,
                      "cloc": 4.0,
                      "blank": 1.0,
                      "sloc_average": 5.5,
                      "ploc_average": 4.0,
                      "lloc_average": 3.0,
                      "cloc_average": 2.0,
                      "blank_average": 0.5,
                      "sloc_min": 11.0,
                      "sloc_max": 11.0,
                      "cloc_min": 4.0,
                      "cloc_max": 4.0,
                      "ploc_min": 8.0,
                      "ploc_max": 8.0,
                      "lloc_min": 6.0,
                      "lloc_max": 6.0,
                      "blank_min": 1.0,
                      "blank_max": 1.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn cpp_code_line_start_block_blank() {
        // Checks that the blank metric is equal to 1 when there are
        // block comments starting next to code lines.
        check_metrics::<CppParser>(
            "void ConnectToUpdateServer() {
              int pool;

              int updateServer = -42;
              bool isConnected = false;
              int currTry = 0;
              const int numRetries = 10; /* Number of IPC connection retries
              before
              giving up. */
              const int numTries = 20; // Number of IPC connection tries before
                                       // giving up.
            }",
            "foo.cpp",
            |metric| {
                // Spaces: 2
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r###"
                    {
                      "sloc": 12.0,
                      "ploc": 8.0,
                      "lloc": 6.0,
                      "cloc": 5.0,
                      "blank": 1.0,
                      "sloc_average": 6.0,
                      "ploc_average": 4.0,
                      "lloc_average": 3.0,
                      "cloc_average": 2.5,
                      "blank_average": 0.5,
                      "sloc_min": 12.0,
                      "sloc_max": 12.0,
                      "cloc_min": 5.0,
                      "cloc_max": 5.0,
                      "ploc_min": 8.0,
                      "ploc_max": 8.0,
                      "lloc_min": 6.0,
                      "lloc_max": 6.0,
                      "blank_min": 1.0,
                      "blank_max": 1.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn cpp_block_comment_blank() {
        // Checks that the blank metric is equal to 1 when there are
        // block comments on independent lines.
        check_metrics::<CppParser>(
            "void ConnectToUpdateServer() {
              int pool;

              int updateServer = -42;
              bool isConnected = false;
              int currTry = 0;
              /* Number of IPC connection retries
              before
              giving up. */
              const int numRetries = 10;
              const int numTries = 20; // Number of IPC connection tries before
                                       // giving up.
            }",
            "foo.cpp",
            |metric| {
                // Spaces: 2
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r###"
                    {
                      "sloc": 13.0,
                      "ploc": 8.0,
                      "lloc": 6.0,
                      "cloc": 5.0,
                      "blank": 1.0,
                      "sloc_average": 6.5,
                      "ploc_average": 4.0,
                      "lloc_average": 3.0,
                      "cloc_average": 2.5,
                      "blank_average": 0.5,
                      "sloc_min": 13.0,
                      "sloc_max": 13.0,
                      "cloc_min": 5.0,
                      "cloc_max": 5.0,
                      "ploc_min": 8.0,
                      "ploc_max": 8.0,
                      "lloc_min": 6.0,
                      "lloc_max": 6.0,
                      "blank_min": 1.0,
                      "blank_max": 1.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn cpp_code_line_block_one_line_blank() {
        // Checks that the blank metric is equal to 1 when there are
        // block comments before the same code line.
        check_metrics::<CppParser>(
            "void ConnectToUpdateServer() {
              int pool;

              int updateServer = -42;
              bool isConnected = false;
              int currTry = 0;
              /* Number of IPC connection retries before giving up. */ const int numRetries = 10;
              const int numTries = 20; // Number of IPC connection tries before
                                       // giving up.
            }",
            "foo.cpp",
            |metric| {
                // Spaces: 2
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r###"
                    {
                      "sloc": 10.0,
                      "ploc": 8.0,
                      "lloc": 6.0,
                      "cloc": 3.0,
                      "blank": 1.0,
                      "sloc_average": 5.0,
                      "ploc_average": 4.0,
                      "lloc_average": 3.0,
                      "cloc_average": 1.5,
                      "blank_average": 0.5,
                      "sloc_min": 10.0,
                      "sloc_max": 10.0,
                      "cloc_min": 3.0,
                      "cloc_max": 3.0,
                      "ploc_min": 8.0,
                      "ploc_max": 8.0,
                      "lloc_min": 6.0,
                      "lloc_max": 6.0,
                      "blank_min": 1.0,
                      "blank_max": 1.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn cpp_code_line_end_block_blank() {
        // Checks that the blank metric is equal to 1 when there are
        // block comments ending next to code lines.
        check_metrics::<CppParser>(
            "void ConnectToUpdateServer() {
              int pool;

              int updateServer = -42;
              bool isConnected = false;
              int currTry = 0;
              /* Number of IPC connection retries
              before
              giving up. */ const int numRetries = 10;
              const int numTries = 20; // Number of IPC connection tries before
                                       // giving up.
            }",
            "foo.cpp",
            |metric| {
                // Spaces: 2
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r###"
                    {
                      "sloc": 12.0,
                      "ploc": 8.0,
                      "lloc": 6.0,
                      "cloc": 5.0,
                      "blank": 1.0,
                      "sloc_average": 6.0,
                      "ploc_average": 4.0,
                      "lloc_average": 3.0,
                      "cloc_average": 2.5,
                      "blank_average": 0.5,
                      "sloc_min": 12.0,
                      "sloc_max": 12.0,
                      "cloc_min": 5.0,
                      "cloc_max": 5.0,
                      "ploc_min": 8.0,
                      "ploc_max": 8.0,
                      "lloc_min": 6.0,
                      "lloc_max": 6.0,
                      "blank_min": 1.0,
                      "blank_max": 1.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn python_cloc() {
        check_metrics::<PythonParser>(
            "\"\"\"Block comment
            Block comment
            \"\"\"
            # Line Comment
            a = 42 # Line Comment",
            "foo.py",
            |metric| {
                // Spaces: 1
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r###"
                    {
                      "sloc": 5.0,
                      "ploc": 1.0,
                      "lloc": 2.0,
                      "cloc": 5.0,
                      "blank": 0.0,
                      "sloc_average": 5.0,
                      "ploc_average": 1.0,
                      "lloc_average": 2.0,
                      "cloc_average": 5.0,
                      "blank_average": 0.0,
                      "sloc_min": 5.0,
                      "sloc_max": 5.0,
                      "cloc_min": 5.0,
                      "cloc_max": 5.0,
                      "ploc_min": 1.0,
                      "ploc_max": 1.0,
                      "lloc_min": 2.0,
                      "lloc_max": 2.0,
                      "blank_min": 0.0,
                      "blank_max": 0.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn rust_cloc() {
        check_metrics::<RustParser>(
            "/*Block comment
            Block Comment*/
            //Line Comment
            /*Block Comment*/ let a = 42; // Line Comment",
            "foo.rs",
            |metric| {
                // Spaces: 1
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r###"
                    {
                      "sloc": 4.0,
                      "ploc": 1.0,
                      "lloc": 1.0,
                      "cloc": 5.0,
                      "blank": 0.0,
                      "sloc_average": 4.0,
                      "ploc_average": 1.0,
                      "lloc_average": 1.0,
                      "cloc_average": 5.0,
                      "blank_average": 0.0,
                      "sloc_min": 4.0,
                      "sloc_max": 4.0,
                      "cloc_min": 5.0,
                      "cloc_max": 5.0,
                      "ploc_min": 1.0,
                      "ploc_max": 1.0,
                      "lloc_min": 1.0,
                      "lloc_max": 1.0,
                      "blank_min": 0.0,
                      "blank_max": 0.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn c_cloc() {
        check_metrics::<CppParser>(
            "/*Block comment
            Block Comment*/
            //Line Comment
            /*Block Comment*/ int a = 42; // Line Comment",
            "foo.c",
            |metric| {
                // Spaces: 1
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r###"
                    {
                      "sloc": 4.0,
                      "ploc": 1.0,
                      "lloc": 1.0,
                      "cloc": 5.0,
                      "blank": 0.0,
                      "sloc_average": 4.0,
                      "ploc_average": 1.0,
                      "lloc_average": 1.0,
                      "cloc_average": 5.0,
                      "blank_average": 0.0,
                      "sloc_min": 4.0,
                      "sloc_max": 4.0,
                      "cloc_min": 5.0,
                      "cloc_max": 5.0,
                      "ploc_min": 1.0,
                      "ploc_max": 1.0,
                      "lloc_min": 1.0,
                      "lloc_max": 1.0,
                      "blank_min": 0.0,
                      "blank_max": 0.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn python_lloc() {
        check_metrics::<PythonParser>(
            "for x in range(0,42):
                if x % 2 == 0:
                    print(x)",
            "foo.py",
            |metric| {
                // Spaces: 1
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r###"
                    {
                      "sloc": 3.0,
                      "ploc": 3.0,
                      "lloc": 3.0,
                      "cloc": 0.0,
                      "blank": 0.0,
                      "sloc_average": 3.0,
                      "ploc_average": 3.0,
                      "lloc_average": 3.0,
                      "cloc_average": 0.0,
                      "blank_average": 0.0,
                      "sloc_min": 3.0,
                      "sloc_max": 3.0,
                      "cloc_min": 0.0,
                      "cloc_max": 0.0,
                      "ploc_min": 3.0,
                      "ploc_max": 3.0,
                      "lloc_min": 3.0,
                      "lloc_max": 3.0,
                      "blank_min": 0.0,
                      "blank_max": 0.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn rust_lloc() {
        check_metrics::<RustParser>(
            "for x in 0..42 {
                if x % 2 == 0 {
                    println!(\"{}\", x);
                }
             }",
            "foo.rs",
            |metric| {
                // Spaces: 1
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r###"
                    {
                      "sloc": 5.0,
                      "ploc": 5.0,
                      "lloc": 3.0,
                      "cloc": 0.0,
                      "blank": 0.0,
                      "sloc_average": 5.0,
                      "ploc_average": 5.0,
                      "lloc_average": 3.0,
                      "cloc_average": 0.0,
                      "blank_average": 0.0,
                      "sloc_min": 5.0,
                      "sloc_max": 5.0,
                      "cloc_min": 0.0,
                      "cloc_max": 0.0,
                      "ploc_min": 5.0,
                      "ploc_max": 5.0,
                      "lloc_min": 3.0,
                      "lloc_max": 3.0,
                      "blank_min": 0.0,
                      "blank_max": 0.0
                    }"###
                );
            },
        );

        // LLOC returns three because there is an empty Rust statement
        check_metrics::<RustParser>(
            "let a = 42;
             if true {
                42
             } else {
                43
             };",
            "foo.rs",
            |metric| {
                // Spaces: 1
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r###"
                    {
                      "sloc": 6.0,
                      "ploc": 6.0,
                      "lloc": 3.0,
                      "cloc": 0.0,
                      "blank": 0.0,
                      "sloc_average": 6.0,
                      "ploc_average": 6.0,
                      "lloc_average": 3.0,
                      "cloc_average": 0.0,
                      "blank_average": 0.0,
                      "sloc_min": 6.0,
                      "sloc_max": 6.0,
                      "cloc_min": 0.0,
                      "cloc_max": 0.0,
                      "ploc_min": 6.0,
                      "ploc_max": 6.0,
                      "lloc_min": 3.0,
                      "lloc_max": 3.0,
                      "blank_min": 0.0,
                      "blank_max": 0.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn c_lloc() {
        check_metrics::<CppParser>(
            "for (;;)
                break;",
            "foo.c",
            |metric| {
                // Spaces: 1
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r###"
                    {
                      "sloc": 2.0,
                      "ploc": 2.0,
                      "lloc": 2.0,
                      "cloc": 0.0,
                      "blank": 0.0,
                      "sloc_average": 2.0,
                      "ploc_average": 2.0,
                      "lloc_average": 2.0,
                      "cloc_average": 0.0,
                      "blank_average": 0.0,
                      "sloc_min": 2.0,
                      "sloc_max": 2.0,
                      "cloc_min": 0.0,
                      "cloc_max": 0.0,
                      "ploc_min": 2.0,
                      "ploc_max": 2.0,
                      "lloc_min": 2.0,
                      "lloc_max": 2.0,
                      "blank_min": 0.0,
                      "blank_max": 0.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn cpp_lloc() {
        check_metrics::<CppParser>(
            "nsTArray<xpcGCCallback> callbacks(extraGCCallbacks.Clone());
             for (uint32_t i = 0; i < callbacks.Length(); ++i) {
                 callbacks[i](status);
             }",
            "foo.cpp",
            |metric| {
                // Spaces: 1
                // lloc: nsTArray, for, callbacks
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r###"
                    {
                      "sloc": 4.0,
                      "ploc": 4.0,
                      "lloc": 3.0,
                      "cloc": 0.0,
                      "blank": 0.0,
                      "sloc_average": 4.0,
                      "ploc_average": 4.0,
                      "lloc_average": 3.0,
                      "cloc_average": 0.0,
                      "blank_average": 0.0,
                      "sloc_min": 4.0,
                      "sloc_max": 4.0,
                      "cloc_min": 0.0,
                      "cloc_max": 0.0,
                      "ploc_min": 4.0,
                      "ploc_max": 4.0,
                      "lloc_min": 3.0,
                      "lloc_max": 3.0,
                      "blank_min": 0.0,
                      "blank_max": 0.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn cpp_return_lloc() {
        check_metrics::<CppParser>(
            "uint8_t* pixel_data = frame.GetFrameDataAtPos(DesktopVector(x, y));
             return RgbaColor(pixel_data) == blank_pixel_;",
            "foo.cpp",
            |metric| {
                // Spaces: 1
                // lloc: pixel_data, return
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r###"
                    {
                      "sloc": 2.0,
                      "ploc": 2.0,
                      "lloc": 2.0,
                      "cloc": 0.0,
                      "blank": 0.0,
                      "sloc_average": 2.0,
                      "ploc_average": 2.0,
                      "lloc_average": 2.0,
                      "cloc_average": 0.0,
                      "blank_average": 0.0,
                      "sloc_min": 2.0,
                      "sloc_max": 2.0,
                      "cloc_min": 0.0,
                      "cloc_max": 0.0,
                      "ploc_min": 2.0,
                      "ploc_max": 2.0,
                      "lloc_min": 2.0,
                      "lloc_max": 2.0,
                      "blank_min": 0.0,
                      "blank_max": 0.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn cpp_for_lloc() {
        check_metrics::<CppParser>(
            "for (; start != end; ++start) {
                 const unsigned char idx = *start;
                 if (idx > 127 || !kValidTokenMap[idx]) return false;
             }",
            "foo.cpp",
            |metric| {
                // Spaces: 1
                // lloc: for, idx, if, return
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r###"
                    {
                      "sloc": 4.0,
                      "ploc": 4.0,
                      "lloc": 4.0,
                      "cloc": 0.0,
                      "blank": 0.0,
                      "sloc_average": 4.0,
                      "ploc_average": 4.0,
                      "lloc_average": 4.0,
                      "cloc_average": 0.0,
                      "blank_average": 0.0,
                      "sloc_min": 4.0,
                      "sloc_max": 4.0,
                      "cloc_min": 0.0,
                      "cloc_max": 0.0,
                      "ploc_min": 4.0,
                      "ploc_max": 4.0,
                      "lloc_min": 4.0,
                      "lloc_max": 4.0,
                      "blank_min": 0.0,
                      "blank_max": 0.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn cpp_while_lloc() {
        check_metrics::<CppParser>(
            "while (sHeapAtoms) {
                 HttpHeapAtom* next = sHeapAtoms->next;
                 free(sHeapAtoms);
            }",
            "foo.cpp",
            |metric| {
                // Spaces: 1
                // lloc: while, next, free
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r###"
                    {
                      "sloc": 4.0,
                      "ploc": 4.0,
                      "lloc": 3.0,
                      "cloc": 0.0,
                      "blank": 0.0,
                      "sloc_average": 4.0,
                      "ploc_average": 4.0,
                      "lloc_average": 3.0,
                      "cloc_average": 0.0,
                      "blank_average": 0.0,
                      "sloc_min": 4.0,
                      "sloc_max": 4.0,
                      "cloc_min": 0.0,
                      "cloc_max": 0.0,
                      "ploc_min": 4.0,
                      "ploc_max": 4.0,
                      "lloc_min": 3.0,
                      "lloc_max": 3.0,
                      "blank_min": 0.0,
                      "blank_max": 0.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn python_string_on_new_line() {
        // More lines of the same instruction were counted as blank lines
        check_metrics::<PythonParser>(
            "capabilities[\"goog:chromeOptions\"][\"androidPackage\"] = \\
                \"org.chromium.weblayer.shell\"",
            "foo.py",
            |metric| {
                // Spaces: 1
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r###"
                    {
                      "sloc": 2.0,
                      "ploc": 2.0,
                      "lloc": 1.0,
                      "cloc": 0.0,
                      "blank": 0.0,
                      "sloc_average": 2.0,
                      "ploc_average": 2.0,
                      "lloc_average": 1.0,
                      "cloc_average": 0.0,
                      "blank_average": 0.0,
                      "sloc_min": 2.0,
                      "sloc_max": 2.0,
                      "cloc_min": 0.0,
                      "cloc_max": 0.0,
                      "ploc_min": 2.0,
                      "ploc_max": 2.0,
                      "lloc_min": 1.0,
                      "lloc_max": 1.0,
                      "blank_min": 0.0,
                      "blank_max": 0.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn rust_no_field_expression_lloc() {
        check_metrics::<RustParser>(
            "struct Foo {
                field: usize,
             }
             let foo = Foo { 42 };
             foo.field;",
            "foo.rs",
            |metric| {
                // Spaces: 1
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r###"
                    {
                      "sloc": 5.0,
                      "ploc": 5.0,
                      "lloc": 2.0,
                      "cloc": 0.0,
                      "blank": 0.0,
                      "sloc_average": 5.0,
                      "ploc_average": 5.0,
                      "lloc_average": 2.0,
                      "cloc_average": 0.0,
                      "blank_average": 0.0,
                      "sloc_min": 5.0,
                      "sloc_max": 5.0,
                      "cloc_min": 0.0,
                      "cloc_max": 0.0,
                      "ploc_min": 5.0,
                      "ploc_max": 5.0,
                      "lloc_min": 2.0,
                      "lloc_max": 2.0,
                      "blank_min": 0.0,
                      "blank_max": 0.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn rust_no_parenthesized_expression_lloc() {
        check_metrics::<RustParser>("let a = (42 + 0);", "foo.rs", |metric| {
            // Spaces: 1
            insta::assert_json_snapshot!(
                metric.loc,
                @r###"
                    {
                      "sloc": 1.0,
                      "ploc": 1.0,
                      "lloc": 1.0,
                      "cloc": 0.0,
                      "blank": 0.0,
                      "sloc_average": 1.0,
                      "ploc_average": 1.0,
                      "lloc_average": 1.0,
                      "cloc_average": 0.0,
                      "blank_average": 0.0,
                      "sloc_min": 1.0,
                      "sloc_max": 1.0,
                      "cloc_min": 0.0,
                      "cloc_max": 0.0,
                      "ploc_min": 1.0,
                      "ploc_max": 1.0,
                      "lloc_min": 1.0,
                      "lloc_max": 1.0,
                      "blank_min": 0.0,
                      "blank_max": 0.0
                    }"###
            );
        });
    }

    #[test]
    fn rust_no_array_expression_lloc() {
        check_metrics::<RustParser>("let a = [0; 42];", "foo.rs", |metric| {
            // Spaces: 1
            insta::assert_json_snapshot!(
                metric.loc,
                @r###"
                    {
                      "sloc": 1.0,
                      "ploc": 1.0,
                      "lloc": 1.0,
                      "cloc": 0.0,
                      "blank": 0.0,
                      "sloc_average": 1.0,
                      "ploc_average": 1.0,
                      "lloc_average": 1.0,
                      "cloc_average": 0.0,
                      "blank_average": 0.0,
                      "sloc_min": 1.0,
                      "sloc_max": 1.0,
                      "cloc_min": 0.0,
                      "cloc_max": 0.0,
                      "ploc_min": 1.0,
                      "ploc_max": 1.0,
                      "lloc_min": 1.0,
                      "lloc_max": 1.0,
                      "blank_min": 0.0,
                      "blank_max": 0.0
                    }"###
            );
        });
    }

    #[test]
    fn rust_no_tuple_expression_lloc() {
        check_metrics::<RustParser>("let a = (0, 42);", "foo.rs", |metric| {
            // Spaces: 1
            insta::assert_json_snapshot!(
                metric.loc,
                @r###"
                    {
                      "sloc": 1.0,
                      "ploc": 1.0,
                      "lloc": 1.0,
                      "cloc": 0.0,
                      "blank": 0.0,
                      "sloc_average": 1.0,
                      "ploc_average": 1.0,
                      "lloc_average": 1.0,
                      "cloc_average": 0.0,
                      "blank_average": 0.0,
                      "sloc_min": 1.0,
                      "sloc_max": 1.0,
                      "cloc_min": 0.0,
                      "cloc_max": 0.0,
                      "ploc_min": 1.0,
                      "ploc_max": 1.0,
                      "lloc_min": 1.0,
                      "lloc_max": 1.0,
                      "blank_min": 0.0,
                      "blank_max": 0.0
                    }"###
            );
        });
    }

    #[test]
    fn rust_no_unit_expression_lloc() {
        check_metrics::<RustParser>("let a = ();", "foo.rs", |metric| {
            // Spaces: 1
            insta::assert_json_snapshot!(
                metric.loc,
                @r###"
                    {
                      "sloc": 1.0,
                      "ploc": 1.0,
                      "lloc": 1.0,
                      "cloc": 0.0,
                      "blank": 0.0,
                      "sloc_average": 1.0,
                      "ploc_average": 1.0,
                      "lloc_average": 1.0,
                      "cloc_average": 0.0,
                      "blank_average": 0.0,
                      "sloc_min": 1.0,
                      "sloc_max": 1.0,
                      "cloc_min": 0.0,
                      "cloc_max": 0.0,
                      "ploc_min": 1.0,
                      "ploc_max": 1.0,
                      "lloc_min": 1.0,
                      "lloc_max": 1.0,
                      "blank_min": 0.0,
                      "blank_max": 0.0
                    }"###
            );
        });
    }

    #[test]
    fn rust_call_function_lloc() {
        check_metrics::<RustParser>(
            "let a = foo(); // +1
             foo(); // +1
             k!(foo()); // +1",
            "foo.rs",
            |metric| {
                // Spaces: 1
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r###"
                    {
                      "sloc": 3.0,
                      "ploc": 3.0,
                      "lloc": 3.0,
                      "cloc": 3.0,
                      "blank": 0.0,
                      "sloc_average": 3.0,
                      "ploc_average": 3.0,
                      "lloc_average": 3.0,
                      "cloc_average": 3.0,
                      "blank_average": 0.0,
                      "sloc_min": 3.0,
                      "sloc_max": 3.0,
                      "cloc_min": 3.0,
                      "cloc_max": 3.0,
                      "ploc_min": 3.0,
                      "ploc_max": 3.0,
                      "lloc_min": 3.0,
                      "lloc_max": 3.0,
                      "blank_min": 0.0,
                      "blank_max": 0.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn rust_macro_invocation_lloc() {
        check_metrics::<RustParser>(
            "let a = foo!(); // +1
             foo!(); // +1
             k(foo!()); // +1",
            "foo.rs",
            |metric| {
                // Spaces: 1
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r###"
                    {
                      "sloc": 3.0,
                      "ploc": 3.0,
                      "lloc": 3.0,
                      "cloc": 3.0,
                      "blank": 0.0,
                      "sloc_average": 3.0,
                      "ploc_average": 3.0,
                      "lloc_average": 3.0,
                      "cloc_average": 3.0,
                      "blank_average": 0.0,
                      "sloc_min": 3.0,
                      "sloc_max": 3.0,
                      "cloc_min": 3.0,
                      "cloc_max": 3.0,
                      "ploc_min": 3.0,
                      "ploc_max": 3.0,
                      "lloc_min": 3.0,
                      "lloc_max": 3.0,
                      "blank_min": 0.0,
                      "blank_max": 0.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn rust_function_in_loop_lloc() {
        check_metrics::<RustParser>(
            "for (a, b) in c.iter().enumerate() {} // +1
             while (a, b) in c.iter().enumerate() {} // +1
             while let Some(a) = c.strip_prefix(\"hi\") {} // +1",
            "foo.rs",
            |metric| {
                // Spaces: 1
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r###"
                    {
                      "sloc": 3.0,
                      "ploc": 3.0,
                      "lloc": 3.0,
                      "cloc": 3.0,
                      "blank": 0.0,
                      "sloc_average": 3.0,
                      "ploc_average": 3.0,
                      "lloc_average": 3.0,
                      "cloc_average": 3.0,
                      "blank_average": 0.0,
                      "sloc_min": 3.0,
                      "sloc_max": 3.0,
                      "cloc_min": 3.0,
                      "cloc_max": 3.0,
                      "ploc_min": 3.0,
                      "ploc_max": 3.0,
                      "lloc_min": 3.0,
                      "lloc_max": 3.0,
                      "blank_min": 0.0,
                      "blank_max": 0.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn rust_function_in_if_lloc() {
        check_metrics::<RustParser>(
            "if foo() {} // +1
             if let Some(a) = foo() {} // +1",
            "foo.rs",
            |metric| {
                // Spaces: 1
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r###"
                    {
                      "sloc": 2.0,
                      "ploc": 2.0,
                      "lloc": 2.0,
                      "cloc": 2.0,
                      "blank": 0.0,
                      "sloc_average": 2.0,
                      "ploc_average": 2.0,
                      "lloc_average": 2.0,
                      "cloc_average": 2.0,
                      "blank_average": 0.0,
                      "sloc_min": 2.0,
                      "sloc_max": 2.0,
                      "cloc_min": 2.0,
                      "cloc_max": 2.0,
                      "ploc_min": 2.0,
                      "ploc_max": 2.0,
                      "lloc_min": 2.0,
                      "lloc_max": 2.0,
                      "blank_min": 0.0,
                      "blank_max": 0.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn rust_function_in_return_lloc() {
        check_metrics::<RustParser>(
            "return foo();
             await foo();",
            "foo.rs",
            |metric| {
                // Spaces: 1
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r###"
                    {
                      "sloc": 2.0,
                      "ploc": 2.0,
                      "lloc": 2.0,
                      "cloc": 0.0,
                      "blank": 0.0,
                      "sloc_average": 2.0,
                      "ploc_average": 2.0,
                      "lloc_average": 2.0,
                      "cloc_average": 0.0,
                      "blank_average": 0.0,
                      "sloc_min": 2.0,
                      "sloc_max": 2.0,
                      "cloc_min": 0.0,
                      "cloc_max": 0.0,
                      "ploc_min": 2.0,
                      "ploc_max": 2.0,
                      "lloc_min": 2.0,
                      "lloc_max": 2.0,
                      "blank_min": 0.0,
                      "blank_max": 0.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn rust_closure_expression_lloc() {
        check_metrics::<RustParser>(
            "let a = |i: i32| -> i32 { i + 1 }; // +1
             a(42); // +1
             k(b.iter().map(|n| n.parse.ok().unwrap_or(42))); // +1",
            "foo.rs",
            |metric| {
                // Spaces: 3
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r###"
                    {
                      "sloc": 3.0,
                      "ploc": 3.0,
                      "lloc": 3.0,
                      "cloc": 3.0,
                      "blank": 0.0,
                      "sloc_average": 1.0,
                      "ploc_average": 1.0,
                      "lloc_average": 1.0,
                      "cloc_average": 1.0,
                      "blank_average": 0.0,
                      "sloc_min": 1.0,
                      "sloc_max": 1.0,
                      "cloc_min": 0.0,
                      "cloc_max": 0.0,
                      "ploc_min": 1.0,
                      "ploc_max": 1.0,
                      "lloc_min": 0.0,
                      "lloc_max": 0.0,
                      "blank_min": 0.0,
                      "blank_max": 0.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn python_general_loc() {
        check_metrics::<PythonParser>(
            "def func(a,
                      b,
                      c):
                 print(a)
                 print(b)
                 print(c)",
            "foo.py",
            |metric| {
                // Spaces: 2
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r###"
                    {
                      "sloc": 6.0,
                      "ploc": 6.0,
                      "lloc": 3.0,
                      "cloc": 0.0,
                      "blank": 0.0,
                      "sloc_average": 3.0,
                      "ploc_average": 3.0,
                      "lloc_average": 1.5,
                      "cloc_average": 0.0,
                      "blank_average": 0.0,
                      "sloc_min": 6.0,
                      "sloc_max": 6.0,
                      "cloc_min": 0.0,
                      "cloc_max": 0.0,
                      "ploc_min": 6.0,
                      "ploc_max": 6.0,
                      "lloc_min": 3.0,
                      "lloc_max": 3.0,
                      "blank_min": 0.0,
                      "blank_max": 0.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn python_real_loc() {
        check_metrics::<PythonParser>(
            "def web_socket_transfer_data(request):
                while True:
                    line = request.ws_stream.receive_message()
                    if line is None:
                        return
                    code, reason = line.split(' ', 1)
                    if code is None or reason is None:
                        return
                    request.ws_stream.close_connection(int(code), reason)
                    # close_connection() initiates closing handshake. It validates code
                    # and reason. If you want to send a broken close frame for a test,
                    # following code will be useful.
                    # > data = struct.pack('!H', int(code)) + reason.encode('UTF-8')
                    # > request.connection.write(stream.create_close_frame(data))
                    # > # Suppress to re-respond client responding close frame.
                    # > raise Exception(\"customized server initiated closing handshake\")",
            "foo.py",
            |metric| {
                // Spaces: 2
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r###"
                    {
                      "sloc": 16.0,
                      "ploc": 9.0,
                      "lloc": 8.0,
                      "cloc": 7.0,
                      "blank": 0.0,
                      "sloc_average": 8.0,
                      "ploc_average": 4.5,
                      "lloc_average": 4.0,
                      "cloc_average": 3.5,
                      "blank_average": 0.0,
                      "sloc_min": 16.0,
                      "sloc_max": 16.0,
                      "cloc_min": 7.0,
                      "cloc_max": 7.0,
                      "ploc_min": 9.0,
                      "ploc_max": 9.0,
                      "lloc_min": 8.0,
                      "lloc_max": 8.0,
                      "blank_min": 0.0,
                      "blank_max": 0.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn javascript_real_loc() {
        check_metrics::<JavascriptParser>(
            "assert.throws(Test262Error, function() {
               for (let { poisoned: x = ++initEvalCount } = poisonedProperty; ; ) {
                 return;
               }
             });",
            "foo.js",
            |metric| {
                // Spaces: 2
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r###"
                    {
                      "sloc": 5.0,
                      "ploc": 5.0,
                      "lloc": 6.0,
                      "cloc": 0.0,
                      "blank": 0.0,
                      "sloc_average": 2.5,
                      "ploc_average": 2.5,
                      "lloc_average": 3.0,
                      "cloc_average": 0.0,
                      "blank_average": 0.0,
                      "sloc_min": 5.0,
                      "sloc_max": 5.0,
                      "cloc_min": 0.0,
                      "cloc_max": 0.0,
                      "ploc_min": 5.0,
                      "ploc_max": 5.0,
                      "lloc_min": 5.0,
                      "lloc_max": 5.0,
                      "blank_min": 0.0,
                      "blank_max": 0.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn mozjs_real_loc() {
        check_metrics::<MozjsParser>(
            "assert.throws(Test262Error, function() {
               for (let { poisoned: x = ++initEvalCount } = poisonedProperty; ; ) {
                 return;
               }
             });",
            "foo.js",
            |metric| {
                // Spaces: 2
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r###"
                    {
                      "sloc": 5.0,
                      "ploc": 5.0,
                      "lloc": 6.0,
                      "cloc": 0.0,
                      "blank": 0.0,
                      "sloc_average": 2.5,
                      "ploc_average": 2.5,
                      "lloc_average": 3.0,
                      "cloc_average": 0.0,
                      "blank_average": 0.0,
                      "sloc_min": 5.0,
                      "sloc_max": 5.0,
                      "cloc_min": 0.0,
                      "cloc_max": 0.0,
                      "ploc_min": 5.0,
                      "ploc_max": 5.0,
                      "lloc_min": 5.0,
                      "lloc_max": 5.0,
                      "blank_min": 0.0,
                      "blank_max": 0.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn mozjs_blank_and_comment_loc() {
        check_metrics::<MozjsParser>(
            "// a comment
             function f() {

                 var x = 1;

             }",
            "foo.js",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r###"
                    {
                      "sloc": 6.0,
                      "ploc": 3.0,
                      "lloc": 1.0,
                      "cloc": 1.0,
                      "blank": 2.0,
                      "sloc_average": 3.0,
                      "ploc_average": 1.5,
                      "lloc_average": 0.5,
                      "cloc_average": 0.5,
                      "blank_average": 1.0,
                      "sloc_min": 5.0,
                      "sloc_max": 5.0,
                      "cloc_min": 0.0,
                      "cloc_max": 0.0,
                      "ploc_min": 3.0,
                      "ploc_max": 3.0,
                      "lloc_min": 1.0,
                      "lloc_max": 1.0,
                      "blank_min": 2.0,
                      "blank_max": 2.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn cpp_namespace_loc() {
        check_metrics::<CppParser>(
            "namespace mozilla::dom::quota {} // namespace mozilla::dom::quota",
            "foo.cpp",
            |metric| {
                // Spaces: 2
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r###"
                    {
                      "sloc": 1.0,
                      "ploc": 1.0,
                      "lloc": 0.0,
                      "cloc": 1.0,
                      "blank": 0.0,
                      "sloc_average": 0.5,
                      "ploc_average": 0.5,
                      "lloc_average": 0.0,
                      "cloc_average": 0.5,
                      "blank_average": 0.0,
                      "sloc_min": 1.0,
                      "sloc_max": 1.0,
                      "cloc_min": 0.0,
                      "cloc_max": 0.0,
                      "ploc_min": 1.0,
                      "ploc_max": 1.0,
                      "lloc_min": 0.0,
                      "lloc_max": 0.0,
                      "blank_min": 0.0,
                      "blank_max": 0.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn java_comments() {
        check_metrics::<JavaParser>(
            "for (int i = 0; i < 100; i++) { \
               // Print hello
               System.out.println(\"hello\"); \
               // Print world
               System.out.println(\"hello\"); \
             }",
            "foo.java",
            |metric| {
                // Spaces: 1
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r###"
                    {
                      "sloc": 3.0,
                      "ploc": 3.0,
                      "lloc": 3.0,
                      "cloc": 2.0,
                      "blank": 0.0,
                      "sloc_average": 3.0,
                      "ploc_average": 3.0,
                      "lloc_average": 3.0,
                      "cloc_average": 2.0,
                      "blank_average": 0.0,
                      "sloc_min": 3.0,
                      "sloc_max": 3.0,
                      "cloc_min": 2.0,
                      "cloc_max": 2.0,
                      "ploc_min": 3.0,
                      "ploc_max": 3.0,
                      "lloc_min": 3.0,
                      "lloc_max": 3.0,
                      "blank_min": 0.0,
                      "blank_max": 0.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn java_blank() {
        check_metrics::<JavaParser>(
            "int x = 1;


            int y = 2;",
            "foo.java",
            |metric| {
                // Spaces: 1
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r###"
                    {
                      "sloc": 4.0,
                      "ploc": 2.0,
                      "lloc": 2.0,
                      "cloc": 0.0,
                      "blank": 2.0,
                      "sloc_average": 4.0,
                      "ploc_average": 2.0,
                      "lloc_average": 2.0,
                      "cloc_average": 0.0,
                      "blank_average": 2.0,
                      "sloc_min": 4.0,
                      "sloc_max": 4.0,
                      "cloc_min": 0.0,
                      "cloc_max": 0.0,
                      "ploc_min": 2.0,
                      "ploc_max": 2.0,
                      "lloc_min": 2.0,
                      "lloc_max": 2.0,
                      "blank_min": 2.0,
                      "blank_max": 2.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn java_sloc() {
        check_metrics::<JavaParser>(
            "for (int i = 0; i < 100; i++) {
               System.out.println(i);
             }",
            "foo.java",
            |metric| {
                // Spaces: 1
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r###"
                    {
                      "sloc": 3.0,
                      "ploc": 3.0,
                      "lloc": 2.0,
                      "cloc": 0.0,
                      "blank": 0.0,
                      "sloc_average": 3.0,
                      "ploc_average": 3.0,
                      "lloc_average": 2.0,
                      "cloc_average": 0.0,
                      "blank_average": 0.0,
                      "sloc_min": 3.0,
                      "sloc_max": 3.0,
                      "cloc_min": 0.0,
                      "cloc_max": 0.0,
                      "ploc_min": 3.0,
                      "ploc_max": 3.0,
                      "lloc_min": 2.0,
                      "lloc_max": 2.0,
                      "blank_min": 0.0,
                      "blank_max": 0.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn java_module_sloc() {
        check_metrics::<JavaParser>(
            "module helloworld{
              exports com.test;
            }",
            "foo.java",
            |metric| {
                // Spaces: 1
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r###"
                    {
                      "sloc": 3.0,
                      "ploc": 3.0,
                      "lloc": 0.0,
                      "cloc": 0.0,
                      "blank": 0.0,
                      "sloc_average": 3.0,
                      "ploc_average": 3.0,
                      "lloc_average": 0.0,
                      "cloc_average": 0.0,
                      "blank_average": 0.0,
                      "sloc_min": 3.0,
                      "sloc_max": 3.0,
                      "cloc_min": 0.0,
                      "cloc_max": 0.0,
                      "ploc_min": 3.0,
                      "ploc_max": 3.0,
                      "lloc_min": 0.0,
                      "lloc_max": 0.0,
                      "blank_min": 0.0,
                      "blank_max": 0.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn java_single_ploc() {
        check_metrics::<JavaParser>("int x = 1;", "foo.java", |metric| {
            // Spaces: 1
            insta::assert_json_snapshot!(
                metric.loc,
                @r###"
                    {
                      "sloc": 1.0,
                      "ploc": 1.0,
                      "lloc": 1.0,
                      "cloc": 0.0,
                      "blank": 0.0,
                      "sloc_average": 1.0,
                      "ploc_average": 1.0,
                      "lloc_average": 1.0,
                      "cloc_average": 0.0,
                      "blank_average": 0.0,
                      "sloc_min": 1.0,
                      "sloc_max": 1.0,
                      "cloc_min": 0.0,
                      "cloc_max": 0.0,
                      "ploc_min": 1.0,
                      "ploc_max": 1.0,
                      "lloc_min": 1.0,
                      "lloc_max": 1.0,
                      "blank_min": 0.0,
                      "blank_max": 0.0
                    }"###
            );
        });
    }

    #[test]
    fn java_simple_ploc() {
        check_metrics::<JavaParser>(
            "for (int i = 0; i < 100; i = i++) {
               System.out.println(i);
             }",
            "foo.java",
            |metric| {
                // Spaces: 1
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r###"
                    {
                      "sloc": 3.0,
                      "ploc": 3.0,
                      "lloc": 2.0,
                      "cloc": 0.0,
                      "blank": 0.0,
                      "sloc_average": 3.0,
                      "ploc_average": 3.0,
                      "lloc_average": 2.0,
                      "cloc_average": 0.0,
                      "blank_average": 0.0,
                      "sloc_min": 3.0,
                      "sloc_max": 3.0,
                      "cloc_min": 0.0,
                      "cloc_max": 0.0,
                      "ploc_min": 3.0,
                      "ploc_max": 3.0,
                      "lloc_min": 2.0,
                      "lloc_max": 2.0,
                      "blank_min": 0.0,
                      "blank_max": 0.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn java_multi_ploc() {
        check_metrics::<JavaParser>(
            "int x = 1;
            for (int i = 0; i < 100; i++) {
               System.out.println(i);
             }",
            "foo.java",
            |metric| {
                // Spaces: 1
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r###"
                    {
                      "sloc": 4.0,
                      "ploc": 4.0,
                      "lloc": 3.0,
                      "cloc": 0.0,
                      "blank": 0.0,
                      "sloc_average": 4.0,
                      "ploc_average": 4.0,
                      "lloc_average": 3.0,
                      "cloc_average": 0.0,
                      "blank_average": 0.0,
                      "sloc_min": 4.0,
                      "sloc_max": 4.0,
                      "cloc_min": 0.0,
                      "cloc_max": 0.0,
                      "ploc_min": 4.0,
                      "ploc_max": 4.0,
                      "lloc_min": 3.0,
                      "lloc_max": 3.0,
                      "blank_min": 0.0,
                      "blank_max": 0.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn java_single_statement_lloc() {
        check_metrics::<JavaParser>("int max = 10;", "foo.java", |metric| {
            // Spaces: 1
            insta::assert_json_snapshot!(
                metric.loc,
                @r###"
                    {
                      "sloc": 1.0,
                      "ploc": 1.0,
                      "lloc": 1.0,
                      "cloc": 0.0,
                      "blank": 0.0,
                      "sloc_average": 1.0,
                      "ploc_average": 1.0,
                      "lloc_average": 1.0,
                      "cloc_average": 0.0,
                      "blank_average": 0.0,
                      "sloc_min": 1.0,
                      "sloc_max": 1.0,
                      "cloc_min": 0.0,
                      "cloc_max": 0.0,
                      "ploc_min": 1.0,
                      "ploc_max": 1.0,
                      "lloc_min": 1.0,
                      "lloc_max": 1.0,
                      "blank_min": 0.0,
                      "blank_max": 0.0
                    }"###
            );
        });
    }

    #[test]
    fn java_for_lloc() {
        check_metrics::<JavaParser>(
            "for (int i = 0; i < 100; i++) { // + 1
               System.out.println(i); // + 1
             }",
            "foo.java",
            |metric| {
                // Spaces: 1
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r###"
                    {
                      "sloc": 3.0,
                      "ploc": 3.0,
                      "lloc": 2.0,
                      "cloc": 2.0,
                      "blank": 0.0,
                      "sloc_average": 3.0,
                      "ploc_average": 3.0,
                      "lloc_average": 2.0,
                      "cloc_average": 2.0,
                      "blank_average": 0.0,
                      "sloc_min": 3.0,
                      "sloc_max": 3.0,
                      "cloc_min": 2.0,
                      "cloc_max": 2.0,
                      "ploc_min": 3.0,
                      "ploc_max": 3.0,
                      "lloc_min": 2.0,
                      "lloc_max": 2.0,
                      "blank_min": 0.0,
                      "blank_max": 0.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn java_foreach_lloc() {
        check_metrics::<JavaParser>(
            "
            int arr[]={12,13,14,44}; // +1
            for (int i:arr) { // +1
               System.out.println(i); // +1
             }",
            "foo.java",
            |metric| {
                // Spaces: 1
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r###"
                    {
                      "sloc": 4.0,
                      "ploc": 4.0,
                      "lloc": 3.0,
                      "cloc": 3.0,
                      "blank": 0.0,
                      "sloc_average": 4.0,
                      "ploc_average": 4.0,
                      "lloc_average": 3.0,
                      "cloc_average": 3.0,
                      "blank_average": 0.0,
                      "sloc_min": 4.0,
                      "sloc_max": 4.0,
                      "cloc_min": 3.0,
                      "cloc_max": 3.0,
                      "ploc_min": 4.0,
                      "ploc_max": 4.0,
                      "lloc_min": 3.0,
                      "lloc_max": 3.0,
                      "blank_min": 0.0,
                      "blank_max": 0.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn java_while_lloc() {
        check_metrics::<JavaParser>(
            "
            int i=0; // +1
            while(i < 10) { // +1
                i++; // +1
                System.out.println(i); // +1
             }",
            "foo.java",
            |metric| {
                // Spaces: 1
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r###"
                    {
                      "sloc": 5.0,
                      "ploc": 5.0,
                      "lloc": 4.0,
                      "cloc": 4.0,
                      "blank": 0.0,
                      "sloc_average": 5.0,
                      "ploc_average": 5.0,
                      "lloc_average": 4.0,
                      "cloc_average": 4.0,
                      "blank_average": 0.0,
                      "sloc_min": 5.0,
                      "sloc_max": 5.0,
                      "cloc_min": 4.0,
                      "cloc_max": 4.0,
                      "ploc_min": 5.0,
                      "ploc_max": 5.0,
                      "lloc_min": 4.0,
                      "lloc_max": 4.0,
                      "blank_min": 0.0,
                      "blank_max": 0.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn java_do_while_lloc() {
        check_metrics::<JavaParser>(
            "
            int i=0; // +1
            do { // +1
                i++; // +1
                System.out.println(i); // +1
             } while(i < 10)",
            "foo.java",
            |metric| {
                // Spaces: 1
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r###"
                    {
                      "sloc": 5.0,
                      "ploc": 5.0,
                      "lloc": 4.0,
                      "cloc": 4.0,
                      "blank": 0.0,
                      "sloc_average": 5.0,
                      "ploc_average": 5.0,
                      "lloc_average": 4.0,
                      "cloc_average": 4.0,
                      "blank_average": 0.0,
                      "sloc_min": 5.0,
                      "sloc_max": 5.0,
                      "cloc_min": 4.0,
                      "cloc_max": 4.0,
                      "ploc_min": 5.0,
                      "ploc_max": 5.0,
                      "lloc_min": 4.0,
                      "lloc_max": 4.0,
                      "blank_min": 0.0,
                      "blank_max": 0.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn java_switch_lloc() {
        check_metrics::<JavaParser>(
            "switch(grade) { // +1
                case 'A' :
                   System.out.println(\"Pass with distinction\"); // +1
                   break; // +1
                case 'B' :
                case 'C' :
                   System.out.println(\"Pass\"); // +1
                   break; // +1
                case 'D' :
                   System.out.println(\"At risk\"); // +1
                case 'F' :
                   System.out.println(\"Fail\"); // +1
                   break; // +1
                default :
                   System.out.println(\"Invalid grade\"); // +1
             }",
            "foo.java",
            |metric| {
                // Spaces: 1
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r###"
                    {
                      "sloc": 16.0,
                      "ploc": 16.0,
                      "lloc": 9.0,
                      "cloc": 9.0,
                      "blank": 0.0,
                      "sloc_average": 16.0,
                      "ploc_average": 16.0,
                      "lloc_average": 9.0,
                      "cloc_average": 9.0,
                      "blank_average": 0.0,
                      "sloc_min": 16.0,
                      "sloc_max": 16.0,
                      "cloc_min": 9.0,
                      "cloc_max": 9.0,
                      "ploc_min": 16.0,
                      "ploc_max": 16.0,
                      "lloc_min": 9.0,
                      "lloc_max": 9.0,
                      "blank_min": 0.0,
                      "blank_max": 0.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn java_continue_lloc() {
        check_metrics::<JavaParser>(
            "int max = 10; // +1

            for (int i = 0; i < max; i++) { // +1
                if(i % 2 == 0) { continue;} + 2
                System.out.println(i); // +1
             }",
            "foo.java",
            |metric| {
                // Spaces: 1
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r###"
                    {
                      "sloc": 6.0,
                      "ploc": 5.0,
                      "lloc": 5.0,
                      "cloc": 3.0,
                      "blank": 1.0,
                      "sloc_average": 6.0,
                      "ploc_average": 5.0,
                      "lloc_average": 5.0,
                      "cloc_average": 3.0,
                      "blank_average": 1.0,
                      "sloc_min": 6.0,
                      "sloc_max": 6.0,
                      "cloc_min": 3.0,
                      "cloc_max": 3.0,
                      "ploc_min": 5.0,
                      "ploc_max": 5.0,
                      "lloc_min": 5.0,
                      "lloc_max": 5.0,
                      "blank_min": 1.0,
                      "blank_max": 1.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn java_try_lloc() {
        check_metrics::<JavaParser>(
            "try { // +1
                int[] myNumbers = {1, 2, 3}; // +1
                System.out.println(myNumbers[10]); // +1
              } catch (Exception e) {
                System.out.println(e.getMessage()); // +1
                throw e; // +1
              }",
            "foo.java",
            |metric| {
                // Spaces: 1
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r###"
                    {
                      "sloc": 7.0,
                      "ploc": 7.0,
                      "lloc": 5.0,
                      "cloc": 5.0,
                      "blank": 0.0,
                      "sloc_average": 7.0,
                      "ploc_average": 7.0,
                      "lloc_average": 5.0,
                      "cloc_average": 5.0,
                      "blank_average": 0.0,
                      "sloc_min": 7.0,
                      "sloc_max": 7.0,
                      "cloc_min": 5.0,
                      "cloc_max": 5.0,
                      "ploc_min": 7.0,
                      "ploc_max": 7.0,
                      "lloc_min": 5.0,
                      "lloc_max": 5.0,
                      "blank_min": 0.0,
                      "blank_max": 0.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn java_class_loc() {
        check_metrics::<JavaParser>(
            "
            public class Person {
              private String name;
              public Person(String name){
                this.name = name; // +1
              }
              public String getName() {
                return name; // +1
              }
            }",
            "foo.java",
            |metric| {
                // Spaces: 4
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r###"
                    {
                      "sloc": 9.0,
                      "ploc": 9.0,
                      "lloc": 2.0,
                      "cloc": 2.0,
                      "blank": 0.0,
                      "sloc_average": 2.25,
                      "ploc_average": 2.25,
                      "lloc_average": 0.5,
                      "cloc_average": 0.5,
                      "blank_average": 0.0,
                      "sloc_min": 9.0,
                      "sloc_max": 9.0,
                      "cloc_min": 2.0,
                      "cloc_max": 2.0,
                      "ploc_min": 9.0,
                      "ploc_max": 9.0,
                      "lloc_min": 2.0,
                      "lloc_max": 2.0,
                      "blank_min": 0.0,
                      "blank_max": 0.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn java_expressions_lloc() {
        check_metrics::<JavaParser>(
            "int x = 10;                                                            // +1 local var declaration
            x=+89;                                                                  // +1 expression statement
            int y = x * 2;                                                          // +1 local var declaration
            IntFunction double = (n) -> n*2;                                        // +1 local var declaration
            int y2 = double(x);                                                     // +1 local var declaration
            System.out.println(\"double \" + x + \" = \" + y2);                     // +1 expression statement
            String message = (x % 2) == 0 ? \"Evenly done.\" : \"Oddly done.\";     // +1 local var declaration
            Object done = (Runnable) () -> { System.out.println(\"Done!\"); };      // +2 local var declaration + expression statement
            String s = \"string\";                                                  // +1 local var declaration
            boolean isS = (s instanceof String);                                    // +1 local var declaration
            done.run();                                                             // +1 expression statement
            ",
            "foo.java",
            |metric| {
                // Spaces: 1
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r###"
                    {
                      "sloc": 11.0,
                      "ploc": 11.0,
                      "lloc": 12.0,
                      "cloc": 11.0,
                      "blank": 0.0,
                      "sloc_average": 11.0,
                      "ploc_average": 11.0,
                      "lloc_average": 12.0,
                      "cloc_average": 11.0,
                      "blank_average": 0.0,
                      "sloc_min": 11.0,
                      "sloc_max": 11.0,
                      "cloc_min": 11.0,
                      "cloc_max": 11.0,
                      "ploc_min": 11.0,
                      "ploc_max": 11.0,
                      "lloc_min": 12.0,
                      "lloc_max": 12.0,
                      "blank_min": 0.0,
                      "blank_max": 0.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn java_statement_inline_loc() {
        check_metrics::<JavaParser>(
            "for (int i = 0; i < 100; i++) { System.out.println(\"hello\"); }",
            "foo.java",
            |metric| {
                // Spaces: 1
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r###"
                    {
                      "sloc": 1.0,
                      "ploc": 1.0,
                      "lloc": 2.0,
                      "cloc": 0.0,
                      "blank": 0.0,
                      "sloc_average": 1.0,
                      "ploc_average": 1.0,
                      "lloc_average": 2.0,
                      "cloc_average": 0.0,
                      "blank_average": 0.0,
                      "sloc_min": 1.0,
                      "sloc_max": 1.0,
                      "cloc_min": 0.0,
                      "cloc_max": 0.0,
                      "ploc_min": 1.0,
                      "ploc_max": 1.0,
                      "lloc_min": 2.0,
                      "lloc_max": 2.0,
                      "blank_min": 0.0,
                      "blank_max": 0.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn java_general_loc() {
        check_metrics::<JavaParser>(
            "int max = 100;

            /*
              Loop through and print
                from: 0
                to: max
            */
            for (int i = 0; i < max; i++) {
               // Print the value
               System.out.println(i);
             }",
            "foo.java",
            |metric| {
                // Spaces: 1
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r###"
                    {
                      "sloc": 11.0,
                      "ploc": 4.0,
                      "lloc": 3.0,
                      "cloc": 6.0,
                      "blank": 1.0,
                      "sloc_average": 11.0,
                      "ploc_average": 4.0,
                      "lloc_average": 3.0,
                      "cloc_average": 6.0,
                      "blank_average": 1.0,
                      "sloc_min": 11.0,
                      "sloc_max": 11.0,
                      "cloc_min": 6.0,
                      "cloc_max": 6.0,
                      "ploc_min": 4.0,
                      "ploc_max": 4.0,
                      "lloc_min": 3.0,
                      "lloc_max": 3.0,
                      "blank_min": 1.0,
                      "blank_max": 1.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn java_main_class_loc() {
        check_metrics::<JavaParser>(
            "package com.company;
             /**
             * The HelloWorldApp class implements an application that
             * simply prints \"Hello World!\" to standard output.
             */

            class HelloWorldApp {
              public void main(String[] args) {
                String message = args.length == 0 ? \"Hello empty world\" : \"Hello world\"; // +1 lloc : 1 var assignment
                System.out.println(message); // Display the string. +1 lloc
              }
            }",
            "foo.java",
            |metric| {
                // Spaces: 3
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r###"
                    {
                      "sloc": 12.0,
                      "ploc": 7.0,
                      "lloc": 2.0,
                      "cloc": 6.0,
                      "blank": 1.0,
                      "sloc_average": 4.0,
                      "ploc_average": 2.3333333333333335,
                      "lloc_average": 0.6666666666666666,
                      "cloc_average": 2.0,
                      "blank_average": 0.3333333333333333,
                      "sloc_min": 6.0,
                      "sloc_max": 6.0,
                      "cloc_min": 2.0,
                      "cloc_max": 2.0,
                      "ploc_min": 6.0,
                      "ploc_max": 6.0,
                      "lloc_min": 2.0,
                      "lloc_max": 2.0,
                      "blank_min": 0.0,
                      "blank_max": 0.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn go_general_loc() {
        check_metrics::<GoParser>(
            "package main

            // entrypoint
            func main() {
                /* loop body */
                for i := 0; i < 10; i++ {
                    fmt.Println(i)
                }
            }",
            "foo.go",
            |metric| {
                // Spaces: 2 (unit + main).
                // lloc: for_statement (+1), fmt.Println expression (+1).
                //       `i := 0` and `i++` inside the for-clause are gated.
                // cloc: 2 comments (line + block).
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r###"
                    {
                      "sloc": 9.0,
                      "ploc": 6.0,
                      "lloc": 2.0,
                      "cloc": 2.0,
                      "blank": 1.0,
                      "sloc_average": 4.5,
                      "ploc_average": 3.0,
                      "lloc_average": 1.0,
                      "cloc_average": 1.0,
                      "blank_average": 0.5,
                      "sloc_min": 6.0,
                      "sloc_max": 6.0,
                      "cloc_min": 1.0,
                      "cloc_max": 1.0,
                      "ploc_min": 5.0,
                      "ploc_max": 5.0,
                      "lloc_min": 2.0,
                      "lloc_max": 2.0,
                      "blank_min": 0.0,
                      "blank_max": 0.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn go_for_clause_does_not_double_count_lloc() {
        // Bare `for` body has only a return; the `for_statement` itself is the
        // single logical line. Confirms ShortVarDeclaration in a for-clause
        // does not add an extra lloc.
        check_metrics::<GoParser>(
            "package main
            func f(n int) int {
                for i := 0; i < n; i++ {
                    return i
                }
                return 0
            }",
            "foo.go",
            |metric| {
                // Expected lloc: for (+1), return (+1), return (+1) = 3.
                // Without the gate, ShortVarDeclaration would add an extra (+1).
                assert_eq!(metric.loc.lloc(), 3.0);
            },
        );
    }

    #[test]
    fn go_blank() {
        check_metrics::<GoParser>(
            "package main

            func foo() {
                x := 1

                y := 2
            }",
            "foo.go",
            |metric| {
                // Spaces: 2 (unit + foo).
                // blank: 2 (lines 2 and 5 are empty).
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r#"
                {
                  "sloc": 7.0,
                  "ploc": 5.0,
                  "lloc": 2.0,
                  "cloc": 0.0,
                  "blank": 2.0,
                  "sloc_average": 3.5,
                  "ploc_average": 2.5,
                  "lloc_average": 1.0,
                  "cloc_average": 0.0,
                  "blank_average": 1.0,
                  "sloc_min": 5.0,
                  "sloc_max": 5.0,
                  "cloc_min": 0.0,
                  "cloc_max": 0.0,
                  "ploc_min": 4.0,
                  "ploc_max": 4.0,
                  "lloc_min": 2.0,
                  "lloc_max": 2.0,
                  "blank_min": 1.0,
                  "blank_max": 1.0
                }
                "#
                );
            },
        );
    }

    #[test]
    fn go_cloc_line_comments() {
        check_metrics::<GoParser>(
            "package main

            // helper adds two numbers.
            // It returns their sum.
            func add(a, b int) int {
                // compute the result
                return a + b
            }",
            "foo.go",
            |metric| {
                // Spaces: 2 (unit + add).
                // cloc: 3 lines with `//` comments.
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r#"
                {
                  "sloc": 8.0,
                  "ploc": 4.0,
                  "lloc": 1.0,
                  "cloc": 3.0,
                  "blank": 1.0,
                  "sloc_average": 4.0,
                  "ploc_average": 2.0,
                  "lloc_average": 0.5,
                  "cloc_average": 1.5,
                  "blank_average": 0.5,
                  "sloc_min": 4.0,
                  "sloc_max": 4.0,
                  "cloc_min": 1.0,
                  "cloc_max": 1.0,
                  "ploc_min": 3.0,
                  "ploc_max": 3.0,
                  "lloc_min": 1.0,
                  "lloc_max": 1.0,
                  "blank_min": 0.0,
                  "blank_max": 0.0
                }
                "#
                );
            },
        );
    }

    #[test]
    fn go_cloc_block_comments() {
        check_metrics::<GoParser>(
            "package main

            /* block comment
               spanning two lines */
            func foo() {
                x := 1 /* inline block */
            }",
            "foo.go",
            |metric| {
                // Spaces: 2 (unit + foo).
                // cloc: 2-line block comment + inline block = 3 comment lines.
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r#"
                {
                  "sloc": 7.0,
                  "ploc": 4.0,
                  "lloc": 1.0,
                  "cloc": 3.0,
                  "blank": 1.0,
                  "sloc_average": 3.5,
                  "ploc_average": 2.0,
                  "lloc_average": 0.5,
                  "cloc_average": 1.5,
                  "blank_average": 0.5,
                  "sloc_min": 3.0,
                  "sloc_max": 3.0,
                  "cloc_min": 1.0,
                  "cloc_max": 1.0,
                  "ploc_min": 3.0,
                  "ploc_max": 3.0,
                  "lloc_min": 1.0,
                  "lloc_max": 1.0,
                  "blank_min": 0.0,
                  "blank_max": 0.0
                }
                "#
                );
            },
        );
    }

    #[test]
    fn go_lloc_if_for_switch() {
        check_metrics::<GoParser>(
            "package main

            func foo(n int) int {
                if n > 0 {
                    for i := 0; i < n; i++ {
                        switch i {
                        }
                    }
                }
                return n
            }",
            "foo.go",
            |metric| {
                // Spaces: 2 (unit + foo).
                // lloc: if (+1), for (+1), switch (+1), return (+1) = 4.
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r#"
                {
                  "sloc": 11.0,
                  "ploc": 10.0,
                  "lloc": 4.0,
                  "cloc": 0.0,
                  "blank": 1.0,
                  "sloc_average": 5.5,
                  "ploc_average": 5.0,
                  "lloc_average": 2.0,
                  "cloc_average": 0.0,
                  "blank_average": 0.5,
                  "sloc_min": 9.0,
                  "sloc_max": 9.0,
                  "cloc_min": 0.0,
                  "cloc_max": 0.0,
                  "ploc_min": 9.0,
                  "ploc_max": 9.0,
                  "lloc_min": 4.0,
                  "lloc_max": 4.0,
                  "blank_min": 0.0,
                  "blank_max": 0.0
                }
                "#
                );
            },
        );
    }

    #[test]
    fn go_lloc_go_defer() {
        check_metrics::<GoParser>(
            "package main

            func foo() {
                go run()
                defer cleanup()
            }",
            "foo.go",
            |metric| {
                // Spaces: 2 (unit + foo).
                // lloc: go (+1), defer (+1) = 2.
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r#"
                {
                  "sloc": 6.0,
                  "ploc": 5.0,
                  "lloc": 2.0,
                  "cloc": 0.0,
                  "blank": 1.0,
                  "sloc_average": 3.0,
                  "ploc_average": 2.5,
                  "lloc_average": 1.0,
                  "cloc_average": 0.0,
                  "blank_average": 0.5,
                  "sloc_min": 4.0,
                  "sloc_max": 4.0,
                  "cloc_min": 0.0,
                  "cloc_max": 0.0,
                  "ploc_min": 4.0,
                  "ploc_max": 4.0,
                  "lloc_min": 2.0,
                  "lloc_max": 2.0,
                  "blank_min": 0.0,
                  "blank_max": 0.0
                }
                "#
                );
            },
        );
    }

    #[test]
    fn go_lloc_var_const_declarations() {
        check_metrics::<GoParser>(
            "package main

            func foo() {
                var x int
                var y = 10
                const z = 42
                a := 3
                a = 4
            }",
            "foo.go",
            |metric| {
                // Spaces: 2 (unit + foo).
                // lloc: var (+1), var (+1), const (+1),
                //       short_var_decl (+1), assignment (+1) = 5.
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r#"
                {
                  "sloc": 9.0,
                  "ploc": 8.0,
                  "lloc": 5.0,
                  "cloc": 0.0,
                  "blank": 1.0,
                  "sloc_average": 4.5,
                  "ploc_average": 4.0,
                  "lloc_average": 2.5,
                  "cloc_average": 0.0,
                  "blank_average": 0.5,
                  "sloc_min": 7.0,
                  "sloc_max": 7.0,
                  "cloc_min": 0.0,
                  "cloc_max": 0.0,
                  "ploc_min": 7.0,
                  "ploc_max": 7.0,
                  "lloc_min": 5.0,
                  "lloc_max": 5.0,
                  "blank_min": 0.0,
                  "blank_max": 0.0
                }
                "#
                );
            },
        );
    }

    #[test]
    fn go_lloc_select() {
        check_metrics::<GoParser>(
            "package main

            func foo(ch chan int) {
                select {
                case v := <-ch:
                    _ = v
                }
            }",
            "foo.go",
            |metric| {
                // Spaces: 2 (unit + foo).
                // lloc: select (+1), assignment `_ = v` (+1) = 2.
                // `case v := <-ch:` is a receive_statement inside a
                // communication_case, not a ShortVarDeclaration.
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r#"
                {
                  "sloc": 8.0,
                  "ploc": 7.0,
                  "lloc": 2.0,
                  "cloc": 0.0,
                  "blank": 1.0,
                  "sloc_average": 4.0,
                  "ploc_average": 3.5,
                  "lloc_average": 1.0,
                  "cloc_average": 0.0,
                  "blank_average": 0.5,
                  "sloc_min": 6.0,
                  "sloc_max": 6.0,
                  "cloc_min": 0.0,
                  "cloc_max": 0.0,
                  "ploc_min": 6.0,
                  "ploc_max": 6.0,
                  "lloc_min": 2.0,
                  "lloc_max": 2.0,
                  "blank_min": 0.0,
                  "blank_max": 0.0
                }
                "#
                );
            },
        );
    }

    #[test]
    fn go_sloc_multiline_function() {
        check_metrics::<GoParser>(
            "package main

            func add(
                a int,
                b int,
            ) int {
                return a + b
            }",
            "foo.go",
            |metric| {
                // Spaces: 2 (unit + add).
                // The multi-line signature should count each line as sloc.
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r#"
                {
                  "sloc": 8.0,
                  "ploc": 7.0,
                  "lloc": 1.0,
                  "cloc": 0.0,
                  "blank": 1.0,
                  "sloc_average": 4.0,
                  "ploc_average": 3.5,
                  "lloc_average": 0.5,
                  "cloc_average": 0.0,
                  "blank_average": 0.5,
                  "sloc_min": 6.0,
                  "sloc_max": 6.0,
                  "cloc_min": 0.0,
                  "cloc_max": 0.0,
                  "ploc_min": 6.0,
                  "ploc_max": 6.0,
                  "lloc_min": 1.0,
                  "lloc_max": 1.0,
                  "blank_min": 0.0,
                  "blank_max": 0.0
                }
                "#
                );
            },
        );
    }

    #[test]
    fn go_code_comment_same_line() {
        check_metrics::<GoParser>(
            "package main

            func foo() {
                x := 1 // initialize x
                y := 2 // initialize y
            }",
            "foo.go",
            |metric| {
                // Spaces: 2 (unit + foo).
                // cloc: 2 (inline comments on code lines).
                // blank: 1 (line between package and func).
                // The code+comment lines should count for both ploc and cloc.
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r#"
                {
                  "sloc": 6.0,
                  "ploc": 5.0,
                  "lloc": 2.0,
                  "cloc": 2.0,
                  "blank": 1.0,
                  "sloc_average": 3.0,
                  "ploc_average": 2.5,
                  "lloc_average": 1.0,
                  "cloc_average": 1.0,
                  "blank_average": 0.5,
                  "sloc_min": 4.0,
                  "sloc_max": 4.0,
                  "cloc_min": 2.0,
                  "cloc_max": 2.0,
                  "ploc_min": 4.0,
                  "ploc_max": 4.0,
                  "lloc_min": 2.0,
                  "lloc_max": 2.0,
                  "blank_min": 0.0,
                  "blank_max": 0.0
                }
                "#
                );
            },
        );
    }

    #[test]
    fn perl_grammar_smoke() {
        // Pin the contract that tree-sitter-perl 1.1.2 cleanly parses every
        // Perl construct exercised by the rest of the `perl_*` test suite.
        // If a future grammar bump turns one of these into an error tree,
        // the metric assertions might still pass numerically by coincidence;
        // this test fails loudly instead.
        assert_perl_parses_cleanly(
            "use strict;
use warnings;

# line comment

=pod
multi-line POD
=cut

sub factorial {
    my ($n) = @_;
    return 1 if $n <= 1;
    return $n * factorial($n - 1);
}

my @arr = (1, 2, 3);
my %hash = (a => 1, b => 2);
my $closure = sub { return $_[0] + 1; };

for my $i (1..3) {
    if ($i % 2 == 0) {
        print \"even\\n\";
    } elsif ($i == 1) {
        print \"one\\n\";
    } else {
        print \"odd\\n\";
    }
}

while ($x > 0) {
    last if $x == 0;
    $x--;
}

unless ($done) {
    next;
}

my $heredoc = <<END;
hello
END
",
        );
    }

    #[test]
    fn perl_blank() {
        check_metrics::<PerlParser>(
            "

my $a = 42;

my $b = 43;

",
            "foo.pl",
            |metric| {
                insta::assert_json_snapshot!(metric.loc, @r#"
                {
                  "sloc": 3.0,
                  "ploc": 2.0,
                  "lloc": 2.0,
                  "cloc": 0.0,
                  "blank": 1.0,
                  "sloc_average": 3.0,
                  "ploc_average": 2.0,
                  "lloc_average": 2.0,
                  "cloc_average": 0.0,
                  "blank_average": 1.0,
                  "sloc_min": 3.0,
                  "sloc_max": 3.0,
                  "cloc_min": 0.0,
                  "cloc_max": 0.0,
                  "ploc_min": 2.0,
                  "ploc_max": 2.0,
                  "lloc_min": 2.0,
                  "lloc_max": 2.0,
                  "blank_min": 1.0,
                  "blank_max": 1.0
                }
                "#);
            },
        );
    }

    #[test]
    fn perl_no_zero_blank() {
        check_metrics::<PerlParser>(
            "my $a = 1;
my $b = 2;",
            "foo.pl",
            |metric| {
                insta::assert_json_snapshot!(metric.loc, @r#"
                {
                  "sloc": 2.0,
                  "ploc": 2.0,
                  "lloc": 2.0,
                  "cloc": 0.0,
                  "blank": 0.0,
                  "sloc_average": 2.0,
                  "ploc_average": 2.0,
                  "lloc_average": 2.0,
                  "cloc_average": 0.0,
                  "blank_average": 0.0,
                  "sloc_min": 2.0,
                  "sloc_max": 2.0,
                  "cloc_min": 0.0,
                  "cloc_max": 0.0,
                  "ploc_min": 2.0,
                  "ploc_max": 2.0,
                  "lloc_min": 2.0,
                  "lloc_max": 2.0,
                  "blank_min": 0.0,
                  "blank_max": 0.0
                }
                "#);
            },
        );
    }

    #[test]
    fn perl_cloc_line_comments() {
        check_metrics::<PerlParser>(
            "# top comment
my $a = 1; # trailing
my $b = 2;",
            "foo.pl",
            |metric| {
                insta::assert_json_snapshot!(metric.loc, @r#"
                {
                  "sloc": 3.0,
                  "ploc": 3.0,
                  "lloc": 2.0,
                  "cloc": 2.0,
                  "blank": 0.0,
                  "sloc_average": 3.0,
                  "ploc_average": 3.0,
                  "lloc_average": 2.0,
                  "cloc_average": 2.0,
                  "blank_average": 0.0,
                  "sloc_min": 3.0,
                  "sloc_max": 3.0,
                  "cloc_min": 2.0,
                  "cloc_max": 2.0,
                  "ploc_min": 3.0,
                  "ploc_max": 3.0,
                  "lloc_min": 2.0,
                  "lloc_max": 2.0,
                  "blank_min": 0.0,
                  "blank_max": 0.0
                }
                "#);
            },
        );
    }

    #[test]
    fn perl_cloc_pod_block() {
        check_metrics::<PerlParser>(
            "my $x = 1;
=pod
multi-line
pod block
=cut
my $y = 2;",
            "foo.pl",
            |metric| {
                insta::assert_json_snapshot!(metric.loc, @r#"
                {
                  "sloc": 6.0,
                  "ploc": 2.0,
                  "lloc": 2.0,
                  "cloc": 4.0,
                  "blank": 0.0,
                  "sloc_average": 6.0,
                  "ploc_average": 2.0,
                  "lloc_average": 2.0,
                  "cloc_average": 4.0,
                  "blank_average": 0.0,
                  "sloc_min": 6.0,
                  "sloc_max": 6.0,
                  "cloc_min": 4.0,
                  "cloc_max": 4.0,
                  "ploc_min": 2.0,
                  "ploc_max": 2.0,
                  "lloc_min": 2.0,
                  "lloc_max": 2.0,
                  "blank_min": 0.0,
                  "blank_max": 0.0
                }
                "#);
            },
        );
    }

    #[test]
    fn perl_lloc_simple_statements() {
        check_metrics::<PerlParser>(
            "my $a = 1;
my $b = 2;
my $c = 3;",
            "foo.pl",
            |metric| {
                insta::assert_json_snapshot!(metric.loc, @r#"
                {
                  "sloc": 3.0,
                  "ploc": 3.0,
                  "lloc": 3.0,
                  "cloc": 0.0,
                  "blank": 0.0,
                  "sloc_average": 3.0,
                  "ploc_average": 3.0,
                  "lloc_average": 3.0,
                  "cloc_average": 0.0,
                  "blank_average": 0.0,
                  "sloc_min": 3.0,
                  "sloc_max": 3.0,
                  "cloc_min": 0.0,
                  "cloc_max": 0.0,
                  "ploc_min": 3.0,
                  "ploc_max": 3.0,
                  "lloc_min": 3.0,
                  "lloc_max": 3.0,
                  "blank_min": 0.0,
                  "blank_max": 0.0
                }
                "#);
            },
        );
    }

    #[test]
    fn perl_lloc_compound_statements() {
        check_metrics::<PerlParser>(
            "if ($x) {
    print 'a';
}
while ($n > 0) {
    $n--;
}",
            "foo.pl",
            |metric| {
                insta::assert_json_snapshot!(metric.loc, @r#"
                {
                  "sloc": 6.0,
                  "ploc": 6.0,
                  "lloc": 4.0,
                  "cloc": 0.0,
                  "blank": 0.0,
                  "sloc_average": 6.0,
                  "ploc_average": 6.0,
                  "lloc_average": 4.0,
                  "cloc_average": 0.0,
                  "blank_average": 0.0,
                  "sloc_min": 6.0,
                  "sloc_max": 6.0,
                  "cloc_min": 0.0,
                  "cloc_max": 0.0,
                  "ploc_min": 6.0,
                  "ploc_max": 6.0,
                  "lloc_min": 4.0,
                  "lloc_max": 4.0,
                  "blank_min": 0.0,
                  "blank_max": 0.0
                }
                "#);
            },
        );
    }

    #[test]
    fn perl_lloc_postfix_form_counts_once() {
        // `do_thing() if cond;` is one logical line — wrapped in
        // single_line_statement; the inner if_simple_statement does not
        // add a second LLOC.
        check_metrics::<PerlParser>(
            "sub f {
    return 1 if $_[0];
}",
            "foo.pl",
            |metric| {
                assert_eq!(metric.loc.lloc(), 1.0);
            },
        );
    }

    #[test]
    fn perl_lloc_use_statement() {
        check_metrics::<PerlParser>(
            "use strict;
use warnings;
my $x = 1;",
            "foo.pl",
            |metric| {
                insta::assert_json_snapshot!(metric.loc, @r#"
                {
                  "sloc": 3.0,
                  "ploc": 3.0,
                  "lloc": 3.0,
                  "cloc": 0.0,
                  "blank": 0.0,
                  "sloc_average": 3.0,
                  "ploc_average": 3.0,
                  "lloc_average": 3.0,
                  "cloc_average": 0.0,
                  "blank_average": 0.0,
                  "sloc_min": 3.0,
                  "sloc_max": 3.0,
                  "cloc_min": 0.0,
                  "cloc_max": 0.0,
                  "ploc_min": 3.0,
                  "ploc_max": 3.0,
                  "lloc_min": 3.0,
                  "lloc_max": 3.0,
                  "blank_min": 0.0,
                  "blank_max": 0.0
                }
                "#);
            },
        );
    }

    #[test]
    fn perl_lloc_for_loop() {
        check_metrics::<PerlParser>(
            "for my $i (1..3) {
    print $i;
}",
            "foo.pl",
            |metric| {
                // `for_statement_2` (+1) and `print …;` SEMI in block (+1) → 2
                assert_eq!(metric.loc.lloc(), 2.0);
            },
        );
    }

    #[test]
    fn perl_lloc_loop_control_statement() {
        check_metrics::<PerlParser>(
            "while (1) {
    last if $done;
}",
            "foo.pl",
            |metric| {
                // while_statement (+1) + loop_control_statement (+1) = 2
                assert_eq!(metric.loc.lloc(), 2.0);
            },
        );
    }

    #[test]
    fn perl_lloc_no_double_count_inside_single_line_statement() {
        // SEMI inside a single_line_statement (postfix form) is a child of
        // if_simple_statement, not Block — so it must not add a second LLOC.
        check_metrics::<PerlParser>(
            "sub f {
    print 'a' unless $_[0];
}",
            "foo.pl",
            |metric| {
                assert_eq!(metric.loc.lloc(), 1.0);
            },
        );
    }

    #[test]
    fn perl_lloc_function_definition_not_counted() {
        // `sub f { ... }` itself is a function space, not an LLOC; only its
        // body statements count.
        check_metrics::<PerlParser>(
            "sub f {
    my $x = 1;
}",
            "foo.pl",
            |metric| {
                assert_eq!(metric.loc.lloc(), 1.0);
            },
        );
    }

    #[test]
    fn perl_lloc_anonymous_function() {
        // `my $f = sub { return 1; };` — the assignment is one LLOC at the
        // top level (the SEMI after `};`); the `return 1;` inside the
        // anonymous function block is a second LLOC inside the closure.
        check_metrics::<PerlParser>("my $f = sub { return 1; };", "foo.pl", |metric| {
            assert_eq!(metric.loc.lloc(), 2.0);
        });
    }

    #[test]
    fn perl_lloc_string_content_excluded_from_ploc() {
        // The body of a multi-line double-quoted string is data, not code:
        // intermediate rows that contain only string contents should not be
        // added to PLOC. Row 0 holds `my $s = "line1`; row 2 holds `line3";`
        // (both have code); row 1 is purely string content.
        check_metrics::<PerlParser>(
            "my $s = \"line1
line2
line3\";",
            "foo.pl",
            |metric| {
                // PLOC = {row 0, row 2} = 2. Without the gate, row 1 would
                // also leak in as a leaf-row of the string body.
                assert_eq!(metric.loc.ploc(), 2.0);
            },
        );
    }

    #[test]
    fn perl_lloc_unless_until() {
        check_metrics::<PerlParser>(
            "unless ($x) {
    print 'a';
}
until ($n == 0) {
    $n--;
}",
            "foo.pl",
            |metric| {
                // unless_statement (+1) + print SEMI (+1) + until_statement (+1)
                // + $n-- SEMI (+1) = 4
                assert_eq!(metric.loc.lloc(), 4.0);
            },
        );
    }

    #[test]
    fn perl_lloc_heredoc_body_not_counted() {
        // Heredoc body content is data, not code: the body lines should not
        // contribute LLOC or PLOC.
        check_metrics::<PerlParser>(
            "my $s = <<END;
line1
line2
END
my $x = 1;",
            "foo.pl",
            |metric| {
                // Two top-level statements: the heredoc-using `my $s = …;`
                // and `my $x = 1;`.
                assert_eq!(metric.loc.lloc(), 2.0);
            },
        );
        // Independent confirmation that the snippet is a valid heredoc and
        // not silently parsed as an error tree (which could otherwise yield
        // the same `lloc == 2.0` and mask a grammar regression).
        assert_perl_parses_cleanly(
            "my $s = <<END;
line1
line2
END
my $x = 1;",
        );
    }

    #[test]
    fn perl_lloc_package_and_require() {
        check_metrics::<PerlParser>(
            "package Foo;
require 5.010;
my $x = 1;",
            "foo.pl",
            |metric| {
                insta::assert_json_snapshot!(metric.loc, @r#"
                {
                  "sloc": 3.0,
                  "ploc": 3.0,
                  "lloc": 3.0,
                  "cloc": 0.0,
                  "blank": 0.0,
                  "sloc_average": 3.0,
                  "ploc_average": 3.0,
                  "lloc_average": 3.0,
                  "cloc_average": 0.0,
                  "blank_average": 0.0,
                  "sloc_min": 3.0,
                  "sloc_max": 3.0,
                  "cloc_min": 0.0,
                  "cloc_max": 0.0,
                  "ploc_min": 3.0,
                  "ploc_max": 3.0,
                  "lloc_min": 3.0,
                  "lloc_max": 3.0,
                  "blank_min": 0.0,
                  "blank_max": 0.0
                }
                 "#);
            },
        );
    }

    #[test]
    fn lua_blank() {
        check_metrics::<LuaParser>(
            "local x = 1

local y = 2",
            "foo.lua",
            |metric| {
                assert_eq!(metric.loc.sloc(), 3.0);
                assert_eq!(metric.loc.ploc(), 2.0);
                assert_eq!(metric.loc.lloc(), 2.0);
                assert_eq!(metric.loc.cloc(), 0.0);
                assert_eq!(metric.loc.blank(), 1.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn lua_no_zero_blank() {
        check_metrics::<LuaParser>(
            "local x = 1
local y = 2",
            "foo.lua",
            |metric| {
                assert_eq!(metric.loc.sloc(), 2.0);
                assert_eq!(metric.loc.ploc(), 2.0);
                assert_eq!(metric.loc.lloc(), 2.0);
                assert_eq!(metric.loc.cloc(), 0.0);
                assert_eq!(metric.loc.blank(), 0.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn lua_cloc() {
        check_metrics::<LuaParser>(
            "-- single line comment
local x = 1
--[[
  block comment
  second line
]]",
            "foo.lua",
            |metric| {
                assert_eq!(metric.loc.sloc(), 6.0);
                assert_eq!(metric.loc.ploc(), 1.0);
                assert_eq!(metric.loc.lloc(), 1.0);
                assert_eq!(metric.loc.cloc(), 5.0);
                assert_eq!(metric.loc.blank(), 0.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn lua_lloc() {
        check_metrics::<LuaParser>(
            "local function f(x)
  if x > 0 then
    local y = x + 1
    return y
  end
  return 0
end",
            "foo.lua",
            |metric| {
                assert_eq!(metric.loc.sloc(), 7.0);
                assert_eq!(metric.loc.ploc(), 7.0);
                assert_eq!(metric.loc.lloc(), 5.0);
                assert_eq!(metric.loc.cloc(), 0.0);
                assert_eq!(metric.loc.blank(), 0.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn lua_no_string_lloc() {
        // Long strings spanning multiple lines must not inflate lloc.
        check_metrics::<LuaParser>(
            "local s = [[
  line one
  line two
]]",
            "foo.lua",
            |metric| {
                assert_eq!(metric.loc.sloc(), 4.0);
                assert_eq!(metric.loc.ploc(), 2.0);
                assert_eq!(metric.loc.lloc(), 1.0);
                assert_eq!(metric.loc.cloc(), 0.0);
                assert_eq!(metric.loc.blank(), 2.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn lua_no_functiondefinition_lloc() {
        // Anonymous function definition is an expression, not a statement.
        // The containing variable_declaration counts as lloc; FunctionDefinition must not.
        check_metrics::<LuaParser>(
            "local f = function(x)
  return x + 1
end",
            "foo.lua",
            |metric| {
                assert_eq!(metric.loc.sloc(), 3.0);
                assert_eq!(metric.loc.ploc(), 3.0);
                assert_eq!(metric.loc.lloc(), 2.0);
                assert_eq!(metric.loc.cloc(), 0.0);
                assert_eq!(metric.loc.blank(), 0.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn lua_no_elseif_lloc() {
        // elseif_statement must not add lloc; only if_statement does.
        check_metrics::<LuaParser>(
            "local function f(x)
  if x > 0 then
    return 1
  elseif x < 0 then
    return -1
  else
    return 0
  end
end",
            "foo.lua",
            |metric| {
                assert_eq!(metric.loc.sloc(), 9.0);
                assert_eq!(metric.loc.ploc(), 9.0);
                assert_eq!(metric.loc.lloc(), 5.0);
                assert_eq!(metric.loc.cloc(), 0.0);
                assert_eq!(metric.loc.blank(), 0.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn lua_no_else_lloc() {
        // else_statement must not add lloc.
        check_metrics::<LuaParser>(
            "local function f(x)
  if x > 0 then
    return 1
  else
    return 0
  end
end",
            "foo.lua",
            |metric| {
                assert_eq!(metric.loc.sloc(), 7.0);
                assert_eq!(metric.loc.ploc(), 7.0);
                assert_eq!(metric.loc.lloc(), 4.0);
                assert_eq!(metric.loc.cloc(), 0.0);
                assert_eq!(metric.loc.blank(), 0.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn lua_functiondeclaration_lloc() {
        // Named function declaration counts as one lloc.
        check_metrics::<LuaParser>(
            "function f()
  return 1
end",
            "foo.lua",
            |metric| {
                assert_eq!(metric.loc.sloc(), 3.0);
                assert_eq!(metric.loc.ploc(), 3.0);
                assert_eq!(metric.loc.lloc(), 2.0);
                assert_eq!(metric.loc.cloc(), 0.0);
                assert_eq!(metric.loc.blank(), 0.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn lua_local_function_lloc() {
        // local function declaration is also a function_declaration node → one lloc.
        check_metrics::<LuaParser>(
            "local function g()
  return 2
end",
            "foo.lua",
            |metric| {
                assert_eq!(metric.loc.sloc(), 3.0);
                assert_eq!(metric.loc.ploc(), 3.0);
                assert_eq!(metric.loc.lloc(), 2.0);
                assert_eq!(metric.loc.cloc(), 0.0);
                assert_eq!(metric.loc.blank(), 0.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn lua_for_numeric_lloc() {
        check_metrics::<LuaParser>(
            "for i = 1, 10 do
  print(i)
end",
            "foo.lua",
            |metric| {
                assert_eq!(metric.loc.sloc(), 3.0);
                assert_eq!(metric.loc.ploc(), 3.0);
                assert_eq!(metric.loc.lloc(), 1.0);
                assert_eq!(metric.loc.cloc(), 0.0);
                assert_eq!(metric.loc.blank(), 0.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn lua_for_generic_lloc() {
        check_metrics::<LuaParser>(
            "for k, v in pairs(t) do
  print(k, v)
end",
            "foo.lua",
            |metric| {
                assert_eq!(metric.loc.sloc(), 3.0);
                assert_eq!(metric.loc.ploc(), 3.0);
                assert_eq!(metric.loc.lloc(), 1.0);
                assert_eq!(metric.loc.cloc(), 0.0);
                assert_eq!(metric.loc.blank(), 0.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn lua_repeat_lloc() {
        check_metrics::<LuaParser>(
            "local i = 0
repeat
  i = i + 1
until i >= 10",
            "foo.lua",
            |metric| {
                assert_eq!(metric.loc.sloc(), 4.0);
                assert_eq!(metric.loc.ploc(), 4.0);
                assert_eq!(metric.loc.lloc(), 3.0);
                assert_eq!(metric.loc.cloc(), 0.0);
                assert_eq!(metric.loc.blank(), 0.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn lua_local_decl_lloc() {
        check_metrics::<LuaParser>(
            "local x = 1
local y, z = 2, 3",
            "foo.lua",
            |metric| {
                assert_eq!(metric.loc.sloc(), 2.0);
                assert_eq!(metric.loc.ploc(), 2.0);
                assert_eq!(metric.loc.lloc(), 2.0);
                assert_eq!(metric.loc.cloc(), 0.0);
                assert_eq!(metric.loc.blank(), 0.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn lua_function_call_lloc() {
        // Standalone function calls have no expression_statement wrapper in Lua.
        // They fall to the `_` branch → counted as ploc, not lloc.
        check_metrics::<LuaParser>(
            "print(\"hello\")
local x = 1",
            "foo.lua",
            |metric| {
                assert_eq!(metric.loc.sloc(), 2.0);
                assert_eq!(metric.loc.ploc(), 2.0);
                assert_eq!(metric.loc.lloc(), 1.0);
                assert_eq!(metric.loc.cloc(), 0.0);
                assert_eq!(metric.loc.blank(), 0.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn lua_toplevel_assignment_lloc() {
        // Bare `x = 1` at chunk level: parent is Chunk, not VariableDeclaration,
        // so the parent-guard correctly counts it as 1 lloc.
        check_metrics::<LuaParser>(
            "x = 1
y, z = 2, 3",
            "foo.lua",
            |metric| {
                assert_eq!(metric.loc.sloc(), 2.0);
                assert_eq!(metric.loc.ploc(), 2.0);
                assert_eq!(metric.loc.lloc(), 2.0);
                assert_eq!(metric.loc.cloc(), 0.0);
                assert_eq!(metric.loc.blank(), 0.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn tsx_basic_loc() {
        check_metrics::<TsxParser>(
            "// A simple utility function
            function add(a: number, b: number): number {
                /* multi-line
                   comment */
                return a + b;
            }

            const greet = (name: string) => {
                return `Hello, ${name}`;
            };",
            "foo.tsx",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r###"
                    {
                      "sloc": 10.0,
                      "ploc": 6.0,
                      "lloc": 4.0,
                      "cloc": 3.0,
                      "blank": 1.0,
                      "sloc_average": 3.3333333333333335,
                      "ploc_average": 2.0,
                      "lloc_average": 1.3333333333333333,
                      "cloc_average": 1.0,
                      "blank_average": 0.3333333333333333,
                      "sloc_min": 3.0,
                      "sloc_max": 5.0,
                      "cloc_min": 0.0,
                      "cloc_max": 2.0,
                      "ploc_min": 3.0,
                      "ploc_max": 3.0,
                      "lloc_min": 2.0,
                      "lloc_max": 2.0,
                      "blank_min": 0.0,
                      "blank_max": 0.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn typescript_basic_loc() {
        check_metrics::<TypescriptParser>(
            "// Line comment
            /* Block
               comment */
            function greet(name: string): string {
                return `Hello, ${name}`;
            }

            const add = (a: number, b: number): number => a + b;",
            "foo.ts",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r###"
                    {
                      "sloc": 8.0,
                      "ploc": 4.0,
                      "lloc": 2.0,
                      "cloc": 3.0,
                      "blank": 1.0,
                      "sloc_average": 2.6666666666666665,
                      "ploc_average": 1.3333333333333333,
                      "lloc_average": 0.6666666666666666,
                      "cloc_average": 1.0,
                      "blank_average": 0.3333333333333333,
                      "sloc_min": 1.0,
                      "sloc_max": 3.0,
                      "cloc_min": 0.0,
                      "cloc_max": 0.0,
                      "ploc_min": 1.0,
                      "ploc_max": 3.0,
                      "lloc_min": 0.0,
                      "lloc_max": 2.0,
                      "blank_min": 0.0,
                      "blank_max": 0.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn csharp_comments() {
        check_metrics::<CsharpParser>(
            "for (int i = 0; i < 100; i++) {
               // Print hello
               System.Console.WriteLine(\"hello\");
               /// XML doc comment
               System.Console.WriteLine(\"hello\");
             }",
            "foo.cs",
            |metric| {
                assert_eq!(metric.loc.sloc(), 6.0);
                assert_eq!(metric.loc.ploc(), 4.0);
                assert_eq!(metric.loc.lloc(), 3.0);
                assert_eq!(metric.loc.cloc(), 2.0);
                assert_eq!(metric.loc.blank(), 0.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn csharp_blank() {
        check_metrics::<CsharpParser>(
            "int x = 1;


            int y = 2;",
            "foo.cs",
            |metric| {
                assert_eq!(metric.loc.sloc(), 4.0);
                assert_eq!(metric.loc.ploc(), 2.0);
                assert_eq!(metric.loc.lloc(), 2.0);
                assert_eq!(metric.loc.cloc(), 0.0);
                assert_eq!(metric.loc.blank(), 2.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn csharp_sloc() {
        check_metrics::<CsharpParser>(
            "for (int i = 0; i < 100; i++) {
               System.Console.WriteLine(i);
             }",
            "foo.cs",
            |metric| {
                assert_eq!(metric.loc.sloc(), 3.0);
                assert_eq!(metric.loc.ploc(), 3.0);
                assert_eq!(metric.loc.lloc(), 2.0);
                assert_eq!(metric.loc.cloc(), 0.0);
                assert_eq!(metric.loc.blank(), 0.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn csharp_module_sloc() {
        check_metrics::<CsharpParser>(
            "namespace HelloWorld {
              class Program { }
            }",
            "foo.cs",
            |metric| {
                assert_eq!(metric.loc.sloc(), 3.0);
                assert_eq!(metric.loc.ploc(), 3.0);
                assert_eq!(metric.loc.lloc(), 0.0);
                assert_eq!(metric.loc.cloc(), 0.0);
                assert_eq!(metric.loc.blank(), 0.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn csharp_single_ploc() {
        check_metrics::<CsharpParser>("int x = 1;", "foo.cs", |metric| {
            assert_eq!(metric.loc.sloc(), 1.0);
            assert_eq!(metric.loc.ploc(), 1.0);
            assert_eq!(metric.loc.lloc(), 1.0);
            assert_eq!(metric.loc.cloc(), 0.0);
            assert_eq!(metric.loc.blank(), 0.0);
            insta::assert_json_snapshot!(metric.loc);
        });
    }

    #[test]
    fn csharp_simple_ploc() {
        check_metrics::<CsharpParser>(
            "for (int i = 0; i < 100; i++) {
               System.Console.WriteLine(i);
             }",
            "foo.cs",
            |metric| {
                assert_eq!(metric.loc.sloc(), 3.0);
                assert_eq!(metric.loc.ploc(), 3.0);
                assert_eq!(metric.loc.lloc(), 2.0);
                assert_eq!(metric.loc.cloc(), 0.0);
                assert_eq!(metric.loc.blank(), 0.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn csharp_multi_ploc() {
        check_metrics::<CsharpParser>(
            "int x = 1;
            for (int i = 0; i < 100; i++) {
               System.Console.WriteLine(i);
             }",
            "foo.cs",
            |metric| {
                assert_eq!(metric.loc.sloc(), 4.0);
                assert_eq!(metric.loc.ploc(), 4.0);
                assert_eq!(metric.loc.lloc(), 3.0);
                assert_eq!(metric.loc.cloc(), 0.0);
                assert_eq!(metric.loc.blank(), 0.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn csharp_single_statement_lloc() {
        check_metrics::<CsharpParser>("int max = 10;", "foo.cs", |metric| {
            assert_eq!(metric.loc.sloc(), 1.0);
            assert_eq!(metric.loc.ploc(), 1.0);
            assert_eq!(metric.loc.lloc(), 1.0);
            assert_eq!(metric.loc.cloc(), 0.0);
            assert_eq!(metric.loc.blank(), 0.0);
            insta::assert_json_snapshot!(metric.loc);
        });
    }

    #[test]
    fn csharp_for_lloc() {
        check_metrics::<CsharpParser>(
            "for (int i = 0; i < 10; i++) {
                System.Console.WriteLine(i);
            }",
            "foo.cs",
            |metric| {
                assert_eq!(metric.loc.sloc(), 3.0);
                assert_eq!(metric.loc.ploc(), 3.0);
                assert_eq!(metric.loc.lloc(), 2.0);
                assert_eq!(metric.loc.cloc(), 0.0);
                assert_eq!(metric.loc.blank(), 0.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn csharp_foreach_lloc() {
        check_metrics::<CsharpParser>(
            "foreach (var item in items) {
                System.Console.WriteLine(item);
            }",
            "foo.cs",
            |metric| {
                assert_eq!(metric.loc.sloc(), 3.0);
                assert_eq!(metric.loc.ploc(), 3.0);
                assert_eq!(metric.loc.lloc(), 2.0);
                assert_eq!(metric.loc.cloc(), 0.0);
                assert_eq!(metric.loc.blank(), 0.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn csharp_while_lloc() {
        check_metrics::<CsharpParser>(
            "int i = 0;
            while (i < 10) {
                i++;
            }",
            "foo.cs",
            |metric| {
                assert_eq!(metric.loc.sloc(), 4.0);
                assert_eq!(metric.loc.ploc(), 4.0);
                assert_eq!(metric.loc.lloc(), 3.0);
                assert_eq!(metric.loc.cloc(), 0.0);
                assert_eq!(metric.loc.blank(), 0.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn csharp_do_while_lloc() {
        check_metrics::<CsharpParser>(
            "int i = 0;
            do {
                i++;
            } while (i < 10);",
            "foo.cs",
            |metric| {
                assert_eq!(metric.loc.sloc(), 4.0);
                assert_eq!(metric.loc.ploc(), 4.0);
                assert_eq!(metric.loc.lloc(), 3.0);
                assert_eq!(metric.loc.cloc(), 0.0);
                assert_eq!(metric.loc.blank(), 0.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn csharp_switch_lloc() {
        check_metrics::<CsharpParser>(
            "switch (x) {
                case 1: System.Console.WriteLine(1); break;
                case 2: System.Console.WriteLine(2); break;
                default: System.Console.WriteLine(0); break;
            }
            string s = x switch { 1 => \"one\", _ => \"other\" };",
            "foo.cs",
            |metric| {
                assert_eq!(metric.loc.sloc(), 6.0);
                assert_eq!(metric.loc.ploc(), 6.0);
                assert_eq!(metric.loc.lloc(), 8.0);
                assert_eq!(metric.loc.cloc(), 0.0);
                assert_eq!(metric.loc.blank(), 0.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn csharp_continue_lloc() {
        check_metrics::<CsharpParser>(
            "for (int i = 0; i < 10; i++) {
                if (i == 5) continue;
                System.Console.WriteLine(i);
            }",
            "foo.cs",
            |metric| {
                assert_eq!(metric.loc.sloc(), 4.0);
                assert_eq!(metric.loc.ploc(), 4.0);
                assert_eq!(metric.loc.lloc(), 4.0);
                assert_eq!(metric.loc.cloc(), 0.0);
                assert_eq!(metric.loc.blank(), 0.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn csharp_try_lloc() {
        check_metrics::<CsharpParser>(
            "try {
                System.Console.WriteLine(\"try\");
            } catch (System.Exception e) {
                throw new System.Exception(\"caught\");
            } finally {
                System.Console.WriteLine(\"done\");
            }",
            "foo.cs",
            |metric| {
                assert_eq!(metric.loc.sloc(), 7.0);
                assert_eq!(metric.loc.ploc(), 7.0);
                assert_eq!(metric.loc.lloc(), 4.0);
                assert_eq!(metric.loc.cloc(), 0.0);
                assert_eq!(metric.loc.blank(), 0.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn csharp_class_loc() {
        check_metrics::<CsharpParser>(
            "class A {
                int x;
                public void M() {
                    System.Console.WriteLine(x);
                }
            }",
            "foo.cs",
            |metric| {
                assert_eq!(metric.loc.sloc(), 6.0);
                assert_eq!(metric.loc.ploc(), 6.0);
                assert_eq!(metric.loc.lloc(), 1.0);
                assert_eq!(metric.loc.cloc(), 0.0);
                assert_eq!(metric.loc.blank(), 0.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn csharp_expressions_lloc() {
        check_metrics::<CsharpParser>(
            "int a = 1;
            int b = 2;
            int c = a + b;
            System.Console.WriteLine(c);",
            "foo.cs",
            |metric| {
                assert_eq!(metric.loc.sloc(), 4.0);
                assert_eq!(metric.loc.ploc(), 4.0);
                assert_eq!(metric.loc.lloc(), 4.0);
                assert_eq!(metric.loc.cloc(), 0.0);
                assert_eq!(metric.loc.blank(), 0.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn csharp_statement_inline_loc() {
        check_metrics::<CsharpParser>(
            "if (x > 0) System.Console.WriteLine(x);",
            "foo.cs",
            |metric| {
                assert_eq!(metric.loc.sloc(), 1.0);
                assert_eq!(metric.loc.ploc(), 1.0);
                assert_eq!(metric.loc.lloc(), 2.0);
                assert_eq!(metric.loc.cloc(), 0.0);
                assert_eq!(metric.loc.blank(), 0.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn csharp_general_loc() {
        check_metrics::<CsharpParser>(
            "using System;
            namespace Demo {
                class A {
                    public void M() {
                        Console.WriteLine(\"hi\");
                    }
                }
                class B {
                    public int N() { return 0; }
                }
            }",
            "foo.cs",
            |metric| {
                assert_eq!(metric.loc.sloc(), 11.0);
                assert_eq!(metric.loc.ploc(), 11.0);
                assert_eq!(metric.loc.lloc(), 2.0);
                assert_eq!(metric.loc.cloc(), 0.0);
                assert_eq!(metric.loc.blank(), 0.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn csharp_using_lloc() {
        // EC11 — `using_directive` does not bump LLOC; `using_statement`
        // (block form) and the C# 8 simple-using local-declaration
        // (`using var x = ...;`) both do, the latter via the standard
        // `LocalDeclarationStatement` path.
        check_metrics::<CsharpParser>(
            "using System;
            using System.IO;
            class A {
                public void M() {
                    using (var s = File.OpenRead(\"x\")) {
                        Console.WriteLine(s);
                    }
                    using var t = File.OpenRead(\"y\");
                    Console.WriteLine(t);
                }
            }",
            "foo.cs",
            |metric| {
                assert_eq!(metric.loc.sloc(), 11.0);
                assert_eq!(metric.loc.ploc(), 11.0);
                assert_eq!(metric.loc.lloc(), 4.0);
                assert_eq!(metric.loc.cloc(), 0.0);
                assert_eq!(metric.loc.blank(), 0.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn kotlin_loc_basic() {
        check_metrics::<KotlinParser>(
            "// A simple function
            fun greet(name: String): String {
                val greeting = \"Hello, \" + name
                if (name.isEmpty()) {
                    return \"Hello, World!\"
                }
                return greeting
            }",
            "foo.kt",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r###"
                    {
                      "sloc": 8.0,
                      "ploc": 7.0,
                      "lloc": 4.0,
                      "cloc": 1.0,
                      "blank": 0.0,
                      "sloc_average": 4.0,
                      "ploc_average": 3.5,
                      "lloc_average": 2.0,
                      "cloc_average": 0.5,
                      "blank_average": 0.0,
                      "sloc_min": 7.0,
                      "sloc_max": 7.0,
                      "cloc_min": 0.0,
                      "cloc_max": 0.0,
                      "ploc_min": 7.0,
                      "ploc_max": 7.0,
                      "lloc_min": 4.0,
                      "lloc_max": 4.0,
                      "blank_min": 0.0,
                      "blank_max": 0.0
                    }
                    "###
                );
            },
        );
    }

    #[test]
    fn kotlin_loc_bare_expression() {
        check_metrics::<KotlinParser>(
            "fun main() {
                val x = 42
                println(x)
                listOf(1, 2, 3).forEach { println(it) }
            }",
            "foo.kt",
            |metric| {
                // lloc should count: val x = 42 (PropertyDeclaration, +1)
                // + println(x) (CallExpression, parent=Block, +1)
                // + listOf(1, 2, 3).forEach { ... } (CallExpression, parent=Block, +1) = 3
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r#"
                {
                  "sloc": 5.0,
                  "ploc": 5.0,
                  "lloc": 3.0,
                  "cloc": 0.0,
                  "blank": 0.0,
                  "sloc_average": 2.5,
                  "ploc_average": 2.5,
                  "lloc_average": 1.5,
                  "cloc_average": 0.0,
                  "blank_average": 0.0,
                  "sloc_min": 5.0,
                  "sloc_max": 5.0,
                  "cloc_min": 0.0,
                  "cloc_max": 0.0,
                  "ploc_min": 5.0,
                  "ploc_max": 5.0,
                  "lloc_min": 3.0,
                  "lloc_max": 3.0,
                  "blank_min": 0.0,
                  "blank_max": 0.0
                }
                "#
                );
            },
        );
    }

    #[test]
    fn bash_loc() {
        check_metrics::<BashParser>(
            "#!/bin/bash
# This is a comment
f() {
    echo 'hello'
}

# Another comment
f",
            "foo.sh",
            |metric| {
                assert_eq!(metric.loc.sloc(), 8.0);
                assert_eq!(metric.loc.ploc(), 4.0);
                assert_eq!(metric.loc.lloc(), 3.0);
                assert_eq!(metric.loc.cloc(), 3.0);
                assert_eq!(metric.loc.blank(), 1.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    // CRLF regression tests: metrics must be identical regardless of line ending style.
    // These also serve as canaries for tree-sitter row-counting behaviour with \r bytes.

    #[test]
    fn python_cloc_crlf_matches_lf() {
        check_metrics::<PythonParser>("# comment\nx = 1", "foo.py", |m| {
            assert_eq!(m.loc.cloc(), 1.0);
            assert_eq!(m.loc.ploc(), 1.0);
            assert_eq!(m.loc.sloc(), 2.0);
            assert_eq!(m.loc.blank(), 0.0);
        });
        check_metrics::<PythonParser>("# comment\r\nx = 1", "foo.py", |m| {
            assert_eq!(m.loc.cloc(), 1.0);
            assert_eq!(m.loc.ploc(), 1.0);
            assert_eq!(m.loc.sloc(), 2.0);
            assert_eq!(m.loc.blank(), 0.0);
        });
        // Lone-CR (old Mac line endings) is the true canary: without CR normalisation,
        // tree-sitter 0.26.8 only advances its row counter on \n, collapsing all content
        // onto row 0 and producing wrong sloc/cloc metrics.
        check_metrics::<PythonParser>("# comment\rx = 1", "foo.py", |m| {
            assert_eq!(m.loc.cloc(), 1.0);
            assert_eq!(m.loc.ploc(), 1.0);
            assert_eq!(m.loc.sloc(), 2.0);
            assert_eq!(m.loc.blank(), 0.0);
        });
    }

    #[test]
    fn python_blank_crlf_matches_lf() {
        check_metrics::<PythonParser>("# comment\n\nx = 1", "foo.py", |m| {
            assert_eq!(m.loc.blank(), 1.0);
        });
        check_metrics::<PythonParser>("# comment\r\n\r\nx = 1", "foo.py", |m| {
            assert_eq!(m.loc.blank(), 1.0);
        });
        // Lone-CR: without normalisation the blank \r line stays on row 0 and is not counted.
        check_metrics::<PythonParser>("# comment\r\rx = 1", "foo.py", |m| {
            assert_eq!(m.loc.blank(), 1.0);
        });
    }

    #[test]
    fn rust_cloc_crlf_matches_lf() {
        check_metrics::<RustParser>(
            "fn f() {\n    // comment\n    let x = 1;\n}",
            "foo.rs",
            |m| {
                assert_eq!(m.loc.cloc(), 1.0);
                assert_eq!(m.loc.sloc(), 4.0);
            },
        );
        check_metrics::<RustParser>(
            "fn f() {\r\n    // comment\r\n    let x = 1;\r\n}",
            "foo.rs",
            |m| {
                assert_eq!(m.loc.cloc(), 1.0);
                assert_eq!(m.loc.sloc(), 4.0);
            },
        );
        // Lone-CR: without normalisation, tree-sitter 0.26.8 only advances its row counter on
        // \n, so all content collapses onto row 0 and sloc becomes 1 instead of 4.
        check_metrics::<RustParser>(
            "fn f() {\r    // comment\r    let x = 1;\r}",
            "foo.rs",
            |m| {
                assert_eq!(m.loc.cloc(), 1.0);
                assert_eq!(m.loc.sloc(), 4.0);
            },
        );
    }

    #[test]
    fn tcl_blank() {
        check_metrics::<TclParser>("set x 1\n\nset y 2", "foo.tcl", |metric| {
            assert_eq!(metric.loc.sloc(), 3.0);
            assert_eq!(metric.loc.ploc(), 2.0);
            assert_eq!(metric.loc.lloc(), 2.0);
            assert_eq!(metric.loc.cloc(), 0.0);
            assert_eq!(metric.loc.blank(), 1.0);
            insta::assert_json_snapshot!(metric.loc);
        });
    }

    #[test]
    fn tcl_no_zero_blank() {
        check_metrics::<TclParser>("set x 1\nset y 2", "foo.tcl", |metric| {
            assert_eq!(metric.loc.blank(), 0.0);
        });
    }

    #[test]
    fn tcl_cloc() {
        check_metrics::<TclParser>("# This is a comment\nset x 1", "foo.tcl", |metric| {
            assert_eq!(metric.loc.sloc(), 2.0);
            assert_eq!(metric.loc.ploc(), 2.0);
            assert_eq!(metric.loc.lloc(), 1.0);
            assert_eq!(metric.loc.cloc(), 1.0);
            assert_eq!(metric.loc.blank(), 0.0);
            insta::assert_json_snapshot!(metric.loc);
        });
    }

    #[test]
    fn tcl_lloc() {
        check_metrics::<TclParser>(
            "proc f {x} {
    while {$x > 0} {
        if {$x > 10} {
            set x [expr {$x - 1}]
        }
    }
}",
            "foo.tcl",
            |metric| {
                assert_eq!(metric.loc.sloc(), 7.0);
                assert_eq!(metric.loc.ploc(), 7.0);
                assert_eq!(metric.loc.lloc(), 4.0);
                assert_eq!(metric.loc.cloc(), 0.0);
                assert_eq!(metric.loc.blank(), 0.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn tcl_no_command_substitution_lloc() {
        // `string toupper` inside [...] is a sub-expression; only `puts` is top-level.
        check_metrics::<TclParser>("puts [string toupper x]", "foo.tcl", |metric| {
            assert_eq!(metric.loc.sloc(), 1.0);
            assert_eq!(metric.loc.ploc(), 1.0);
            assert_eq!(metric.loc.lloc(), 1.0);
            assert_eq!(metric.loc.cloc(), 0.0);
            assert_eq!(metric.loc.blank(), 0.0);
            insta::assert_json_snapshot!(metric.loc);
        });
    }

    #[test]
    fn tcl_procedure_lloc() {
        check_metrics::<TclParser>("proc foo {} {\n    puts hello\n}", "foo.tcl", |metric| {
            assert_eq!(metric.loc.sloc(), 3.0);
            assert_eq!(metric.loc.ploc(), 3.0);
            assert_eq!(metric.loc.lloc(), 2.0);
            assert_eq!(metric.loc.cloc(), 0.0);
            assert_eq!(metric.loc.blank(), 0.0);
            insta::assert_json_snapshot!(metric.loc);
        });
    }

    #[test]
    fn tcl_if_lloc() {
        check_metrics::<TclParser>("if {1} {\n    puts hello\n}", "foo.tcl", |metric| {
            assert_eq!(metric.loc.sloc(), 3.0);
            assert_eq!(metric.loc.ploc(), 3.0);
            assert_eq!(metric.loc.lloc(), 2.0);
            assert_eq!(metric.loc.cloc(), 0.0);
            assert_eq!(metric.loc.blank(), 0.0);
            insta::assert_json_snapshot!(metric.loc);
        });
    }

    #[test]
    fn tcl_elseif_lloc() {
        // if=1 lloc, elseif=1 lloc, else adds 0 lloc
        check_metrics::<TclParser>(
            "if {$x > 10} {
    puts big
} elseif {$x > 5} {
    puts medium
} else {
    puts small
}",
            "foo.tcl",
            |metric| {
                assert_eq!(metric.loc.sloc(), 7.0);
                assert_eq!(metric.loc.ploc(), 7.0);
                assert_eq!(metric.loc.lloc(), 5.0);
                assert_eq!(metric.loc.cloc(), 0.0);
                assert_eq!(metric.loc.blank(), 0.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn tcl_while_lloc() {
        check_metrics::<TclParser>(
            "while {$x > 0} {\n    set x [expr {$x - 1}]\n}",
            "foo.tcl",
            |metric| {
                assert_eq!(metric.loc.sloc(), 3.0);
                assert_eq!(metric.loc.ploc(), 3.0);
                assert_eq!(metric.loc.lloc(), 2.0);
                assert_eq!(metric.loc.cloc(), 0.0);
                assert_eq!(metric.loc.blank(), 0.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn tcl_foreach_lloc() {
        check_metrics::<TclParser>(
            "foreach item {a b c} {\n    puts $item\n}",
            "foo.tcl",
            |metric| {
                assert_eq!(metric.loc.sloc(), 3.0);
                assert_eq!(metric.loc.ploc(), 3.0);
                assert_eq!(metric.loc.lloc(), 2.0);
                assert_eq!(metric.loc.cloc(), 0.0);
                assert_eq!(metric.loc.blank(), 0.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn tcl_set_lloc() {
        check_metrics::<TclParser>("set x 42", "foo.tcl", |metric| {
            assert_eq!(metric.loc.sloc(), 1.0);
            assert_eq!(metric.loc.ploc(), 1.0);
            assert_eq!(metric.loc.lloc(), 1.0);
            assert_eq!(metric.loc.cloc(), 0.0);
            assert_eq!(metric.loc.blank(), 0.0);
            insta::assert_json_snapshot!(metric.loc);
        });
    }

    #[test]
    fn tcl_global_lloc() {
        check_metrics::<TclParser>("global x", "foo.tcl", |metric| {
            assert_eq!(metric.loc.sloc(), 1.0);
            assert_eq!(metric.loc.ploc(), 1.0);
            assert_eq!(metric.loc.lloc(), 1.0);
            assert_eq!(metric.loc.cloc(), 0.0);
            assert_eq!(metric.loc.blank(), 0.0);
            insta::assert_json_snapshot!(metric.loc);
        });
    }

    #[test]
    fn tcl_try_catch_lloc() {
        // try=1 lloc; catch command=1 lloc; commands inside bodies count separately
        check_metrics::<TclParser>(
            "catch {
    set x 1
} result
try {
    set y 2
} on error {msg} {
    puts $msg
}",
            "foo.tcl",
            |metric| {
                assert_eq!(metric.loc.sloc(), 8.0);
                assert_eq!(metric.loc.ploc(), 8.0);
                assert_eq!(metric.loc.lloc(), 5.0);
                assert_eq!(metric.loc.cloc(), 0.0);
                assert_eq!(metric.loc.blank(), 0.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn tcl_namespace_lloc() {
        check_metrics::<TclParser>(
            "namespace eval myns {\n    set x 1\n}",
            "foo.tcl",
            |metric| {
                assert_eq!(metric.loc.sloc(), 3.0);
                assert_eq!(metric.loc.ploc(), 3.0);
                assert_eq!(metric.loc.lloc(), 2.0);
                assert_eq!(metric.loc.cloc(), 0.0);
                assert_eq!(metric.loc.blank(), 0.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn tcl_regexp_lloc() {
        check_metrics::<TclParser>("regexp {^[0-9]+$} $x", "foo.tcl", |metric| {
            assert_eq!(metric.loc.sloc(), 1.0);
            assert_eq!(metric.loc.ploc(), 1.0);
            assert_eq!(metric.loc.lloc(), 1.0);
            assert_eq!(metric.loc.cloc(), 0.0);
            assert_eq!(metric.loc.blank(), 0.0);
            insta::assert_json_snapshot!(metric.loc);
        });
    }

    #[test]
    fn tcl_expr_cmd_lloc() {
        check_metrics::<TclParser>("expr {1 + 2}", "foo.tcl", |metric| {
            assert_eq!(metric.loc.sloc(), 1.0);
            assert_eq!(metric.loc.ploc(), 1.0);
            assert_eq!(metric.loc.lloc(), 1.0);
            assert_eq!(metric.loc.cloc(), 0.0);
            assert_eq!(metric.loc.blank(), 0.0);
            insta::assert_json_snapshot!(metric.loc);
        });
    }

    #[test]
    fn tcl_no_expr_cmd_substitution_lloc() {
        // `expr` inside [...] is a sub-expression, not a statement; only `set` counts.
        check_metrics::<TclParser>("set x [expr {1 + 2}]", "foo.tcl", |metric| {
            assert_eq!(metric.loc.sloc(), 1.0);
            assert_eq!(metric.loc.ploc(), 1.0);
            assert_eq!(metric.loc.lloc(), 1.0);
            assert_eq!(metric.loc.cloc(), 0.0);
            assert_eq!(metric.loc.blank(), 0.0);
            insta::assert_json_snapshot!(metric.loc);
        });
    }

    #[test]
    fn tcl_nested_commands_lloc() {
        // Commands inside proc body are recursively parsed; verify each counts.
        check_metrics::<TclParser>(
            "proc f {x} {
    set y [expr {$x * 2}]
    puts $y
}",
            "foo.tcl",
            |metric| {
                assert_eq!(metric.loc.sloc(), 4.0);
                assert_eq!(metric.loc.ploc(), 4.0);
                assert_eq!(metric.loc.lloc(), 3.0);
                assert_eq!(metric.loc.cloc(), 0.0);
                assert_eq!(metric.loc.blank(), 0.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn tcl_command_lloc() {
        check_metrics::<TclParser>("puts hello", "foo.tcl", |metric| {
            assert_eq!(metric.loc.sloc(), 1.0);
            assert_eq!(metric.loc.ploc(), 1.0);
            assert_eq!(metric.loc.lloc(), 1.0);
            assert_eq!(metric.loc.cloc(), 0.0);
            assert_eq!(metric.loc.blank(), 0.0);
            insta::assert_json_snapshot!(metric.loc);
        });
    }

    #[test]
    fn tcl_no_else_lloc() {
        // `else` block does not add a logical line.
        check_metrics::<TclParser>(
            "if {1} {\n    puts yes\n} else {\n    puts no\n}",
            "foo.tcl",
            |metric| {
                assert_eq!(metric.loc.sloc(), 5.0);
                assert_eq!(metric.loc.ploc(), 5.0);
                assert_eq!(metric.loc.lloc(), 3.0);
                assert_eq!(metric.loc.cloc(), 0.0);
                assert_eq!(metric.loc.blank(), 0.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn tcl_no_finally_lloc() {
        // `finally` block, like `else`, does not add a logical line.
        // proc(1) + try(1) + puts_hi(1) + puts_done(1) + finally(0) = 4.
        check_metrics::<TclParser>(
            "proc f {} {\n    try {\n        puts hi\n    } finally {\n        puts done\n    }\n}",
            "foo.tcl",
            |metric| {
                assert_eq!(
                    metric.loc.lloc(),
                    4.0,
                    "finally adds 0 lloc; would be 5 if finally counted"
                );
            },
        );
    }

    #[test]
    fn tcl_multiline_block() {
        check_metrics::<TclParser>(
            "proc f {x} {
    set a 1

    set b 2
    return [expr {$a + $b}]
}",
            "foo.tcl",
            |metric| {
                assert_eq!(metric.loc.sloc(), 6.0);
                assert_eq!(metric.loc.ploc(), 5.0);
                assert_eq!(metric.loc.lloc(), 4.0);
                assert_eq!(metric.loc.cloc(), 0.0);
                assert_eq!(metric.loc.blank(), 1.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn javascript_blank() {
        check_metrics::<JavascriptParser>(
            "// header comment
        function f() {

            var x = 1;

            var y = 2;
        }",
            "foo.js",
            |metric| {
                assert_eq!(metric.loc.sloc(), 7.0);
                assert_eq!(metric.loc.ploc(), 4.0);
                assert_eq!(metric.loc.lloc(), 1.0);
                assert_eq!(metric.loc.cloc(), 1.0);
                assert_eq!(metric.loc.blank(), 2.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn javascript_cloc() {
        check_metrics::<JavascriptParser>(
            "// line comment
        /* block
           comment */
        function f() {
            return 1; // inline
        }",
            "foo.js",
            |metric| {
                assert_eq!(metric.loc.sloc(), 6.0);
                assert_eq!(metric.loc.ploc(), 3.0);
                assert_eq!(metric.loc.lloc(), 2.0);
                assert_eq!(metric.loc.cloc(), 4.0);
                assert_eq!(metric.loc.blank(), 0.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn mozjs_blank() {
        check_metrics::<MozjsParser>(
            "function f() {

            var x = 1;

        }",
            "foo.js",
            |metric| {
                assert_eq!(metric.loc.sloc(), 5.0);
                assert_eq!(metric.loc.ploc(), 3.0);
                assert_eq!(metric.loc.lloc(), 1.0);
                assert_eq!(metric.loc.cloc(), 0.0);
                assert_eq!(metric.loc.blank(), 2.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn mozjs_cloc() {
        check_metrics::<MozjsParser>(
            "// header
        /* block comment */
        function f() {
            return 42;
        }",
            "foo.js",
            |metric| {
                assert_eq!(metric.loc.sloc(), 5.0);
                assert_eq!(metric.loc.ploc(), 3.0);
                assert_eq!(metric.loc.lloc(), 2.0);
                assert_eq!(metric.loc.cloc(), 2.0);
                assert_eq!(metric.loc.blank(), 0.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn mozjs_no_zero_blank() {
        check_metrics::<MozjsParser>(
            "function f() {
            var x = 1; // comment
            var y = 2; // comment
        }",
            "foo.js",
            |metric| {
                assert_eq!(metric.loc.sloc(), 4.0);
                assert_eq!(metric.loc.ploc(), 4.0);
                assert_eq!(metric.loc.lloc(), 1.0);
                assert_eq!(metric.loc.cloc(), 2.0);
                assert_eq!(metric.loc.blank(), 0.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn mozjs_arrow_function_loc() {
        check_metrics::<MozjsParser>(
            "const add = (a, b) => a + b;
        const greet = name => {
            return 'Hello ' + name;
        };",
            "foo.js",
            |metric| {
                assert_eq!(metric.loc.sloc(), 4.0);
                assert_eq!(metric.loc.ploc(), 4.0);
                assert_eq!(metric.loc.lloc(), 2.0);
                assert_eq!(metric.loc.cloc(), 0.0);
                assert_eq!(metric.loc.blank(), 0.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn mozjs_multiple_functions_loc() {
        check_metrics::<MozjsParser>(
            "function f() {
            return 1;
        }
        function g() {
            return 2;
        }",
            "foo.js",
            |metric| {
                assert_eq!(metric.loc.sloc(), 6.0);
                assert_eq!(metric.loc.ploc(), 6.0);
                assert_eq!(metric.loc.lloc(), 4.0);
                assert_eq!(metric.loc.cloc(), 0.0);
                assert_eq!(metric.loc.blank(), 0.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn mozjs_nested_function_loc() {
        check_metrics::<MozjsParser>(
            "function outer() {
            function inner() {
                return 1;
            }
            return inner();
        }",
            "foo.js",
            |metric| {
                assert_eq!(metric.loc.sloc(), 6.0);
                assert_eq!(metric.loc.ploc(), 6.0);
                assert_eq!(metric.loc.lloc(), 4.0);
                assert_eq!(metric.loc.cloc(), 0.0);
                assert_eq!(metric.loc.blank(), 0.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn mozjs_if_lloc() {
        check_metrics::<MozjsParser>(
            "function f(x) {
            if (x > 0) {
                return 1;
            } else {
                return -1;
            }
        }",
            "foo.js",
            |metric| {
                assert_eq!(metric.loc.sloc(), 7.0);
                assert_eq!(metric.loc.ploc(), 7.0);
                assert_eq!(metric.loc.lloc(), 6.0);
                assert_eq!(metric.loc.cloc(), 0.0);
                assert_eq!(metric.loc.blank(), 0.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn mozjs_for_lloc() {
        check_metrics::<MozjsParser>(
            "function f(n) {
            var s = 0;
            for (var i = 0; i < n; i++) {
                s += i;
            }
            return s;
        }",
            "foo.js",
            |metric| {
                assert_eq!(metric.loc.sloc(), 7.0);
                assert_eq!(metric.loc.ploc(), 7.0);
                assert_eq!(metric.loc.lloc(), 5.0);
                assert_eq!(metric.loc.cloc(), 0.0);
                assert_eq!(metric.loc.blank(), 0.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn bash_blank() {
        check_metrics::<BashParser>(
            "#!/bin/bash

        f() {

            echo hello

        }",
            "foo.sh",
            |metric| {
                assert_eq!(metric.loc.sloc(), 7.0);
                assert_eq!(metric.loc.ploc(), 3.0);
                assert_eq!(metric.loc.lloc(), 2.0);
                assert_eq!(metric.loc.cloc(), 1.0);
                assert_eq!(metric.loc.blank(), 3.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn bash_cloc() {
        check_metrics::<BashParser>(
            "# header comment
        f() {
            # body comment
            echo hello
        }",
            "foo.sh",
            |metric| {
                assert_eq!(metric.loc.sloc(), 5.0);
                assert_eq!(metric.loc.ploc(), 3.0);
                assert_eq!(metric.loc.lloc(), 2.0);
                assert_eq!(metric.loc.cloc(), 2.0);
                assert_eq!(metric.loc.blank(), 0.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn bash_no_zero_blank() {
        check_metrics::<BashParser>(
            "f() {
            echo hello # inline comment
            echo world # another comment
        }",
            "foo.sh",
            |metric| {
                assert_eq!(metric.loc.sloc(), 4.0);
                assert_eq!(metric.loc.ploc(), 4.0);
                assert_eq!(metric.loc.lloc(), 3.0);
                assert_eq!(metric.loc.cloc(), 2.0);
                assert_eq!(metric.loc.blank(), 0.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn bash_if_lloc() {
        check_metrics::<BashParser>(
            "f() {
            if [ $1 -gt 0 ]; then
                echo positive
            else
                echo negative
            fi
        }",
            "foo.sh",
            |metric| {
                assert_eq!(metric.loc.sloc(), 7.0);
                assert_eq!(metric.loc.ploc(), 7.0);
                assert_eq!(metric.loc.lloc(), 4.0);
                assert_eq!(metric.loc.cloc(), 0.0);
                assert_eq!(metric.loc.blank(), 0.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn bash_for_lloc() {
        check_metrics::<BashParser>(
            "f() {
            for i in 1 2 3; do
                echo $i
            done
        }",
            "foo.sh",
            |metric| {
                assert_eq!(metric.loc.sloc(), 5.0);
                assert_eq!(metric.loc.ploc(), 5.0);
                assert_eq!(metric.loc.lloc(), 3.0);
                assert_eq!(metric.loc.cloc(), 0.0);
                assert_eq!(metric.loc.blank(), 0.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn bash_while_lloc() {
        check_metrics::<BashParser>(
            "f() {
            local n=5
            while [ $n -gt 0 ]; do
                echo $n
                n=$((n - 1))
            done
        }",
            "foo.sh",
            |metric| {
                assert_eq!(metric.loc.sloc(), 7.0);
                assert_eq!(metric.loc.ploc(), 7.0);
                assert_eq!(metric.loc.lloc(), 4.0);
                assert_eq!(metric.loc.cloc(), 0.0);
                assert_eq!(metric.loc.blank(), 0.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn bash_case_lloc() {
        check_metrics::<BashParser>(
            "f() {
            case $1 in
                start) echo starting ;;
                stop)  echo stopping ;;
                *)     echo unknown  ;;
            esac
        }",
            "foo.sh",
            |metric| {
                assert_eq!(metric.loc.sloc(), 7.0);
                assert_eq!(metric.loc.ploc(), 7.0);
                assert_eq!(metric.loc.lloc(), 5.0);
                assert_eq!(metric.loc.cloc(), 0.0);
                assert_eq!(metric.loc.blank(), 0.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn bash_multiple_functions_loc() {
        check_metrics::<BashParser>(
            "f() {
            echo hello
        }
        g() {
            echo world
        }",
            "foo.sh",
            |metric| {
                assert_eq!(metric.loc.sloc(), 6.0);
                assert_eq!(metric.loc.ploc(), 6.0);
                assert_eq!(metric.loc.lloc(), 4.0);
                assert_eq!(metric.loc.cloc(), 0.0);
                assert_eq!(metric.loc.blank(), 0.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn bash_nested_function_loc() {
        check_metrics::<BashParser>(
            "outer() {
            inner() {
                echo inner
            }
            inner
            echo outer
        }",
            "foo.sh",
            |metric| {
                assert_eq!(metric.loc.sloc(), 7.0);
                assert_eq!(metric.loc.ploc(), 7.0);
                assert_eq!(metric.loc.lloc(), 5.0);
                assert_eq!(metric.loc.cloc(), 0.0);
                assert_eq!(metric.loc.blank(), 0.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn bash_heredoc_loc() {
        check_metrics::<BashParser>(
            "f() {
            cat <<EOF
line1
line2
EOF
        }",
            "foo.sh",
            |metric| {
                assert_eq!(metric.loc.sloc(), 6.0);
                assert_eq!(metric.loc.ploc(), 5.0);
                assert_eq!(metric.loc.lloc(), 2.0);
                assert_eq!(metric.loc.cloc(), 0.0);
                assert_eq!(metric.loc.blank(), 1.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn kotlin_loc_blank() {
        check_metrics::<KotlinParser>(
            "fun f(): Int {

            val x = 1

            return x
        }",
            "foo.kt",
            |metric| {
                assert_eq!(metric.loc.sloc(), 6.0);
                assert_eq!(metric.loc.ploc(), 4.0);
                assert_eq!(metric.loc.lloc(), 2.0);
                assert_eq!(metric.loc.cloc(), 0.0);
                assert_eq!(metric.loc.blank(), 2.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn kotlin_loc_cloc() {
        check_metrics::<KotlinParser>(
            "// header comment
        /* block
           comment */
        fun f(): Int {
            return 42 // inline
        }",
            "foo.kt",
            |metric| {
                assert_eq!(metric.loc.sloc(), 6.0);
                assert_eq!(metric.loc.ploc(), 3.0);
                assert_eq!(metric.loc.lloc(), 1.0);
                assert_eq!(metric.loc.cloc(), 4.0);
                assert_eq!(metric.loc.blank(), 0.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn kotlin_loc_no_zero_blank() {
        check_metrics::<KotlinParser>(
            "fun f(): Int {
            val x = 1 // x
            val y = 2 // y
            return x + y
        }",
            "foo.kt",
            |metric| {
                assert_eq!(metric.loc.sloc(), 5.0);
                assert_eq!(metric.loc.ploc(), 5.0);
                assert_eq!(metric.loc.lloc(), 3.0);
                assert_eq!(metric.loc.cloc(), 2.0);
                assert_eq!(metric.loc.blank(), 0.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn kotlin_loc_if_lloc() {
        check_metrics::<KotlinParser>(
            "fun classify(n: Int): String {
            if (n > 0) {
                return \"positive\"
            } else if (n < 0) {
                return \"negative\"
            }
            return \"zero\"
        }",
            "foo.kt",
            |metric| {
                assert_eq!(metric.loc.sloc(), 8.0);
                assert_eq!(metric.loc.ploc(), 8.0);
                assert_eq!(metric.loc.lloc(), 5.0);
                assert_eq!(metric.loc.cloc(), 0.0);
                assert_eq!(metric.loc.blank(), 0.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn kotlin_loc_for_lloc() {
        check_metrics::<KotlinParser>(
            "fun sum(n: Int): Int {
            var s = 0
            for (i in 1..n) {
                s += i
            }
            return s
        }",
            "foo.kt",
            |metric| {
                assert_eq!(metric.loc.sloc(), 7.0);
                assert_eq!(metric.loc.ploc(), 7.0);
                assert_eq!(metric.loc.lloc(), 4.0);
                assert_eq!(metric.loc.cloc(), 0.0);
                assert_eq!(metric.loc.blank(), 0.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn kotlin_loc_when_lloc() {
        check_metrics::<KotlinParser>(
            "fun describe(x: Int): String {
            return when (x) {
                1 -> \"one\"
                2 -> \"two\"
                else -> \"other\"
            }
        }",
            "foo.kt",
            |metric| {
                assert_eq!(metric.loc.sloc(), 7.0);
                assert_eq!(metric.loc.ploc(), 7.0);
                assert_eq!(metric.loc.lloc(), 2.0);
                assert_eq!(metric.loc.cloc(), 0.0);
                assert_eq!(metric.loc.blank(), 0.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn kotlin_loc_lambda_lloc() {
        check_metrics::<KotlinParser>(
            "fun f(list: List<Int>): List<Int> {
            return list.filter { it > 0 }
                       .map { it * 2 }
        }",
            "foo.kt",
            |metric| {
                assert_eq!(metric.loc.sloc(), 4.0);
                assert_eq!(metric.loc.ploc(), 4.0);
                assert_eq!(metric.loc.lloc(), 1.0);
                assert_eq!(metric.loc.cloc(), 0.0);
                assert_eq!(metric.loc.blank(), 0.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn kotlin_loc_class_loc() {
        check_metrics::<KotlinParser>(
            "class Counter {
            private var count = 0
            fun increment() { count++ }
            fun get(): Int = count
        }",
            "foo.kt",
            |metric| {
                assert_eq!(metric.loc.sloc(), 5.0);
                assert_eq!(metric.loc.ploc(), 5.0);
                assert_eq!(metric.loc.lloc(), 1.0);
                assert_eq!(metric.loc.cloc(), 0.0);
                assert_eq!(metric.loc.blank(), 0.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn kotlin_loc_multiple_functions_loc() {
        check_metrics::<KotlinParser>(
            "fun f(): Int {
            return 1
        }
        fun g(): Int {
            return 2
        }",
            "foo.kt",
            |metric| {
                assert_eq!(metric.loc.sloc(), 6.0);
                assert_eq!(metric.loc.ploc(), 6.0);
                assert_eq!(metric.loc.lloc(), 2.0);
                assert_eq!(metric.loc.cloc(), 0.0);
                assert_eq!(metric.loc.blank(), 0.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn kotlin_loc_while_lloc() {
        check_metrics::<KotlinParser>(
            "fun countdown(n: Int) {
            var i = n
            while (i > 0) {
                println(i)
                i--
            }
        }",
            "foo.kt",
            |metric| {
                assert_eq!(metric.loc.sloc(), 7.0);
                assert_eq!(metric.loc.ploc(), 7.0);
                assert_eq!(metric.loc.lloc(), 3.0);
                assert_eq!(metric.loc.cloc(), 0.0);
                assert_eq!(metric.loc.blank(), 0.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn typescript_blank() {
        check_metrics::<TypescriptParser>(
            "function f(): void {

            const x = 1;

        }",
            "foo.ts",
            |metric| {
                assert_eq!(metric.loc.sloc(), 5.0);
                assert_eq!(metric.loc.ploc(), 3.0);
                assert_eq!(metric.loc.lloc(), 1.0);
                assert_eq!(metric.loc.cloc(), 0.0);
                assert_eq!(metric.loc.blank(), 2.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn typescript_cloc() {
        check_metrics::<TypescriptParser>(
            "// header
        /* block
           comment */
        function f(): number {
            return 42; // inline
        }",
            "foo.ts",
            |metric| {
                assert_eq!(metric.loc.sloc(), 6.0);
                assert_eq!(metric.loc.ploc(), 3.0);
                assert_eq!(metric.loc.lloc(), 2.0);
                assert_eq!(metric.loc.cloc(), 4.0);
                assert_eq!(metric.loc.blank(), 0.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn typescript_no_zero_blank() {
        check_metrics::<TypescriptParser>(
            "function f(): void {
            const x = 1; // x
            const y = 2; // y
        }",
            "foo.ts",
            |metric| {
                assert_eq!(metric.loc.sloc(), 4.0);
                assert_eq!(metric.loc.ploc(), 4.0);
                assert_eq!(metric.loc.lloc(), 1.0);
                assert_eq!(metric.loc.cloc(), 2.0);
                assert_eq!(metric.loc.blank(), 0.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn typescript_if_lloc() {
        check_metrics::<TypescriptParser>(
            "function classify(n: number): string {
            if (n > 0) {
                return 'positive';
            } else {
                return 'non-positive';
            }
        }",
            "foo.ts",
            |metric| {
                assert_eq!(metric.loc.sloc(), 7.0);
                assert_eq!(metric.loc.ploc(), 7.0);
                assert_eq!(metric.loc.lloc(), 6.0);
                assert_eq!(metric.loc.cloc(), 0.0);
                assert_eq!(metric.loc.blank(), 0.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn typescript_for_lloc() {
        check_metrics::<TypescriptParser>(
            "function sum(n: number): number {
            let s = 0;
            for (let i = 0; i < n; i++) {
                s += i;
            }
            return s;
        }",
            "foo.ts",
            |metric| {
                assert_eq!(metric.loc.sloc(), 7.0);
                assert_eq!(metric.loc.ploc(), 7.0);
                assert_eq!(metric.loc.lloc(), 5.0);
                assert_eq!(metric.loc.cloc(), 0.0);
                assert_eq!(metric.loc.blank(), 0.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn typescript_while_lloc() {
        check_metrics::<TypescriptParser>(
            "function countdown(n: number): void {
            let i = n;
            while (i > 0) {
                console.log(i);
                i--;
            }
        }",
            "foo.ts",
            |metric| {
                assert_eq!(metric.loc.sloc(), 7.0);
                assert_eq!(metric.loc.ploc(), 7.0);
                assert_eq!(metric.loc.lloc(), 5.0);
                assert_eq!(metric.loc.cloc(), 0.0);
                assert_eq!(metric.loc.blank(), 0.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn typescript_switch_lloc() {
        check_metrics::<TypescriptParser>(
            "function describe(x: number): string {
            switch (x) {
                case 1: return 'one';
                case 2: return 'two';
                default: return 'other';
            }
        }",
            "foo.ts",
            |metric| {
                assert_eq!(metric.loc.sloc(), 7.0);
                assert_eq!(metric.loc.ploc(), 7.0);
                assert_eq!(metric.loc.lloc(), 5.0);
                assert_eq!(metric.loc.cloc(), 0.0);
                assert_eq!(metric.loc.blank(), 0.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn typescript_class_loc() {
        check_metrics::<TypescriptParser>(
            "class Counter {
            private count: number = 0;
            increment(): void { this.count++; }
            get(): number { return this.count; }
        }",
            "foo.ts",
            |metric| {
                assert_eq!(metric.loc.sloc(), 5.0);
                assert_eq!(metric.loc.ploc(), 5.0);
                assert_eq!(metric.loc.lloc(), 4.0);
                assert_eq!(metric.loc.cloc(), 0.0);
                assert_eq!(metric.loc.blank(), 0.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn typescript_arrow_function_loc() {
        check_metrics::<TypescriptParser>(
            "const add = (a: number, b: number): number => a + b;
        const greet = (name: string): string => {
            return `Hello, ${name}`;
        };",
            "foo.ts",
            |metric| {
                assert_eq!(metric.loc.sloc(), 4.0);
                assert_eq!(metric.loc.ploc(), 4.0);
                assert_eq!(metric.loc.lloc(), 2.0);
                assert_eq!(metric.loc.cloc(), 0.0);
                assert_eq!(metric.loc.blank(), 0.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn typescript_interface_loc() {
        check_metrics::<TypescriptParser>(
            "interface Shape {
            area(): number;
            perimeter(): number;
        }
        function describe(s: Shape): string {
            return `area=${s.area()}`;
        }",
            "foo.ts",
            |metric| {
                assert_eq!(metric.loc.sloc(), 7.0);
                assert_eq!(metric.loc.ploc(), 7.0);
                assert_eq!(metric.loc.lloc(), 2.0);
                assert_eq!(metric.loc.cloc(), 0.0);
                assert_eq!(metric.loc.blank(), 0.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn typescript_multiple_functions_loc() {
        check_metrics::<TypescriptParser>(
            "function f(): number {
            return 1;
        }
        function g(): number {
            return 2;
        }
        function h(): number {
            return 3;
        }",
            "foo.ts",
            |metric| {
                assert_eq!(metric.loc.sloc(), 9.0);
                assert_eq!(metric.loc.ploc(), 9.0);
                assert_eq!(metric.loc.lloc(), 6.0);
                assert_eq!(metric.loc.cloc(), 0.0);
                assert_eq!(metric.loc.blank(), 0.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn typescript_try_catch_lloc() {
        check_metrics::<TypescriptParser>(
            "function safe(x: number): number {
            try {
                return 1 / x;
            } catch (e) {
                return 0;
            }
        }",
            "foo.ts",
            |metric| {
                assert_eq!(metric.loc.sloc(), 7.0);
                assert_eq!(metric.loc.ploc(), 7.0);
                assert_eq!(metric.loc.lloc(), 6.0);
                assert_eq!(metric.loc.cloc(), 0.0);
                assert_eq!(metric.loc.blank(), 0.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn typescript_nested_functions_loc() {
        check_metrics::<TypescriptParser>(
            "function outer(x: number): number {
            function inner(y: number): number {
                return y * 2;
            }
            return inner(x) + 1;
        }",
            "foo.ts",
            |metric| {
                assert_eq!(metric.loc.sloc(), 6.0);
                assert_eq!(metric.loc.ploc(), 6.0);
                assert_eq!(metric.loc.lloc(), 4.0);
                assert_eq!(metric.loc.cloc(), 0.0);
                assert_eq!(metric.loc.blank(), 0.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn typescript_generic_function_loc() {
        check_metrics::<TypescriptParser>(
            "function identity<T>(value: T): T {
            return value;
        }
        function first<T>(arr: T[]): T | undefined {
            return arr[0];
        }",
            "foo.ts",
            |metric| {
                assert_eq!(metric.loc.sloc(), 6.0);
                assert_eq!(metric.loc.ploc(), 6.0);
                assert_eq!(metric.loc.lloc(), 4.0);
                assert_eq!(metric.loc.cloc(), 0.0);
                assert_eq!(metric.loc.blank(), 0.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn tsx_blank() {
        check_metrics::<TsxParser>(
            "function f(): void {

            const x = 1;

        }",
            "foo.tsx",
            |metric| {
                assert_eq!(metric.loc.sloc(), 5.0);
                assert_eq!(metric.loc.ploc(), 3.0);
                assert_eq!(metric.loc.lloc(), 1.0);
                assert_eq!(metric.loc.cloc(), 0.0);
                assert_eq!(metric.loc.blank(), 2.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn tsx_cloc() {
        check_metrics::<TsxParser>(
            "// header
        /* block
           comment */
        function f(): number {
            return 42; // inline
        }",
            "foo.tsx",
            |metric| {
                assert_eq!(metric.loc.sloc(), 6.0);
                assert_eq!(metric.loc.ploc(), 3.0);
                assert_eq!(metric.loc.lloc(), 2.0);
                assert_eq!(metric.loc.cloc(), 4.0);
                assert_eq!(metric.loc.blank(), 0.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn tsx_no_zero_blank() {
        check_metrics::<TsxParser>(
            "function f(): void {
            const x = 1; // x
            const y = 2; // y
        }",
            "foo.tsx",
            |metric| {
                assert_eq!(metric.loc.sloc(), 4.0);
                assert_eq!(metric.loc.ploc(), 4.0);
                assert_eq!(metric.loc.lloc(), 1.0);
                assert_eq!(metric.loc.cloc(), 2.0);
                assert_eq!(metric.loc.blank(), 0.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn tsx_if_lloc() {
        check_metrics::<TsxParser>(
            "function classify(n: number): string {
            if (n > 0) {
                return 'positive';
            } else {
                return 'non-positive';
            }
        }",
            "foo.tsx",
            |metric| {
                assert_eq!(metric.loc.sloc(), 7.0);
                assert_eq!(metric.loc.ploc(), 7.0);
                assert_eq!(metric.loc.lloc(), 6.0);
                assert_eq!(metric.loc.cloc(), 0.0);
                assert_eq!(metric.loc.blank(), 0.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn tsx_for_lloc() {
        check_metrics::<TsxParser>(
            "function sum(n: number): number {
            let s = 0;
            for (let i = 0; i < n; i++) {
                s += i;
            }
            return s;
        }",
            "foo.tsx",
            |metric| {
                assert_eq!(metric.loc.sloc(), 7.0);
                assert_eq!(metric.loc.ploc(), 7.0);
                assert_eq!(metric.loc.lloc(), 5.0);
                assert_eq!(metric.loc.cloc(), 0.0);
                assert_eq!(metric.loc.blank(), 0.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn tsx_while_lloc() {
        check_metrics::<TsxParser>(
            "function countdown(n: number): void {
            let i = n;
            while (i > 0) {
                console.log(i);
                i--;
            }
        }",
            "foo.tsx",
            |metric| {
                assert_eq!(metric.loc.sloc(), 7.0);
                assert_eq!(metric.loc.ploc(), 7.0);
                assert_eq!(metric.loc.lloc(), 5.0);
                assert_eq!(metric.loc.cloc(), 0.0);
                assert_eq!(metric.loc.blank(), 0.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn tsx_switch_lloc() {
        check_metrics::<TsxParser>(
            "function describe(x: number): string {
            switch (x) {
                case 1: return 'one';
                case 2: return 'two';
                default: return 'other';
            }
        }",
            "foo.tsx",
            |metric| {
                assert_eq!(metric.loc.sloc(), 7.0);
                assert_eq!(metric.loc.ploc(), 7.0);
                assert_eq!(metric.loc.lloc(), 5.0);
                assert_eq!(metric.loc.cloc(), 0.0);
                assert_eq!(metric.loc.blank(), 0.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn tsx_class_loc() {
        check_metrics::<TsxParser>(
            "class Counter {
            private count: number = 0;
            increment(): void { this.count++; }
            get(): number { return this.count; }
        }",
            "foo.tsx",
            |metric| {
                assert_eq!(metric.loc.sloc(), 5.0);
                assert_eq!(metric.loc.ploc(), 5.0);
                assert_eq!(metric.loc.lloc(), 4.0);
                assert_eq!(metric.loc.cloc(), 0.0);
                assert_eq!(metric.loc.blank(), 0.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn tsx_arrow_function_loc() {
        check_metrics::<TsxParser>(
            "const add = (a: number, b: number): number => a + b;
        const greet = (name: string): string => {
            return `Hello, ${name}`;
        };",
            "foo.tsx",
            |metric| {
                assert_eq!(metric.loc.sloc(), 4.0);
                assert_eq!(metric.loc.ploc(), 4.0);
                assert_eq!(metric.loc.lloc(), 2.0);
                assert_eq!(metric.loc.cloc(), 0.0);
                assert_eq!(metric.loc.blank(), 0.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn tsx_multiple_functions_loc() {
        check_metrics::<TsxParser>(
            "function f(): number {
            return 1;
        }
        function g(): number {
            return 2;
        }
        function h(): number {
            return 3;
        }",
            "foo.tsx",
            |metric| {
                assert_eq!(metric.loc.sloc(), 9.0);
                assert_eq!(metric.loc.ploc(), 9.0);
                assert_eq!(metric.loc.lloc(), 6.0);
                assert_eq!(metric.loc.cloc(), 0.0);
                assert_eq!(metric.loc.blank(), 0.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn tsx_try_catch_lloc() {
        check_metrics::<TsxParser>(
            "function safe(x: number): number {
            try {
                return 1 / x;
            } catch (e) {
                return 0;
            }
        }",
            "foo.tsx",
            |metric| {
                assert_eq!(metric.loc.sloc(), 7.0);
                assert_eq!(metric.loc.ploc(), 7.0);
                assert_eq!(metric.loc.lloc(), 6.0);
                assert_eq!(metric.loc.cloc(), 0.0);
                assert_eq!(metric.loc.blank(), 0.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn tsx_nested_functions_loc() {
        check_metrics::<TsxParser>(
            "function outer(x: number): number {
            function inner(y: number): number {
                return y * 2;
            }
            return inner(x) + 1;
        }",
            "foo.tsx",
            |metric| {
                assert_eq!(metric.loc.sloc(), 6.0);
                assert_eq!(metric.loc.ploc(), 6.0);
                assert_eq!(metric.loc.lloc(), 4.0);
                assert_eq!(metric.loc.cloc(), 0.0);
                assert_eq!(metric.loc.blank(), 0.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn tsx_interface_loc() {
        check_metrics::<TsxParser>(
            "interface Shape {
            area(): number;
            perimeter(): number;
        }
        function describe(s: Shape): string {
            return `area=${s.area()}`;
        }",
            "foo.tsx",
            |metric| {
                assert_eq!(metric.loc.sloc(), 7.0);
                assert_eq!(metric.loc.ploc(), 7.0);
                assert_eq!(metric.loc.lloc(), 2.0);
                assert_eq!(metric.loc.cloc(), 0.0);
                assert_eq!(metric.loc.blank(), 0.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn tsx_generic_function_loc() {
        check_metrics::<TsxParser>(
            "function identity<T>(value: T): T {
            return value;
        }
        function first<T>(arr: T[]): T | undefined {
            return arr[0];
        }",
            "foo.tsx",
            |metric| {
                assert_eq!(metric.loc.sloc(), 6.0);
                assert_eq!(metric.loc.ploc(), 6.0);
                assert_eq!(metric.loc.lloc(), 4.0);
                assert_eq!(metric.loc.cloc(), 0.0);
                assert_eq!(metric.loc.blank(), 0.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn php_blank() {
        check_metrics::<PhpParser>(
            "<?php

$a = 1;

$b = 2;

",
            "foo.php",
            |metric| {
                assert_eq!(metric.loc.sloc(), 5.0);
                assert_eq!(metric.loc.ploc(), 3.0);
                assert_eq!(metric.loc.lloc(), 2.0);
                assert_eq!(metric.loc.cloc(), 0.0);
                assert_eq!(metric.loc.blank(), 2.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn php_no_zero_blank() {
        check_metrics::<PhpParser>(
            "<?php
$a = 1;
$b = 2;",
            "foo.php",
            |metric| {
                assert_eq!(metric.loc.sloc(), 3.0);
                assert_eq!(metric.loc.ploc(), 3.0);
                assert_eq!(metric.loc.lloc(), 2.0);
                assert_eq!(metric.loc.cloc(), 0.0);
                assert_eq!(metric.loc.blank(), 0.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn php_cloc_double_slash() {
        check_metrics::<PhpParser>(
            "<?php
// first
// second
$a = 1; // trailing",
            "foo.php",
            |metric| {
                assert_eq!(metric.loc.sloc(), 4.0);
                assert_eq!(metric.loc.ploc(), 2.0);
                assert_eq!(metric.loc.lloc(), 1.0);
                assert_eq!(metric.loc.cloc(), 3.0);
                assert_eq!(metric.loc.blank(), 0.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn php_cloc_hash() {
        check_metrics::<PhpParser>(
            "<?php
# first
# second
$a = 1;",
            "foo.php",
            |metric| {
                assert_eq!(metric.loc.sloc(), 4.0);
                assert_eq!(metric.loc.ploc(), 2.0);
                assert_eq!(metric.loc.lloc(), 1.0);
                assert_eq!(metric.loc.cloc(), 2.0);
                assert_eq!(metric.loc.blank(), 0.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn php_cloc_block() {
        check_metrics::<PhpParser>(
            "<?php
/*
 * block
 * comment
 */
$a = 1;",
            "foo.php",
            |metric| {
                assert_eq!(metric.loc.sloc(), 6.0);
                assert_eq!(metric.loc.ploc(), 2.0);
                assert_eq!(metric.loc.lloc(), 1.0);
                assert_eq!(metric.loc.cloc(), 4.0);
                assert_eq!(metric.loc.blank(), 0.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn php_lloc() {
        // Three statements: assignment, if (with body), echo.
        check_metrics::<PhpParser>(
            "<?php
$a = 1;
if ($a > 0) {
    echo $a;
}",
            "foo.php",
            |metric| {
                assert_eq!(metric.loc.sloc(), 5.0);
                assert_eq!(metric.loc.ploc(), 5.0);
                assert_eq!(metric.loc.lloc(), 3.0);
                assert_eq!(metric.loc.cloc(), 0.0);
                assert_eq!(metric.loc.blank(), 0.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn php_no_parenthesized_expression_lloc() {
        // Parenthesized expression should not add an extra LLOC over the
        // surrounding expression_statement.
        check_metrics::<PhpParser>(
            "<?php
$a = (1 + 2);",
            "foo.php",
            |metric| {
                assert_eq!(metric.loc.sloc(), 2.0);
                assert_eq!(metric.loc.ploc(), 2.0);
                assert_eq!(metric.loc.lloc(), 1.0);
                assert_eq!(metric.loc.cloc(), 0.0);
                assert_eq!(metric.loc.blank(), 0.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn php_no_compound_statement_lloc() {
        // Block wrappers (`{ … }`) are not LLOC themselves.
        check_metrics::<PhpParser>(
            "<?php
function f(): void {
    $a = 1;
}",
            "foo.php",
            |metric| {
                assert_eq!(metric.loc.sloc(), 4.0);
                assert_eq!(metric.loc.ploc(), 4.0);
                assert_eq!(metric.loc.lloc(), 1.0);
                assert_eq!(metric.loc.cloc(), 0.0);
                assert_eq!(metric.loc.blank(), 0.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn php_no_colon_block_lloc() {
        // Alternative syntax (`if: … endif;`) uses ColonBlock instead of
        // CompoundStatement; it is also not LLOC.
        check_metrics::<PhpParser>(
            "<?php
if (true):
    $a = 1;
endif;",
            "foo.php",
            |metric| {
                assert_eq!(metric.loc.sloc(), 4.0);
                assert_eq!(metric.loc.ploc(), 4.0);
                assert_eq!(metric.loc.lloc(), 2.0);
                assert_eq!(metric.loc.cloc(), 0.0);
                assert_eq!(metric.loc.blank(), 0.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn php_no_else_clause_lloc() {
        // ElseClause and ElseIfClause are sub-parts of IfStatement.
        check_metrics::<PhpParser>(
            "<?php
if ($x) {
    $a = 1;
} elseif ($y) {
    $a = 2;
} else {
    $a = 3;
}",
            "foo.php",
            |metric| {
                assert_eq!(metric.loc.sloc(), 8.0);
                assert_eq!(metric.loc.ploc(), 8.0);
                assert_eq!(metric.loc.lloc(), 4.0);
                assert_eq!(metric.loc.cloc(), 0.0);
                assert_eq!(metric.loc.blank(), 0.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn php_no_case_statement_lloc() {
        // CaseStatement / DefaultStatement are switch arms, not separate
        // statements.
        check_metrics::<PhpParser>(
            "<?php
switch ($x) {
    case 1:
        $a = 1;
        break;
    case 2:
        $a = 2;
        break;
    default:
        $a = 0;
}",
            "foo.php",
            |metric| {
                assert_eq!(metric.loc.sloc(), 11.0);
                assert_eq!(metric.loc.ploc(), 11.0);
                assert_eq!(metric.loc.lloc(), 6.0);
                assert_eq!(metric.loc.cloc(), 0.0);
                assert_eq!(metric.loc.blank(), 0.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn php_no_match_arm_lloc() {
        // MatchConditionalExpression / MatchDefaultExpression are arms;
        // only the surrounding expression_statement counts.
        check_metrics::<PhpParser>(
            "<?php
$a = match ($x) {
    1 => 'one',
    2 => 'two',
    default => 'other',
};",
            "foo.php",
            |metric| {
                assert_eq!(metric.loc.sloc(), 6.0);
                assert_eq!(metric.loc.ploc(), 6.0);
                assert_eq!(metric.loc.lloc(), 1.0);
                assert_eq!(metric.loc.cloc(), 0.0);
                assert_eq!(metric.loc.blank(), 0.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn php_no_throw_in_expression_lloc() {
        // PHP 8 `throw` as expression: only the surrounding statement
        // counts (the `??` in this example), not the throw_expression.
        check_metrics::<PhpParser>(
            "<?php
$x = $y ?? throw new \\Exception('nope');",
            "foo.php",
            |metric| {
                assert_eq!(metric.loc.sloc(), 2.0);
                assert_eq!(metric.loc.ploc(), 2.0);
                assert_eq!(metric.loc.lloc(), 1.0);
                assert_eq!(metric.loc.cloc(), 0.0);
                assert_eq!(metric.loc.blank(), 0.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn php_no_closure_in_assignment_lloc() {
        // Anonymous function as RHS does not add an LLOC; only the
        // expression_statement counts. The closure body's statements are
        // counted in its own FuncSpace.
        check_metrics::<PhpParser>(
            "<?php
$f = function (): int {
    return 42;
};",
            "foo.php",
            |metric| {
                assert_eq!(metric.loc.sloc(), 4.0);
                assert_eq!(metric.loc.ploc(), 4.0);
                assert_eq!(metric.loc.lloc(), 2.0);
                assert_eq!(metric.loc.cloc(), 0.0);
                assert_eq!(metric.loc.blank(), 0.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn php_for_lloc() {
        // The for_statement contributes 1 LLOC; init/cond/update are NOT
        // separate statements in PHP's grammar.
        check_metrics::<PhpParser>(
            "<?php
for ($i = 0; $i < 10; $i++) {
    echo $i;
}",
            "foo.php",
            |metric| {
                assert_eq!(metric.loc.sloc(), 4.0);
                assert_eq!(metric.loc.ploc(), 4.0);
                assert_eq!(metric.loc.lloc(), 2.0);
                assert_eq!(metric.loc.cloc(), 0.0);
                assert_eq!(metric.loc.blank(), 0.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn php_foreach_lloc() {
        check_metrics::<PhpParser>(
            "<?php
foreach ($items as $k => $v) {
    echo $v;
}",
            "foo.php",
            |metric| {
                assert_eq!(metric.loc.sloc(), 4.0);
                assert_eq!(metric.loc.ploc(), 4.0);
                assert_eq!(metric.loc.lloc(), 2.0);
                assert_eq!(metric.loc.cloc(), 0.0);
                assert_eq!(metric.loc.blank(), 0.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn php_try_lloc() {
        check_metrics::<PhpParser>(
            "<?php
try {
    $a = 1;
} catch (\\Exception $e) {
    $a = 0;
} finally {
    $b = 2;
}",
            "foo.php",
            |metric| {
                assert_eq!(metric.loc.sloc(), 8.0);
                assert_eq!(metric.loc.ploc(), 8.0);
                assert_eq!(metric.loc.lloc(), 4.0);
                assert_eq!(metric.loc.cloc(), 0.0);
                assert_eq!(metric.loc.blank(), 0.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn php_class_loc() {
        check_metrics::<PhpParser>(
            "<?php
class A {
    public int $x = 0;
    private const Y = 1;
    public function f(): int {
        return $this->x;
    }
}",
            "foo.php",
            |metric| {
                assert_eq!(metric.loc.sloc(), 8.0);
                assert_eq!(metric.loc.ploc(), 8.0);
                assert_eq!(metric.loc.lloc(), 3.0);
                assert_eq!(metric.loc.cloc(), 0.0);
                assert_eq!(metric.loc.blank(), 0.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn php_namespace_use_lloc() {
        check_metrics::<PhpParser>(
            "<?php
namespace App;
use App\\Foo;
use App\\Bar;
$a = 1;",
            "foo.php",
            |metric| {
                assert_eq!(metric.loc.sloc(), 5.0);
                assert_eq!(metric.loc.ploc(), 5.0);
                assert_eq!(metric.loc.lloc(), 3.0);
                assert_eq!(metric.loc.cloc(), 0.0);
                assert_eq!(metric.loc.blank(), 0.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn php_general_loc() {
        check_metrics::<PhpParser>(
            "<?php
// header
namespace App;
use App\\Foo;

class Bar {
    public int $n = 0;

    public function add(int $x): int {
        if ($x > 0) {
            return $this->n + $x;
        }
        return $this->n;
    }
}",
            "foo.php",
            |metric| {
                assert_eq!(metric.loc.sloc(), 15.0);
                assert_eq!(metric.loc.ploc(), 12.0);
                assert_eq!(metric.loc.lloc(), 5.0);
                assert_eq!(metric.loc.cloc(), 1.0);
                assert_eq!(metric.loc.blank(), 2.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn php_match_in_expression_lloc() {
        // Match inside another expression (e.g. assignment RHS) — the
        // outer expression_statement counts, the inner match arms do not.
        check_metrics::<PhpParser>(
            "<?php
$y = 10 + match ($x) { 1 => 2, default => 0 };",
            "foo.php",
            |metric| {
                assert_eq!(metric.loc.sloc(), 2.0);
                assert_eq!(metric.loc.ploc(), 2.0);
                assert_eq!(metric.loc.lloc(), 1.0);
                assert_eq!(metric.loc.cloc(), 0.0);
                assert_eq!(metric.loc.blank(), 0.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn php_html_island_ploc() {
        // Embedded HTML between PHP tags ("text interpolation"). HTML
        // rows must contribute to PLOC (they are not blank and not a
        // PHP comment); this test locks that behavior so a future
        // grammar bump or impl tweak that excludes `text` nodes from
        // the default PLOC branch is caught.
        check_metrics::<PhpParser>(
            "<?php if ($cond): ?>
<div>hello</div>
<p>world</p>
<?php endif; ?>",
            "foo.php",
            |metric| {
                assert_eq!(metric.loc.sloc(), 4.0);
                assert_eq!(metric.loc.ploc(), 3.0);
                assert_eq!(metric.loc.lloc(), 1.0);
                assert_eq!(metric.loc.cloc(), 0.0);
                assert_eq!(metric.loc.blank(), 1.0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn php_short_echo_tag_ploc() {
        // `<?=` is the same `php_tag` kind as `<?php` per
        // tree-sitter-php 0.24.2. A regression that re-classified `<?=`
        // would shift PLOC; this test pins the current behavior.
        check_metrics::<PhpParser>("<p><?= $name ?></p>", "foo.php", |metric| {
            assert_eq!(metric.loc.sloc(), 1.0);
            assert_eq!(metric.loc.ploc(), 1.0);
            assert_eq!(metric.loc.lloc(), 1.0);
            assert_eq!(metric.loc.cloc(), 0.0);
            assert_eq!(metric.loc.blank(), 0.0);
            insta::assert_json_snapshot!(metric.loc)
        });
    }
}
