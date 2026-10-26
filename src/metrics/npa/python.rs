//! `Npa` implementation for Python.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use super::*;
use crate::lang_helpers::python::{PYTHON_TARGET_PATTERN_KINDS, python_self_attribute_name};

// Python attribute counting.
//
// Python has two flavours of class attributes:
// 1. Class-level (a.k.a. static): direct assignments inside the class
//    body — `class C: x = 1` or `class C: x: int = 1`.
// 2. Instance attributes: `self.x = …` assigned inside any method
//    body, conventionally inside `__init__`.
//
// Python has no visibility keyword. The PEP-8 convention `_x` for
// "internal" and `__x` for "name-mangled private" is purely advisory
// and not represented in the AST. We therefore treat every class
// attribute as public — `class_npa == class_na` — matching the Python
// ethos of "consenting adults". Documented as part of the trait
// contract for Python. (The impl does read the `code` source bytes —
// widened into the trait by #219 — but only to deduplicate attribute
// names, not to infer any visibility distinction.)
//
// Strategy: when the visitor hits a `ClassDefinition`, walk the body
// once and tally both class-level assignments and the `self.X = …`
// targets introduced by any method body. Counting on the
// `ClassDefinition` node (not its enclosed function spaces) keeps the
// attribution local to the surrounding class space, even though
// `self.X = …` lives inside a child `FunctionDefinition` space whose
// own `npa` stats are not class spaces.
impl Npa for PythonCode {
    fn compute<'a>(
        node: &Node<'a>,
        code: &'a [u8],
        ancestors: Ancestors<'a, '_>,
        stats: &mut Stats,
    ) {
        use Python::*;

        // Gate on `ClassDefinition` specifically: `is_func_space` is
        // also true for `Module` / `FunctionDefinition`, which would
        // over-eagerly mark every space as a class space.
        // role-bypass: allow(Container) — which container's body to walk;
        // not whether it opens a space
        if !matches!(node.kind_id().into(), ClassDefinition) {
            return;
        }

        let Some(body) = python_class_body(node) else {
            return;
        };

        // Counts of distinct class attributes (class-level + self.*).
        // `self.x` may appear in several methods — and in different
        // branches of the same method — but per Fitzpatrick's intent
        // each *attribute* counts once. We deduplicate by the
        // attribute identifier text (read via the `code` bytes
        // widened into the trait by #219), so:
        //   class C:
        //       def __init__(self): self.value = None
        //       def reset(self):    self.value = None
        // counts `value` once, not twice. Closes #215.
        //
        // The class-level and instance passes share one `seen` set so a
        // class default and an instance write of the same name collapse:
        //   class C:
        //       x = 1                       # class default
        //       def __init__(self): self.x = 2   # instance shadows it
        // counts `x` once (#412 dedup). Typical Python classes declare
        // under a dozen attributes; `with_capacity(8)` covers the common
        // case without a rehash and costs negligibly when fewer.
        let mut seen: std::collections::HashSet<&[u8]> =
            std::collections::HashSet::with_capacity(8);
        python_collect_class_level_attrs(&body, code, &mut seen);
        python_collect_unique_self_attrs(&body, code, ancestors, &mut seen);
        let total = seen.len();

        stats.class_na += total;
        // No visibility keyword in Python — every attribute is "public".
        stats.class_npa += total;
    }
}

// Returns the `block` body child of a `ClassDefinition` if present.
// `ClassDefinition` children are: `class` keyword, identifier,
// optional type-parameters, optional argument-list (base classes),
// `:`, block. The body is always the final child.
fn python_class_body<'a>(class_def: &Node<'a>) -> Option<Node<'a>> {
    class_def.children().find(python_is_block)
}

// Collects the names bound by class-level attribute assignments into
// `seen`. Walks direct `ExpressionStatement` children of the class
// body; for each contained `Assignment` carrying an `=` token
// (excluding bare type-only annotations like `x: int`, which parse as
// `Assignment` without an `=` — these declare a type but bind nothing),
// every bound *name* contributes one attribute. This counts each name,
// not each statement, so `a = b = 3` (chained) and `p, q = 1, 2`
// (unpacking) each contribute two attributes — mirroring Java's
// per-`VariableDeclarator` counting (#412 (c)). Names are deduplicated
// against the shared `seen` set so a class default `x = 1` and an
// instance `self.x = 2` count `x` once (instance shadows the class
// default — #412 dedup).
fn python_collect_class_level_attrs<'a>(
    body: &Node<'a>,
    code: &'a [u8],
    seen: &mut std::collections::HashSet<&'a [u8]>,
) {
    use Python::*;

    for stmt in body.children() {
        if stmt.kind_id() != ExpressionStatement {
            continue;
        }
        for child in stmt.children() {
            if child.kind_id() == Assignment && child.first_child(|id| id == EQ).is_some() {
                python_collect_bound_names_from_target(&child, code, seen);
            }
        }
    }
}

// Collects the simple-identifier names bound by a class-level
// `Assignment` target into `seen`, following chained `=` assignments
// and unpacking targets:
//   x = 1            → {x}
//   a = b = 3        → {a, b}   (nested Assignment in the value)
//   p, q = 1, 2      → {p, q}   (pattern_list / list_pattern target)
// Only simple-name bindings contribute here; an attribute target
// (`obj.x = …`) at class level is not a simple name binding and is
// ignored (the self/cls instance-attribute pass handles those).
// Walks an assignment / destructuring `target`, invoking `collect` on
// every leaf binding element with the chain from `chain`'s contents
// down through the unpacking wrappers (`PYTHON_TARGET_PATTERN_KINDS`),
// so `(a, (b, c)) = …` and `self.a, (self.b, self.c) = …` yield every
// bound element, not just the top-level ones. Non-pattern nodes —
// including punctuation children (commas, brackets) — are handed to
// `collect`, which filters by kind, exactly as the previous flat loop
// did. `chain` is left as it was found.
//
// Walked with an explicit stack, as `binds` in `lang_helpers/python.rs`
// is (#1597): a target nested thousands of brackets deep parses, and a
// recursive walk overflowed the consumer thread's stack at ~8,000
// levels. Each pending element records the chain depth it sits under,
// so popping it truncates the chain back to its wrapper; elements are
// pushed reversed so they are collected in document order.
fn python_walk_target_elements<'a>(
    target: &Node<'a>,
    chain: &mut Vec<Node<'a>>,
    collect: &mut impl FnMut(&Node<'a>, &[Node<'a>]),
) {
    let base = chain.len();
    let mut pending = vec![(*target, base)];
    while let Some((node, depth)) = pending.pop() {
        chain.truncate(depth);
        if PYTHON_TARGET_PATTERN_KINDS.contains(&node.kind_id()) {
            chain.push(node);
            let under = chain.len();
            let first = pending.len();
            pending.extend(node.children().map(|element| (element, under)));
            pending[first..].reverse();
        } else {
            collect(&node, chain);
        }
    }
    chain.truncate(base);
}

fn python_collect_bound_names_from_target<'a>(
    assignment: &Node<'a>,
    code: &'a [u8],
    seen: &mut std::collections::HashSet<&'a [u8]>,
) {
    let Some(target) = assignment.child(0) else {
        return;
    };
    // Every simple-name binding contributes one attribute, including names
    // nested inside an unpacking pattern. An attribute target (`obj.x = …`)
    // at class level is not a simple name binding and is ignored (the
    // self/cls instance-attribute pass handles those).
    python_walk_target_elements(&target, &mut Vec::new(), &mut |element, _| {
        if element.kind_id() == Python::Identifier
            && let Some(name) = code.get(element.start_byte()..element.end_byte())
        {
            seen.insert(name);
        }
    });
    // Chained `a = b = 3`: the right operand is itself a nested
    // `Assignment`, whose own target binds another name. Recurse so
    // every link in the chain is counted.
    if let Some(value) = assignment.child(assignment.child_count().saturating_sub(1))
        && value.kind_id() == Python::Assignment
    {
        python_collect_bound_names_from_target(&value, code, seen);
    }
}

// Collects the unique `self.<attr>` / `cls.<attr>` instance-attribute
// names bound anywhere in the class's method bodies into `seen`. Walks
// every method body once. Deduplicating by identifier text fixes #215:
// re-binding `self.x` across methods or branches no longer inflates the
// count. Sharing the `seen` set with the class-level pass also dedups
// across the two (#412 dedup).
fn python_collect_unique_self_attrs<'a>(
    body: &Node<'a>,
    code: &'a [u8],
    ancestors: Ancestors<'a, '_>,
    seen: &mut std::collections::HashSet<&'a [u8]>,
) {
    for stmt in body.children() {
        if let Some(func) = python_unwrap_function(&stmt, code, ancestors) {
            python_collect_self_attrs_in_subtree(&func, code, seen);
        }
    }
}

fn python_collect_self_attrs_in_subtree<'a>(
    root: &Node<'a>,
    code: &'a [u8],
    seen: &mut std::collections::HashSet<&'a [u8]>,
) {
    use Python::*;

    // One cursor for the whole subtree, not one per node: this is the
    // crate's heaviest `children` consumer by a wide margin — 92 % of
    // the Python metric walk's child scans — and each would otherwise
    // build and free a `TreeCursor` (#1112, `Node::children_with`).
    let mut cursor = root.cursor();
    let mut stack: Vec<Node<'a>> = Vec::with_capacity(32);
    stack.extend(root.children_with(&mut cursor));
    // One chain buffer for every assignment the walk meets, rather than
    // one allocation per assignment.
    let mut chain: Vec<Node<'a>> = Vec::with_capacity(4);
    while let Some(node) = stack.pop() {
        // Boundary: do not descend into nested classes, functions, or
        // lambdas. Their attributes belong to their inner scope.
        // role-bypass: allow(FuncOpener, Container, Closure) — the scope
        // boundary of a traversal, not whether a node opens a space
        if matches!(
            node.kind_id().into(),
            FunctionDefinition | ClassDefinition | DecoratedDefinition | Lambda
        ) {
            continue;
        }

        if node.kind_id() == Assignment {
            python_collect_self_attrs_from_target(&node, code, &mut chain, seen);
        }

        stack.extend(node.children_with(&mut cursor));
    }
}

// Collects `self.<attr>` / `cls.<attr>` names bound by an `Assignment`'s
// target into `seen`. Handles the single-attribute shape (`self.a = 1`),
// flat unpacking (`self.a, self.b = …`, #412 (b)), and nested unpacking
// (`self.a, (self.b, self.c) = …`) uniformly via the shared
// `python_walk_target_elements` recursion. A chained
// `self.a = self.b = 1` is handled by the caller's subtree walk: the
// nested `Assignment` in the value is visited as its own `Assignment`.
fn python_collect_self_attrs_from_target<'a>(
    assignment: &Node<'a>,
    code: &'a [u8],
    chain: &mut Vec<Node<'a>>,
    seen: &mut std::collections::HashSet<&'a [u8]>,
) {
    use crate::roles::python::member;
    let Some(target) = assignment.child(0) else {
        return;
    };
    // Collect only the `Member` row's elements — a bound `self` / `cls`
    // attribute; unpacking may mix them with foreign targets (`self.a,
    // x = …`), which the row declines. The element's chain is rebuilt
    // from the assignment down through the pattern wrappers, which is
    // what the row's gate climbs; the row reads the receiver and this
    // reads the name, one child scan between them.
    chain.clear();
    chain.push(*assignment);
    python_walk_target_elements(&target, chain, &mut |element, chain| {
        if member::owns(element, code, Ancestors::known(chain))
            && let Some(name) = python_self_attribute_name(element, code)
        {
            seen.insert(name);
        }
    });
}

// The `def` a class-body statement is, decorated or not — the
// `FuncOpener` row's callable, met directly or as the child of a
// `decorated_definition`. The row is ungated, so the chain handed to
// it (the body's, where the `def`'s is one longer) is never read;
// `npm` reads this through `super::npa` too (§7 parity).
pub(crate) fn python_unwrap_function<'a>(
    node: &Node<'a>,
    code: &[u8],
    ancestors: Ancestors<'a, '_>,
) -> Option<Node<'a>> {
    use crate::roles::python::func_opener;
    if func_opener::is_callable(node, code, ancestors) {
        return Some(*node);
    }
    (node.kind_id() == Python::DecoratedDefinition as u16)
        .then(|| {
            node.children()
                .find(|c| func_opener::is_callable(c, code, ancestors))
        })
        .flatten()
}
