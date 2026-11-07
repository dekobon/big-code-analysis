// Executes the report's inline sort script against a minimal DOM stub and
// prints what it did, so a Rust test can assert on behaviour rather than
// on the script's spelling. Driven by `html_report::tests::
// inline_sort_script_behaves_under_a_dom_stub`; not a `node --test` file.
//
// Usage: node sort_script_harness.mjs <script-file>
//
// The stub implements only what the script touches: `document
// .querySelectorAll('table.hotspot')`, `thead th` lookup, `tBodies`,
// `rows`, `cells`, `textContent`, `dataset`, the `aria-sort` attribute,
// `addEventListener` and `appendChild`. Collections are array-likes with
// the browser's method surface — `NodeList` has `forEach` but no `map`,
// `HTMLCollection` has neither — so a script that reaches for an Array
// method fails here as it would in a browser. `textContent` is a plain
// per-node string, not the concatenation of descendants a browser
// computes: a `<th>` with a child `<a>` reads `""`, which is fine while
// the script never reads a header's text. One table carries a numeric
// column whose cells are `5`, `12`, `` (blank) and `1,000` and a text
// column; its first header holds a child link, as the metric headers
// in the real page do, and its third ships `aria-sort="descending"`,
// as the pre-ranked column of every hotspot table does.

import { readFileSync } from "node:fs"

const script = readFileSync(process.argv[2], "utf8")

// An array-like: indices and `length`, plus `forEach` where the browser
// collection has it. No `map`, `filter`, `slice` or `sort`.
function collection(items, { forEach = false } = {}) {
  const c = Object.create(null)
  items.forEach((n, i) => {
    c[i] = n
  })
  c.length = items.length
  if (forEach) c.forEach = (fn) => items.forEach(fn)
  return c
}

class Node {
  // Private so no reader gets a real Array; `children` below is the
  // browser's `HTMLCollection` shape.
  #children
  constructor(tag, text = "") {
    this.tag = tag
    this.textContent = text
    this.#children = []
    this.attrs = new Map()
    this.listeners = new Map()
    this.parent = null
  }
  get children() {
    return collection(this.#children)
  }
  // `dataset.foo` reads the `data-foo` attribute, as in a browser.
  get dataset() {
    const attrs = this.attrs
    return new Proxy(
      {},
      {
        get: (_, key) => (typeof key === "string" ? (attrs.get(`data-${key}`) ?? undefined) : undefined),
      },
    )
  }
  appendChild(child) {
    if (child.parent) child.parent.#children.splice(child.parent.#children.indexOf(child), 1)
    child.parent = this
    this.#children.push(child)
    return child
  }
  getAttribute(name) {
    return this.attrs.has(name) ? this.attrs.get(name) : null
  }
  setAttribute(name, value) {
    this.attrs.set(name, String(value))
  }
  removeAttribute(name) {
    this.attrs.delete(name)
  }
  addEventListener(type, fn) {
    if (!this.listeners.has(type)) this.listeners.set(type, [])
    this.listeners.get(type).push(fn)
  }
  // Bubbling dispatch: the event's `target` is the node dispatched on,
  // listeners run from the target up through its ancestors.
  dispatch(type, event) {
    event.target = this
    event.defaultPrevented = false
    event.preventDefault = () => {
      event.defaultPrevented = true
    }
    for (let node = this; node; node = node.parent) {
      for (const fn of node.listeners.get(type) ?? []) fn(event)
    }
    return event
  }
  querySelectorAll(selector) {
    const out = []
    const walk = (node) => {
      for (const child of node.#children) {
        if (matches(child, selector)) out.push(child)
        walk(child)
      }
    }
    walk(this)
    return collection(out, { forEach: true })
  }
  // Each getter exists only on the element the DOM defines it for, so
  // a script reading `tbl.rows` or `tbody.children` fails here by name
  // rather than as an empty readback that reads like a sort bug.
  get tBodies() {
    this.only("table", "tBodies")
    return collection(this.#children.filter((c) => c.tag === "tbody"))
  }
  get rows() {
    this.only(["tbody", "thead", "table"], "rows")
    return collection(this.#children.filter((c) => c.tag === "tr"))
  }
  get cells() {
    this.only("tr", "cells")
    return collection(this.#children.filter((c) => c.tag === "td" || c.tag === "th"))
  }
  only(tags, prop) {
    if (![tags].flat().includes(this.tag)) throw new Error(`stub does not model <${this.tag}>.${prop}`)
  }
}

function matches(node, selector) {
  switch (selector) {
    case "table.hotspot":
      return node.tag === "table" && node.attrs.get("class") === "hotspot"
    case "thead th":
      return node.tag === "th" && node.parent?.parent?.tag === "thead"
    default:
      throw new Error(`stub does not model selector ${selector}`)
  }
}

const document = new Node("document")
const table = document.appendChild(new Node("table"))
table.setAttribute("class", "hotspot")
const thead = table.appendChild(new Node("thead"))
const headRow = thead.appendChild(new Node("tr"))
const numericTh = headRow.appendChild(new Node("th"))
numericTh.setAttribute("data-numeric", "1")
const link = numericTh.appendChild(new Node("a", "SLOC"))
const textTh = headRow.appendChild(new Node("th", "File"))
const rankedTh = headRow.appendChild(new Node("th", "Risk"))
rankedTh.setAttribute("data-numeric", "1")
rankedTh.setAttribute("aria-sort", "descending")
const tbody = table.appendChild(new Node("tbody"))
for (const [num, text, risk] of [
  ["5", "b.rs", "9.0"],
  ["12", "a.rs", "7.0"],
  ["", "d.rs", "3.0"],
  ["1,000", "c.rs", "1.0"],
]) {
  const tr = tbody.appendChild(new Node("tr"))
  tr.appendChild(new Node("td", num))
  tr.appendChild(new Node("td", text))
  tr.appendChild(new Node("td", risk))
}

// The script reads `document` as a free global.
new Function("document", script)(document)

const column = (idx) => Array.from(tbody.rows, (r) => r.cells[idx].textContent)
const out = {}

// The pre-ranked column announces `descending` at render time, so its
// first activation must reverse to `ascending`, and clear nothing else
// since no other header is sorted yet.
rankedTh.dispatch("click", {})
out.rankedFirstClick = column(2)
out.rankedAriaAfterFirstClick = rankedTh.getAttribute("aria-sort")

numericTh.dispatch("click", {})
out.numericAscending = column(0)
out.ariaAfterFirstClick = numericTh.getAttribute("aria-sort")
numericTh.dispatch("click", {})
out.numericDescending = column(0)
out.ariaAfterSecondClick = numericTh.getAttribute("aria-sort")

textTh.dispatch("click", {})
out.textAscending = column(1)
out.numericAriaCleared = numericTh.getAttribute("aria-sort")
out.rankedAriaCleared = rankedTh.getAttribute("aria-sort")

// Keyboard on the header itself sorts and swallows the key's default.
const enterOnHeader = textTh.dispatch("keydown", { key: "Enter" })
out.enterOnHeaderSorts = column(1)
out.enterOnHeaderPrevented = enterOnHeader.defaultPrevented
const spaceOnHeader = numericTh.dispatch("keydown", { key: " " })
out.spaceOnHeaderSorts = column(0)
out.spaceOnHeaderPrevented = spaceOnHeader.defaultPrevented
// Enter on the child link bubbles to the header; the link must keep its
// default (navigation) and the order must not move. A browser activates
// an anchor whose Enter was not cancelled by synthesizing a `click` on
// it, which bubbles to the header too, so model that second event.
const before = column(0)
const enterOnLink = link.dispatch("keydown", { key: "Enter" })
out.enterOnLinkPrevented = enterOnLink.defaultPrevented
if (!enterOnLink.defaultPrevented) link.dispatch("click", {})
out.enterOnLinkMovedRows = column(0).join() !== before.join()
// A mouse click on the link is the link's too.
link.dispatch("click", {})
out.clickOnLinkMovedRows = column(0).join() !== before.join()
// An unrelated key does nothing.
const tab = numericTh.dispatch("keydown", { key: "Tab" })
out.tabPrevented = tab.defaultPrevented

process.stdout.write(JSON.stringify(out))
