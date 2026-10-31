# Ruby visibility oracle (#1401)

The `#1401` test block in `src/metrics/npm.rs` asserts, for each
class-body shape the Ruby `npm` walk reads, which of the methods it
counts are public. The *visibility* half of several of those rows was
taken from Ruby itself rather than written by hand: this script, under
ruby 3.0.2, reports what `public_instance_methods(false)` and its
siblings file for six of the shapes, and the walk in
`src/metrics/npm/ruby.rs` was changed to agree with it, never the other
way round. It is the oracle in [lesson 6](lessons_learned.md)'s sense
for those rows, and a retained subset of the investigation's probes,
not a transcript of every test.

Two things it cannot say, which the tests say for themselves. The walk
is syntactic and counts every branch, as `nom` does, so `Cond` reports
three methods here while the test counts four: `legacy` exists only on
the branch Ruby did not take, and the metric takes both. And the test
fixtures are not these classes verbatim; several carry extra rows
(a doubly decorated `twice`, the parenthesised, rescue-modifier and
assignment positions, the later shadowing cases) whose visibility was
settled by the same method on one-off probes that were not retained.

One expectation is deliberately absent from the output. `private
public def x` — a visibility keyword applied to what a nested one
returns — is a `TypeError` on 3.0 (`private: nil is not a symbol nor a
string`) and defines `x` private on 3.1 and later, because 3.1's
`set_visibility` returns its argument. The walk follows 3.1; the test
row says so.

```ruby
def report(k)
  pub = k.public_instance_methods(false).sort
  prv = k.private_instance_methods(false).sort
  prot = k.protected_instance_methods(false).sort
  sing = k.singleton_methods(false).sort
  puts "#{k}: public=#{pub} private=#{prv} protected=#{prot} singleton=#{sing}"
end
class Memo
  def self.memoize(name); name; end
  memoize def cached; end
  def plain; end
end
report(Memo)
class Cond
  if RUBY_VERSION > "3.0"
    def modern; end
  else
    def legacy; end
  end
  private
  begin
    def wrapped; end
  end
  def plain; end
end
report(Cond)
class L
  while false
    def in_while; end
  end
  for i in [] do
    def in_for; end
  end
  def early; end if true
  begin
    def in_begin; end
  rescue
    def in_rescue; end
  ensure
    def in_ensure; end
  end
  private = false
  if private
    def guarded; end
  end
  def still_public; end
end
report(L)
class K
  x = 1
  case x
  when 1
    def w1; end
  else
    def welse; end
  end
  case x
  in Integer
    def in1; end
  end
  until true
    def u; end
  end
  def early2; end unless false
  def early3; end while false
end
report(K)
class Dec
  def self.memoize(n); n; end
  memoize memoize def twice; end
  memoize def initialize; end
  private
  memoize def after_private; end
end
report(Dec)
class Inside
  private
  if true
    public
  end
  def after_branch; end
end
report(Inside)
```

Output under `ruby 3.0.2p107` (re-run 2026-10-31, identical to the
capture taken for the fix):

```text
Memo: public=[:cached, :plain] private=[] protected=[] singleton=[:memoize]
Cond: public=[:modern] private=[:plain, :wrapped] protected=[] singleton=[]
L: public=[:early, :in_begin, :in_ensure, :still_public] private=[] protected=[] singleton=[]
K: public=[:early2, :in1, :w1] private=[] protected=[] singleton=[]
Dec: public=[:twice] private=[:after_private, :initialize] protected=[] singleton=[:memoize]
Inside: public=[:after_branch] private=[] protected=[] singleton=[]
```
