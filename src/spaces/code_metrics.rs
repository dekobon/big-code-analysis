//! Inherent and `Display` impl blocks for [`super::CodeMetrics`].
//!
//! Split out of `spaces.rs` to keep that module focused on the public
//! API type definitions. The blocks are moved verbatim; method and
//! trait resolution is by type, so `crate::spaces::CodeMetrics`'s
//! methods and `Display` impl resolve unchanged.

use super::*;

impl CodeMetrics {
    /// Construct a `CodeMetrics` whose `selected` mask is the given
    /// [`MetricSet`]. All metric fields are at their `Default` value;
    /// the walker fills them in for whichever metrics the mask
    /// admits.
    #[inline]
    #[must_use]
    pub fn with_selected(selected: MetricSet) -> Self {
        Self {
            selected,
            ..Self::default()
        }
    }

    /// Returns the set of metrics that were computed for this space.
    #[inline]
    #[must_use]
    pub fn selected(&self) -> MetricSet {
        self.selected
    }
}

impl fmt::Display for CodeMetrics {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        writeln!(f, "{}", self.nargs)?;
        writeln!(f, "{}", self.nexits)?;
        writeln!(f, "{}", self.cognitive)?;
        writeln!(f, "{}", self.cyclomatic)?;
        writeln!(f, "{}", self.halstead)?;
        writeln!(f, "{}", self.loc)?;
        writeln!(f, "{}", self.nom)?;
        writeln!(f, "{}", self.tokens)?;
        write!(f, "{}", self.mi)
    }
}

impl CodeMetrics {
    /// Project these metrics into their [`crate::wire::CodeMetrics`] form,
    /// eliding metrics not in [`CodeMetrics::selected`] (and disabled
    /// class-only metrics) exactly as the serialized output does.
    #[must_use]
    pub fn to_wire(&self) -> crate::wire::CodeMetrics {
        crate::wire::CodeMetrics::from(self)
    }

    /// Sum each metric component from `other` into `self` in place. Used to
    /// roll nested function-space metrics into their parent space.
    pub fn merge(&mut self, other: &CodeMetrics) {
        self.cognitive.merge(&other.cognitive);
        self.cyclomatic.merge(&other.cyclomatic);
        self.halstead.merge(&other.halstead);
        self.loc.merge(&other.loc);
        self.nom.merge(&other.nom);
        self.tokens.merge(&other.tokens);
        self.mi.merge(&other.mi);
        self.nargs.merge(&other.nargs);
        self.nexits.merge(&other.nexits);
        self.abc.merge(&other.abc);
        self.wmc.merge(&other.wmc);
        self.npm.merge(&other.npm);
        self.npa.merge(&other.npa);
        // Union the selection masks so a parent space's emitted
        // fields are the union of every nested space's selection.
        // In practice every nested space shares the same mask (set
        // once from `MetricsOptions::metrics`), so this is the
        // identity operation; we union rather than assign to keep
        // `merge` correct under future callers that mix
        // independently-built `FuncSpace` values.
        self.selected = self.selected.union(other.selected);
    }
}
