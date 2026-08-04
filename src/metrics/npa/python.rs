//! `Npa` implementation for Python.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use super::*;

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
        _ancestors: Ancestors<'a, '_>,
        stats: &mut Stats,
    ) {
        use Python::*;

        // Gate on `ClassDefinition` specifically: `is_func_space` is
        // also true for `Module` / `FunctionDefinition`, which would
        // over-eagerly mark every space as a class space.
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
        python_collect_unique_self_attrs(&body, code, &mut seen);
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
// every leaf binding element. Recurses through nested unpacking patterns
// (`pattern_list` / `expression_list` / `tuple_pattern` / `list_pattern`)
// so `(a, (b, c)) = …` and `self.a, (self.b, self.c) = …` yield every
// bound element, not just the top-level ones. Non-pattern nodes —
// including punctuation children (commas, brackets) — are handed to
// `collect`, which filters by kind, exactly as the previous flat loop did.
fn python_walk_target_elements<'a>(target: &Node<'a>, collect: &mut impl FnMut(&Node<'a>)) {
    match target.kind_id().into() {
        // `tuple_pattern` / `list_pattern` each carry two aliased kind_ids:
        // the hidden supertype (`TuplePattern` 168 / `ListPattern` 167,
        // never emitted) and the live node the grammar actually produces
        // for `(a, b) = …` / `[a, b] = …` (`TuplePattern2` 179 /
        // `ListPattern2` 180). Matching only the supertype alias silently
        // dropped every parenthesized/bracketed unpacking target, so
        // enumerate both aliases per the hidden-alias discipline (#419).
        Python::PatternList
        | Python::ExpressionList
        | Python::TuplePattern
        | Python::TuplePattern2
        | Python::ListPattern
        | Python::ListPattern2 => {
            for element in target.children() {
                python_walk_target_elements(&element, collect);
            }
        }
        _ => collect(target),
    }
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
    python_walk_target_elements(&target, &mut |element| {
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
    seen: &mut std::collections::HashSet<&'a [u8]>,
) {
    for stmt in body.children() {
        if let Some(func) = python_unwrap_function(&stmt) {
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
    while let Some(node) = stack.pop() {
        // Boundary: do not descend into nested classes, functions, or
        // lambdas. Their attributes belong to their inner scope.
        if matches!(
            node.kind_id().into(),
            FunctionDefinition | ClassDefinition | DecoratedDefinition | Lambda
        ) {
            continue;
        }

        if node.kind_id() == Assignment {
            python_collect_self_attrs_from_target(&node, code, seen);
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
    seen: &mut std::collections::HashSet<&'a [u8]>,
) {
    let Some(target) = assignment.child(0) else {
        return;
    };
    // Collect only the `self`/`cls` attribute elements; unpacking may mix
    // them with foreign targets (`self.a, x = …`), which are filtered out
    // here because they are not `self`/`cls` attributes.
    python_walk_target_elements(&target, &mut |element| {
        if element.kind_id() == Python::Attribute
            && let Some(name) = python_self_attr_name_bytes(element, code)
        {
            seen.insert(name);
        }
    });
}

// Conventional receiver names that denote the enclosing object inside
// a method body: `self` for instance methods, `cls` for classmethods.
// We match the receiver bytes against these literals rather than
// resolving the enclosing function's first parameter. The pragmatic
// choice (per #412): reading source bytes and matching `self`/`cls` is
// clearly better than the prior structural-only proxy (which counted
// ANY `obj.x = …` as an instance attribute) and covers the
// overwhelming majority of real code. A non-conventionally-named first
// parameter is rare enough that under-counting it is preferable to the
// over-count of treating every foreign-object write as an attribute.
const PYTHON_SELF_RECEIVERS: [&[u8]; 2] = [b"self", b"cls"];

// Returns the byte slice for the attribute identifier of a
// `self.<attr>` / `cls.<attr>` Attribute node, or `None` when the
// receiver is not a self/cls alias.
//
// `attr` is an `Attribute` node. Its first child is the receiver:
//   self.x       → receiver is Identifier "self"      → counts
//   db.x         → receiver is Identifier "db"        → foreign, skip
//   self.f.g     → receiver is itself an Attribute    → skip
// The last named child is the attribute identifier (the `.` and the
// preceding receiver are siblings; the identifier comes last). Only a
// direct `self.<name> = …` introduces an attribute of the class;
// `self.f.g = …` writes attribute `g` on `self.f`, not a new attribute
// of the class, so the nested-Attribute receiver is intentionally
// rejected. Borrows directly from `code` so the returned slice is the
// canonical dedup key — two `self.value` writes share the same key.
fn python_self_attr_name_bytes<'a>(attr: &Node<'a>, code: &'a [u8]) -> Option<&'a [u8]> {
    // Fully-qualified `Python::*` names — this function deliberately
    // does NOT `use Python::*;` so unqualified `None` keeps its
    // `Option` meaning rather than being shadowed by `Python::None`.
    let receiver = attr.child(0)?;
    if receiver.kind_id() != Python::Identifier {
        return None;
    }
    let receiver_bytes = code.get(receiver.start_byte()..receiver.end_byte())?;
    if !PYTHON_SELF_RECEIVERS.contains(&receiver_bytes) {
        return None;
    }
    // The trailing identifier is the last `Identifier` child of the
    // Attribute node; `.last()` walks the children once and yields it.
    let id = attr
        .children()
        .filter(|c| c.kind_id() == Python::Identifier)
        .last()?;
    // Guard the degenerate single-identifier case: only count when the
    // attribute name is a distinct Identifier from the receiver.
    if id.start_byte() == receiver.start_byte() {
        return None;
    }
    code.get(id.start_byte()..id.end_byte())
}

fn python_unwrap_function<'a>(node: &Node<'a>) -> Option<Node<'a>> {
    // Use fully-qualified names here: `use Python::*` would shadow
    // `Option::None` with `Python::None` and break the last arm.
    match node.kind_id().into() {
        Python::FunctionDefinition => Some(*node),
        Python::DecoratedDefinition => node
            .children()
            .find(|c| c.kind_id() == Python::FunctionDefinition),
        _ => None,
    }
}
