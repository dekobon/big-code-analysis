//! Per-metric wire structs and their `From<&<metric>::Stats>`
//! projections — the per-metric arm of the wire shape (see the parent
//! [`super`] module doc for the single-source-of-truth rationale).

use super::*;

/// Wire form of the `Abc` metric.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Abc {
    /// Sum of assignments across the space.
    pub assignments: u64,
    /// Sum of branches across the space.
    pub branches: u64,
    /// Sum of conditions across the space.
    pub conditions: u64,
    /// Euclidean ABC magnitude across the space (built from the summed
    /// assignment/branch/condition accumulators).
    #[serde(default = "nan_default", with = "non_finite")]
    pub magnitude: f64,
    /// This space's own ABC magnitude — `sqrt(A² + B² + C²)` over just
    /// this space's assignments/branches/conditions, excluding nested
    /// function/closure spaces. Equals [`magnitude`](Self::magnitude)
    /// only at a leaf space; it is the per-space scalar the CLI
    /// thresholds `abc` against (#958). Absent pre-#958 JSON
    /// deserializes to `NaN` via `nan_default`.
    #[serde(default = "nan_default", with = "non_finite")]
    pub value: f64,
    /// Average assignments per space.
    #[serde(default = "nan_default", with = "non_finite")]
    pub assignments_average: f64,
    /// Average branches per space.
    #[serde(default = "nan_default", with = "non_finite")]
    pub branches_average: f64,
    /// Average conditions per space.
    #[serde(default = "nan_default", with = "non_finite")]
    pub conditions_average: f64,
    /// Minimum assignments in a single space.
    pub assignments_min: u64,
    /// Maximum assignments in a single space.
    pub assignments_max: u64,
    /// Minimum branches in a single space.
    pub branches_min: u64,
    /// Maximum branches in a single space.
    pub branches_max: u64,
    /// Minimum conditions in a single space.
    pub conditions_min: u64,
    /// Maximum conditions in a single space.
    pub conditions_max: u64,
}

impl From<&abc::Stats> for Abc {
    fn from(s: &abc::Stats) -> Self {
        Self {
            assignments: s.assignments_sum(),
            branches: s.branches_sum(),
            conditions: s.conditions_sum(),
            magnitude: s.magnitude_sum(),
            value: s.magnitude(),
            assignments_average: s.assignments_average(),
            branches_average: s.branches_average(),
            conditions_average: s.conditions_average(),
            assignments_min: s.assignments_min(),
            assignments_max: s.assignments_max(),
            branches_min: s.branches_min(),
            branches_max: s.branches_max(),
            conditions_min: s.conditions_min(),
            conditions_max: s.conditions_max(),
        }
    }
}

/// Wire form of the `Cognitive` metric.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Cognitive {
    /// Cognitive-complexity sum across the space.
    pub sum: u64,
    /// This space's own cognitive complexity, excluding nested
    /// function/closure spaces. Equals [`sum`](Self::sum) only at a leaf
    /// space; at an interior space the sum rolls up descendants while
    /// this stays the per-space scalar the CLI thresholds against
    /// (#958). `#[serde(default)]` so pre-#958 JSON (which lacks the
    /// field) still deserializes — e.g. when `bca diff` reads an older
    /// metrics file.
    #[serde(default)]
    pub value: u64,
    /// Average cognitive complexity per function.
    #[serde(default = "nan_default", with = "non_finite")]
    pub average: f64,
    /// Minimum cognitive complexity in a single function.
    pub min: u64,
    /// Maximum cognitive complexity in a single function.
    pub max: u64,
}

impl From<&cognitive::Stats> for Cognitive {
    fn from(s: &cognitive::Stats) -> Self {
        Self {
            sum: s.cognitive_sum(),
            value: s.cognitive(),
            average: s.cognitive_average(),
            min: s.cognitive_min(),
            max: s.cognitive_max(),
        }
    }
}

/// Wire form of the modified-cyclomatic sub-record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CyclomaticModified {
    /// Modified-cyclomatic sum across the space.
    pub sum: u64,
    /// This space's own modified-cyclomatic complexity, excluding nested
    /// function/closure spaces (see [`Cyclomatic::value`]).
    #[serde(default)]
    pub value: u64,
    /// Average modified cyclomatic complexity per function.
    #[serde(default = "nan_default", with = "non_finite")]
    pub average: f64,
    /// Minimum modified cyclomatic complexity in a single function.
    pub min: u64,
    /// Maximum modified cyclomatic complexity in a single function.
    pub max: u64,
}

/// Wire form of the `Cyclomatic` metric.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Cyclomatic {
    /// Cyclomatic-complexity sum across the space.
    pub sum: u64,
    /// This space's own cyclomatic complexity, excluding nested
    /// function/closure spaces. Equals [`sum`](Self::sum) only at a leaf
    /// space; at an interior space the sum rolls up descendants while
    /// this stays the per-space scalar the CLI thresholds against
    /// (#958). `#[serde(default)]` so pre-#958 JSON still deserializes.
    #[serde(default)]
    pub value: u64,
    /// Average cyclomatic complexity per function.
    #[serde(default = "nan_default", with = "non_finite")]
    pub average: f64,
    /// Minimum cyclomatic complexity in a single function.
    pub min: u64,
    /// Maximum cyclomatic complexity in a single function.
    pub max: u64,
    /// The modified-cyclomatic projection.
    pub modified: CyclomaticModified,
}

impl From<&cyclomatic::Stats> for Cyclomatic {
    fn from(s: &cyclomatic::Stats) -> Self {
        Self {
            sum: s.cyclomatic_sum(),
            value: s.cyclomatic(),
            average: s.cyclomatic_average(),
            min: s.cyclomatic_min(),
            max: s.cyclomatic_max(),
            modified: CyclomaticModified {
                sum: s.cyclomatic_modified_sum(),
                value: s.cyclomatic_modified(),
                average: s.cyclomatic_modified_average(),
                min: s.cyclomatic_modified_min(),
                max: s.cyclomatic_modified_max(),
            },
        }
    }
}

/// Wire form of the `Nexits` (exit-points) metric.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Nexits {
    /// Exit-point sum across the space.
    pub sum: u64,
    /// Average exit points per function.
    #[serde(default = "nan_default", with = "non_finite")]
    pub average: f64,
    /// Minimum exit points in a single function.
    pub min: u64,
    /// Maximum exit points in a single function.
    pub max: u64,
}

impl From<&nexits::Stats> for Nexits {
    fn from(s: &nexits::Stats) -> Self {
        Self {
            sum: s.nexits_sum(),
            average: s.nexits_average(),
            min: s.nexits_min(),
            max: s.nexits_max(),
        }
    }
}

/// Wire form of the `Halstead` metric suite.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Halstead {
    /// Number of distinct operators (`n1`).
    pub unique_operators: u64,
    /// Total operator occurrences (`N1`).
    pub total_operators: u64,
    /// Number of distinct operands (`n2`).
    pub unique_operands: u64,
    /// Total operand occurrences (`N2`).
    pub total_operands: u64,
    /// Program length (`N = N1 + N2`).
    pub length: u64,
    /// Estimated program length.
    #[serde(default = "nan_default", with = "non_finite")]
    pub estimated_program_length: f64,
    /// Purity ratio (estimated length / length).
    #[serde(default = "nan_default", with = "non_finite")]
    pub purity_ratio: f64,
    /// Program vocabulary (`n = n1 + n2`).
    pub vocabulary: u64,
    /// Program volume.
    #[serde(default = "nan_default", with = "non_finite")]
    pub volume: f64,
    /// Difficulty.
    #[serde(default = "nan_default", with = "non_finite")]
    pub difficulty: f64,
    /// Program level (inverse difficulty).
    #[serde(default = "nan_default", with = "non_finite")]
    pub level: f64,
    /// Effort.
    #[serde(default = "nan_default", with = "non_finite")]
    pub effort: f64,
    /// Estimated time to program (seconds).
    #[serde(default = "nan_default", with = "non_finite")]
    pub time: f64,
    /// Estimated number of delivered bugs.
    #[serde(default = "nan_default", with = "non_finite")]
    pub bugs: f64,
}

impl From<&halstead::Stats> for Halstead {
    fn from(s: &halstead::Stats) -> Self {
        Self {
            unique_operators: s.unique_operators(),
            total_operators: s.total_operators(),
            unique_operands: s.unique_operands(),
            total_operands: s.total_operands(),
            length: s.length(),
            estimated_program_length: s.estimated_program_length(),
            purity_ratio: s.purity_ratio(),
            vocabulary: s.vocabulary(),
            volume: s.volume(),
            difficulty: s.difficulty(),
            level: s.level(),
            effort: s.effort(),
            time: s.time(),
            bugs: s.bugs(),
        }
    }
}

/// Wire form of the `Loc` metric suite.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Loc {
    /// Source lines of code.
    pub sloc: u64,
    /// Physical lines of code.
    pub ploc: u64,
    /// Logical lines of code.
    pub lloc: u64,
    /// Comment lines of code.
    pub cloc: u64,
    /// Blank lines.
    pub blank: u64,
    /// Average SLOC per space.
    #[serde(default = "nan_default", with = "non_finite")]
    pub sloc_average: f64,
    /// Average PLOC per space.
    #[serde(default = "nan_default", with = "non_finite")]
    pub ploc_average: f64,
    /// Average LLOC per space.
    #[serde(default = "nan_default", with = "non_finite")]
    pub lloc_average: f64,
    /// Average CLOC per space.
    #[serde(default = "nan_default", with = "non_finite")]
    pub cloc_average: f64,
    /// Average blank lines per space.
    #[serde(default = "nan_default", with = "non_finite")]
    pub blank_average: f64,
    /// Minimum SLOC in a single space.
    pub sloc_min: u64,
    /// Maximum SLOC in a single space.
    pub sloc_max: u64,
    /// Minimum CLOC in a single space.
    pub cloc_min: u64,
    /// Maximum CLOC in a single space.
    pub cloc_max: u64,
    /// Minimum PLOC in a single space.
    pub ploc_min: u64,
    /// Maximum PLOC in a single space.
    pub ploc_max: u64,
    /// Minimum LLOC in a single space.
    pub lloc_min: u64,
    /// Maximum LLOC in a single space.
    pub lloc_max: u64,
    /// Minimum blank lines in a single space.
    pub blank_min: u64,
    /// Maximum blank lines in a single space.
    pub blank_max: u64,
}

impl From<&loc::Stats> for Loc {
    fn from(s: &loc::Stats) -> Self {
        Self {
            sloc: s.sloc(),
            ploc: s.ploc(),
            lloc: s.lloc(),
            cloc: s.cloc(),
            blank: s.blank(),
            sloc_average: s.sloc_average(),
            ploc_average: s.ploc_average(),
            lloc_average: s.lloc_average(),
            cloc_average: s.cloc_average(),
            blank_average: s.blank_average(),
            sloc_min: s.sloc_min(),
            sloc_max: s.sloc_max(),
            cloc_min: s.cloc_min(),
            cloc_max: s.cloc_max(),
            ploc_min: s.ploc_min(),
            ploc_max: s.ploc_max(),
            lloc_min: s.lloc_min(),
            lloc_max: s.lloc_max(),
            blank_min: s.blank_min(),
            blank_max: s.blank_max(),
        }
    }
}

/// Wire form of the `Mi` (maintainability index) metric.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Mi {
    /// Original maintainability index.
    #[serde(default = "nan_default", with = "non_finite")]
    pub original: f64,
    /// SEI-derivative maintainability index.
    #[serde(default = "nan_default", with = "non_finite")]
    pub sei: f64,
    /// Visual Studio-derivative maintainability index.
    #[serde(default = "nan_default", with = "non_finite")]
    pub visual_studio: f64,
}

impl From<&mi::Stats> for Mi {
    fn from(s: &mi::Stats) -> Self {
        Self {
            original: s.original(),
            sei: s.sei(),
            visual_studio: s.visual_studio(),
        }
    }
}

/// Wire form of the `NArgs` metric.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Nargs {
    /// Sum of function arguments.
    pub function_args: u64,
    /// Sum of closure arguments.
    pub closure_args: u64,
    /// Average function arguments per function.
    #[serde(default = "nan_default", with = "non_finite")]
    pub function_args_average: f64,
    /// Average closure arguments per closure.
    #[serde(default = "nan_default", with = "non_finite")]
    pub closure_args_average: f64,
    /// Total arguments (functions + closures).
    pub total: u64,
    /// Average arguments per function/closure.
    #[serde(default = "nan_default", with = "non_finite")]
    pub average: f64,
    /// Minimum function arguments in a single function.
    pub function_args_min: u64,
    /// Maximum function arguments in a single function.
    pub function_args_max: u64,
    /// Minimum closure arguments in a single closure.
    pub closure_args_min: u64,
    /// Maximum closure arguments in a single closure.
    pub closure_args_max: u64,
}

impl From<&nargs::Stats> for Nargs {
    fn from(s: &nargs::Stats) -> Self {
        Self {
            function_args: s.function_args_sum(),
            closure_args: s.closure_args_sum(),
            function_args_average: s.function_args_average(),
            closure_args_average: s.closure_args_average(),
            total: s.total(),
            average: s.average(),
            function_args_min: s.function_args_min(),
            function_args_max: s.function_args_max(),
            closure_args_min: s.closure_args_min(),
            closure_args_max: s.closure_args_max(),
        }
    }
}

/// Wire form of the `Nom` (number-of-methods) metric.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Nom {
    /// Sum of function definitions.
    pub functions: u64,
    /// Sum of closures.
    pub closures: u64,
    /// Average function definitions per space.
    #[serde(default = "nan_default", with = "non_finite")]
    pub functions_average: f64,
    /// Average closures per space.
    #[serde(default = "nan_default", with = "non_finite")]
    pub closures_average: f64,
    /// Total functions + closures.
    pub total: u64,
    /// Average functions + closures per space.
    #[serde(default = "nan_default", with = "non_finite")]
    pub average: f64,
    /// Minimum function definitions in a single space.
    pub functions_min: u64,
    /// Maximum function definitions in a single space.
    pub functions_max: u64,
    /// Minimum closures in a single space.
    pub closures_min: u64,
    /// Maximum closures in a single space.
    pub closures_max: u64,
}

impl From<&nom::Stats> for Nom {
    fn from(s: &nom::Stats) -> Self {
        Self {
            functions: s.functions_sum(),
            closures: s.closures_sum(),
            functions_average: s.functions_average(),
            closures_average: s.closures_average(),
            total: s.total(),
            average: s.average(),
            functions_min: s.functions_min(),
            functions_max: s.functions_max(),
            closures_min: s.closures_min(),
            closures_max: s.closures_max(),
        }
    }
}

/// Wire form of the `Npa` (number-of-public-attributes) metric.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Npa {
    /// Sum of public attributes across classes.
    pub class_npa_sum: u64,
    /// Sum of public attributes across interfaces.
    pub interface_npa_sum: u64,
    /// Sum of all class attributes.
    pub class_attributes: u64,
    /// Sum of all interface attributes.
    pub interface_attributes: u64,
    /// Class data accessibility ratio.
    #[serde(default = "nan_default", with = "non_finite")]
    pub class_cda: f64,
    /// Interface data accessibility ratio.
    #[serde(default = "nan_default", with = "non_finite")]
    pub interface_cda: f64,
    /// Total public attributes.
    pub total: u64,
    /// Total attributes.
    pub total_attributes: u64,
    /// Overall data accessibility ratio.
    #[serde(default = "nan_default", with = "non_finite")]
    pub cda: f64,
}

impl From<&npa::Stats> for Npa {
    fn from(s: &npa::Stats) -> Self {
        Self {
            class_npa_sum: s.class_npa_sum(),
            interface_npa_sum: s.interface_npa_sum(),
            class_attributes: s.class_na_sum(),
            interface_attributes: s.interface_na_sum(),
            class_cda: s.class_cda(),
            interface_cda: s.interface_cda(),
            total: s.total_npa(),
            total_attributes: s.total_na(),
            cda: s.total_cda(),
        }
    }
}

/// Wire form of the `Npm` (number-of-public-methods) metric.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Npm {
    /// Sum of public methods across classes.
    pub class_npm_sum: u64,
    /// Sum of public methods across interfaces.
    pub interface_npm_sum: u64,
    /// Sum of all class methods.
    pub class_methods: u64,
    /// Sum of all interface methods.
    pub interface_methods: u64,
    /// Class operation accessibility ratio.
    #[serde(default = "nan_default", with = "non_finite")]
    pub class_coa: f64,
    /// Interface operation accessibility ratio.
    #[serde(default = "nan_default", with = "non_finite")]
    pub interface_coa: f64,
    /// Total public methods.
    pub total: u64,
    /// Total methods.
    pub total_methods: u64,
    /// Overall operation accessibility ratio.
    #[serde(default = "nan_default", with = "non_finite")]
    pub coa: f64,
}

impl From<&npm::Stats> for Npm {
    fn from(s: &npm::Stats) -> Self {
        Self {
            class_npm_sum: s.class_npm_sum(),
            interface_npm_sum: s.interface_npm_sum(),
            class_methods: s.class_nm_sum(),
            interface_methods: s.interface_nm_sum(),
            class_coa: s.class_coa(),
            interface_coa: s.interface_coa(),
            total: s.total_npm(),
            total_methods: s.total_nm(),
            coa: s.total_coa(),
        }
    }
}

/// Wire form of the `Tokens` metric.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Tokens {
    /// Token-count sum across the space.
    pub tokens: u64,
    /// Average tokens per space.
    #[serde(default = "nan_default", with = "non_finite")]
    pub average: f64,
    /// Minimum tokens in a single space.
    pub min: u64,
    /// Maximum tokens in a single space.
    pub max: u64,
}

impl From<&tokens::Stats> for Tokens {
    fn from(s: &tokens::Stats) -> Self {
        Self {
            tokens: s.tokens_sum(),
            average: s.tokens_average(),
            min: s.tokens_min(),
            max: s.tokens_max(),
        }
    }
}

/// Wire form of the `Wmc` (weighted-methods-per-class) metric.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Wmc {
    /// Sum of weighted methods across classes.
    pub class_wmc_sum: u64,
    /// Sum of weighted methods across interfaces.
    pub interface_wmc_sum: u64,
    /// Total weighted methods.
    pub total: u64,
}

impl From<&wmc::Stats> for Wmc {
    fn from(s: &wmc::Stats) -> Self {
        Self {
            class_wmc_sum: s.class_wmc_sum(),
            interface_wmc_sum: s.interface_wmc_sum(),
            total: s.total_wmc(),
        }
    }
}
