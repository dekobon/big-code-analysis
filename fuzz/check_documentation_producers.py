#!/usr/bin/env python3
"""Verify bounded documentation producer perturbations and restore exact source.

Run from the repository root with --out pointing to an owned evidence directory.
The AST substitutions are temporary verification inputs, never shipped changes.
"""

import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess


def cases():
    """Return unique producer anchors and independently expected rejection sites."""
    qualification = (
        "big-code-analysis-ast/src/documentation/collector/qualifications.rs"
    )
    collector = "big-code-analysis-ast/src/documentation/collector.rs"
    root = "self.index.canonical.nodes[0].evidence"
    yield (
        "candidate-root",
        qualification,
        "let node = self.index.canonical.nodes[self.index.owner_nodes[candidate.owner.0].0].evidence;",
        f"let node = {root};",
        "qualification candidate evidence differs from owner",
    )
    yield (
        "reference-root",
        qualification,
        "                subject: QualificationSubject::Reference,\n                reason,\n                node,",
        f"                subject: QualificationSubject::Reference,\n                reason,\n                node: {root},",
        "qualification reference evidence is not a document fragment",
    )
    for name, reason in [
        ("candidate-resolution", "Ambiguous"),
        ("reference-resolution", "Conditional"),
    ]:
        yield (
            name,
            collector,
            "    Ok(builder.index)\n}",
            "    for association in &mut builder.index.facts.associations {\n"
            f"        if association.resolution == Resolution::Unresolved(UnresolvedReason::{reason}) {{\n"
            "            association.resolution = Resolution::Resolved;\n        }\n    }\n    Ok(builder.index)\n}",
            "seed association resolution",
        )
    changes = {
        "repeat-slot": "for owner in &mut builder.index.facts.owners { if let Owner::Slot(slot) = owner { slot.role = SlotRole::Binding; } }",
        "repeat-binding": "for association in &mut builder.index.facts.associations { association.binding = Some(builder.index.canonical.nodes[0].evidence); }",
        "repeat-scope": "for association in &mut builder.index.facts.associations { let mut node = association.lexical_scope.node(); node.edit(&tree_sitter::InputEdit { start_byte: 0, old_end_byte: 0, new_end_byte: 1, start_position: tree_sitter::Point::new(0, 0), old_end_position: tree_sitter::Point::new(0, 0), new_end_position: tree_sitter::Point::new(0, 1) }); association.lexical_scope = builder.index.source.node(node)?; }",
    }
    for name, change in changes.items():
        replacement = (
            "    thread_local! { static CALLS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) }; }\n"
            "    let second = CALLS.with(|calls| { let n = calls.get() + 1; calls.set(n); n == 2 });\n"
            f"    if second {{ {change} }}\n    Ok(builder.index)\n}}"
        )
        yield (
            name,
            collector,
            "    Ok(builder.index)\n}",
            replacement,
            "nondeterministic slot fields"
            if name == "repeat-slot"
            else "nondeterministic node evidence",
        )


def run(out, name, test, mutation):
    """Rebuild and run one exact guard; retain complete output and return status."""
    command = [
        "cargo",
        "+nightly",
        "test",
        "--manifest-path",
        "fuzz/Cargo.toml",
        "--locked",
        "--lib",
        test,
        "--",
        "--exact",
        "--nocapture",
    ]
    env = os.environ.copy()
    env.pop("BCA_FUZZ_PRODUCER_CASE", None)
    if mutation:
        env["BCA_FUZZ_PRODUCER_CASE"] = mutation
    result = subprocess.run(
        command, env=env, stdout=subprocess.PIPE, stderr=subprocess.STDOUT
    )
    output = result.stdout.decode(errors="replace")
    (out / f"{name}.log").write_bytes(result.stdout)
    if "error[E" in output or "could not compile" in output:
        raise RuntimeError(f"{name}: compilation failure is not a catch")
    return {
        "command": command,
        "mutation_env": mutation,
        "exit": result.returncode,
    }, output


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    if subprocess.check_output(["git", "status", "--porcelain"]):
        raise SystemExit("producer verification requires a clean tracked checkpoint")
    head = subprocess.check_output(["git", "rev-parse", "HEAD"]).decode().strip()
    prefix = "documentation::tests::remediation::"
    records = []
    for name, filename, old, new, message in cases():
        path = Path(filename)
        subprocess.run(
            ["git", "ls-files", "--error-unmatch", filename],
            check=True,
            stdout=subprocess.DEVNULL,
        )
        original = path.read_bytes()
        assert old != new and original.count(old.encode()) == 1, name
        (out / f"{name}.original").write_bytes(original)
        test = prefix + (
            "producer_second_collection_repeat_control"
            if name.startswith("repeat-")
            else "producer_qualification_runtime_and_registry_controls"
        )
        baseline, text = run(out, name + "-setup", test, None)
        assert baseline["exit"] == 0 and "1 passed" in text, (name, "invalid setup")
        try:
            path.write_bytes(original.replace(old.encode(), new.encode()))
            controlled, text = run(out, name + "-controlled", test, name)
            assert controlled["exit"] == 0 and "1 passed" in text, (
                name,
                "wrong controlled outcome",
            )
            ordinary_test = (
                test
                if name.startswith("repeat-")
                else "documentation::tests::registry_executes_every_dispatch_and_edge_assertion"
            )
            ordinary, text = run(out, name + "-ordinary", ordinary_test, None)
            assert (
                ordinary["exit"] == 101
                and message in text
                and f"{ordinary_test} ... FAILED" in text
            ), (name, "wrong ordinary failure")
            records.append(
                {
                    "name": name,
                    "revision": head,
                    "baseline": baseline,
                    "controlled": controlled,
                    "ordinary": ordinary,
                    "rejection": message,
                    "original_sha256": hashlib.sha256(original).hexdigest(),
                }
            )
        finally:
            path.write_bytes(original)
            assert path.read_bytes() == original
            (out / "results.json").write_text(json.dumps(records, indent=2) + "\n")
        print(
            f"{name}: controlled rejection verified; ordinary guard failed at {message}",
            flush=True,
        )
    assert not subprocess.check_output(["git", "status", "--porcelain"])
    assert (
        subprocess.check_output(["git", "rev-parse", "HEAD"]).decode().strip() == head
    )


if __name__ == "__main__":
    main()
