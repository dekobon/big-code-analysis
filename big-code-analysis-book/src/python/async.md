# Async patterns

`bca.analyze` is CPU-bound: the work is a [tree-sitter](https://tree-sitter.github.io/tree-sitter/) parse plus
the metric passes, both of which release the
[GIL](https://docs.python.org/3/glossary.html#term-global-interpreter-lock)
on the Rust side via [PyO3](https://pyo3.rs/)'s `Python::detach`. The canonical pattern
for calling it from an [asyncio](https://docs.python.org/3/library/asyncio.html)
application is therefore `asyncio.to_thread`:

```python
{{#include ../../../big-code-analysis-py/examples/async_patterns.py:20:42}}
```

## Why `to_thread`, not native `async`

`bca.analyze` is a synchronous Python function backed by
synchronous Rust code; there is no `await` boundary inside it.
Wrapping it in `asyncio.to_thread`:

1. Schedules the call on the default thread pool.
2. Lets other coroutines progress while the parse and metric pass
   run.
3. Returns the result to the calling coroutine when done.

Because the Rust side releases the GIL across the heavy work,
several `to_thread(bca.analyze, ...)` calls run in parallel. This is
not co-operative I/O multiplexing; it is real multi-core use, capped
by the thread pool's size.

## Custom executors

`to_thread` always uses the event loop's default executor. For a
tighter cap on the worker count, use `loop.run_in_executor` with a
purpose-built pool instead:

```python
import asyncio
from concurrent.futures import ThreadPoolExecutor

import big_code_analysis as bca

async def analyze_many(paths: list[str]) -> list[object]:
    loop = asyncio.get_running_loop()
    with ThreadPoolExecutor(max_workers=8) as pool:
        return await asyncio.gather(
            *(loop.run_in_executor(pool, bca.analyze, p) for p in paths)
        )
```

Start with a worker count near the CPUs available to the process.
Benchmark your workload before increasing it; additional workers
compete for CPU time.

## Streaming results

`asyncio.as_completed` lets you start consuming results as soon
as the first analysis finishes, which helps when the per-file work
varies widely in cost (a 5 KB file against a 500 KB generated
bundle):

```python
import asyncio
import big_code_analysis as bca

async def first_failure(paths: list[str]) -> str | None:
    """Return the path of the first file with cyclomatic > 50."""
    tasks = [asyncio.create_task(asyncio.to_thread(bca.analyze, p)) for p in paths]
    try:
        for coro in asyncio.as_completed(tasks):
            result = await coro
            if result is None:
                continue
            if result["metrics"]["cyclomatic"]["sum"] > 50:
                return result["name"]
    finally:
        for t in tasks:
            t.cancel()
    return None
```

The `finally` block cancels pending asyncio tasks; `as_completed`
does not cancel them itself when the caller returns early. Analyses
already running in worker threads continue until they finish;
cancellation does not interrupt the Rust work.

## Anti-pattern: calling `bca.analyze` directly in a coroutine

```python
# Don't do this.
async def bad(path: str) -> dict | None:
    return bca.analyze(path)  # blocks the event loop on every call
```

`async def` does not make the body asynchronous. Without
`to_thread` or an explicit executor, every coroutine that calls
`bca.analyze` stalls the event loop for the full duration of the
parse: other tasks waiting on I/O, timers, or queues all freeze
until the parse returns. The `to_thread` wrapper is one line and
is the difference between a responsive server and a
single-threaded one.

## When `analyze_batch` is the better fit

If you are processing a static, finite list of paths and do not
need streaming results, [`bca.analyze_batch`](batch.md) is
simpler than `gather(*to_thread(...))`: it runs sequentially on
the calling thread but never raises on per-file errors. Wrap the
whole `analyze_batch` call in `asyncio.to_thread` to keep the
event loop responsive:

```python
import asyncio
import big_code_analysis as bca

async def batch(paths: list[str]) -> list[object]:
    return await asyncio.to_thread(bca.analyze_batch, paths)
```

This trades the per-file parallelism of `gather` for the
simpler error model of `analyze_batch`. Pick `gather` when you
want both parallelism and typed `OSError` dispatch; pick
`to_thread(analyze_batch, paths)` when you want one async call
and the never-raise contract.
