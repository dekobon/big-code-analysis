// Executes the report's inline sort script against a minimal DOM stub and
// prints what it did, so a Rust test can assert on behaviour rather than
// on the script's spelling. Driven by `html_report::tests::
// inline_sort_script_behaves_under_a_dom_stub`; not a `node --test` file.
//
// Usage: node sort_script_harness.mjs <script-file>
//
// The stub implements only what the script touches: `document
// .querySelectorAll('table.hotspot')`, `thead th` lookup, `th
// .querySelector('button.sort')`, `tBodies`, `rows`, `cells`,
// `textContent`, `dataset`, the `aria-sort` attribute,
// `addEventListener` and `appendChild`. Collections are array-likes with
// the browser's method surface — `NodeList` has `forEach` but no `map`,
// `HTMLCollection` has neither — so a script that reaches for an Array
// method fails here as it would in a browser. `textContent` is a plain
// per-node string, not the concatenation of descendants a browser
// computes: a `<th>` with a child `<a>` reads `""`, which is fine while
// the script never reads a header's text. One table carries a numeric
// column whose cells are `5`, `12`, `` (blank) and `1,000` and a text
// column. Every header holds its label in a `button.sort` with an empty
// icon span, as the real page renders; the first also carries a sibling
// documentation link, as the metric headers do, and the third ships
// `aria-sort="descending"`, as the pre-ranked column of every hotspot
// table does. A second, one-column table proves sorting one does not
// touch the other.

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
    return collection(this.#descendants(selector), { forEach: true })
  }
  querySelector(selector) {
    return this.#descendants(selector)[0] ?? null
  }
  #descendants(selector) {
    const out = []
    const walk = (node) => {
      for (const child of node.#children) {
        if (matches(child, selector)) out.push(child)
        walk(child)
      }
    }
    walk(this)
    return out
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
    case "button.sort":
      return node.tag === "button" && node.attrs.get("class") === "sort"
    default:
      throw new Error(`stub does not model selector ${selector}`)
  }
}

const document = new Node("document")

// A header as the page renders it: `<th><button class="sort">Label<span
// class="sort-icon" aria-hidden="true"></span></button>[<a class="doc">?</a>]</th>`.
function header(row, label, { numeric = false, doc = false, sorted = null } = {}) {
  const th = row.appendChild(new Node("th"))
  th.setAttribute("scope", "col")
  if (numeric) th.setAttribute("data-numeric", "1")
  if (sorted) th.setAttribute("aria-sort", sorted)
  const button = th.appendChild(new Node("button", label))
  button.setAttribute("type", "button")
  button.setAttribute("class", "sort")
  const icon = button.appendChild(new Node("span"))
  icon.setAttribute("class", "sort-icon")
  icon.setAttribute("aria-hidden", "true")
  let link = null
  if (doc) {
    link = th.appendChild(new Node("a", "?"))
    link.setAttribute("class", "doc")
    link.setAttribute("href", "#")
  }
  return { th, button, icon, link }
}

const table = document.appendChild(new Node("table"))
table.setAttribute("class", "hotspot")
const headRow = table.appendChild(new Node("thead")).appendChild(new Node("tr"))
const numeric = header(headRow, "SLOC", { numeric: true, doc: true })
const text = header(headRow, "File")
const ranked = header(headRow, "Risk", { numeric: true, sorted: "descending" })
const tbody = table.appendChild(new Node("tbody"))
for (const [num, file, risk] of [
  ["5", "b.rs", "9.0"],
  ["12", "a.rs", "7.0"],
  ["", "d.rs", "3.0"],
  ["1,000", "c.rs", "1.0"],
]) {
  const tr = tbody.appendChild(new Node("tr"))
  tr.appendChild(new Node("td", num))
  tr.appendChild(new Node("td", file))
  tr.appendChild(new Node("td", risk))
}

const other = document.appendChild(new Node("table"))
other.setAttribute("class", "hotspot")
const otherHead = other.appendChild(new Node("thead")).appendChild(new Node("tr"))
const otherCol = header(otherHead, "Directory")
const otherBody = other.appendChild(new Node("tbody"))
for (const dir of ["src", "docs", "tests"]) {
  otherBody.appendChild(new Node("tr")).appendChild(new Node("td", dir))
}

const column = (idx) => Array.from(tbody.rows, (r) => r.cells[idx].textContent)
const otherColumn = () => Array.from(otherBody.rows, (r) => r.cells[0].textContent)
const out = {}

// The second table's order as rendered, captured before the script runs
// at all, so a script that sorted at load time — not only on click — is
// seen by the isolation check below.
const otherAsRendered = otherColumn()
out.otherAsRendered = otherAsRendered

// The script reads `document` as a free global.
new Function("document", script)(document)

// The page binds nothing to the header itself: the button is the control.
out.headerListeners = [numeric, text, ranked, otherCol].map((h) => h.th.listeners.size)
out.buttonListeners = [numeric, text, ranked, otherCol].map((h) => h.button.listeners.size)


// The pre-ranked column announces `descending` at render time, so its
// first activation must reverse to `ascending`, and clear nothing else
// since no other header is sorted yet.
ranked.button.dispatch("click", {})
out.rankedFirstClick = column(2)
out.rankedAriaAfterFirstClick = ranked.th.getAttribute("aria-sort")

numeric.button.dispatch("click", {})
out.numericAscending = column(0)
out.ariaAfterFirstClick = numeric.th.getAttribute("aria-sort")
numeric.button.dispatch("click", {})
out.numericDescending = column(0)
out.ariaAfterSecondClick = numeric.th.getAttribute("aria-sort")

text.button.dispatch("click", {})
out.textAscending = column(1)
out.numericAriaCleared = numeric.th.getAttribute("aria-sort")
out.rankedAriaCleared = ranked.th.getAttribute("aria-sort")

// Sorting one table leaves the other untouched, and vice versa.
out.otherUntouchedBySort = otherColumn().join() === otherAsRendered.join()
otherCol.button.dispatch("click", {})
out.otherAscending = otherColumn()
out.firstUntouchedByOther = column(1).join() === ["a.rs", "b.rs", "c.rs", "d.rs"].join()

// A click on the icon inside the button bubbles to the button and sorts.
const beforeIcon = column(0)
numeric.icon.dispatch("click", {})
out.iconClickMovedRows = column(0).join() !== beforeIcon.join()

// Only the button is the control: a click whose target is the
// documentation link, or the header cell itself, leaves the order alone.
// (The button carries the cell's padding, so in a browser the cell is
// rarely the target; the stub proves the listener is not on the th.)
const before = column(0)
numeric.link.dispatch("click", {})
out.linkClickMovedRows = column(0).join() !== before.join()
numeric.th.dispatch("click", {})
out.cellClickMovedRows = column(0).join() !== before.join()

process.stdout.write(JSON.stringify(out))
