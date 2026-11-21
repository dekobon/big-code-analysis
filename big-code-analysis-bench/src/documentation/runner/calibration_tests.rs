use super::*;

pub(crate) fn target_trace(iterations: u32) -> Calibration {
    assert!(iterations.is_power_of_two() && iterations <= 1 << 20);
    Calibration::Batches {
        stages: (0..=iterations.ilog2())
            .map(|at| CalibrationStage {
                iterations: 1 << at,
                samples: vec![
                    if 1 << at == iterations {
                        Duration::from_millis(2)
                    } else {
                        Duration::ZERO
                    };
                    3
                ],
            })
            .collect(),
        outcome: CalibrationOutcome::TargetReached,
    }
}

fn stages_mut(evidence: &mut Calibration) -> &mut Vec<CalibrationStage> {
    let Calibration::Batches { stages, .. } = evidence else {
        panic!("expected batch evidence")
    };
    stages
}

fn outcome_mut(evidence: &mut Calibration) -> &mut CalibrationOutcome {
    let Calibration::Batches { outcome, .. } = evidence else {
        panic!("expected batch evidence")
    };
    outcome
}

#[test]
fn slow_first_call_does_not_freeze_or_select_the_fastest_sample() {
    let durations = [9_000_000, 10, 20, 0, 100, 200, 2_000_001, 0, 2_000_000];
    let mut calls = Vec::new();
    let mut evidence = Calibration::new(Mode::Gate);
    let iterations = calibrate(&mut evidence, "ladder", 8, MAX_CELL_WALK, |count| {
        let elapsed = Duration::from_nanos(durations[calls.len()]);
        calls.push(count);
        Ok(elapsed)
    })
    .unwrap();
    assert_eq!(iterations, 4);
    assert_eq!(calls, [1, 1, 1, 2, 2, 2, 4, 4, 4]);
    let Calibration::Batches { stages, outcome } = &evidence else {
        panic!("expected ladder")
    };
    assert_eq!(*outcome, CalibrationOutcome::TargetReached);
    assert_eq!(
        stages
            .iter()
            .flat_map(|s| &s.samples)
            .map(Duration::as_nanos)
            .collect::<Vec<_>>(),
        durations.map(u128::from)
    );
    evidence
        .validate(Mode::Gate, iterations, MAX_CELL_WALK)
        .unwrap();
}

#[test]
fn first_qualified_stage_stops_after_exactly_three_samples() {
    let mut calls = 0;
    let mut evidence = Calibration::new(Mode::ReportOnly);
    let iterations = calibrate(&mut evidence, "first", 1, MAX_CELL_WALK, |count| {
        calls += 1;
        assert_eq!(count, 1);
        assert!(calls <= 3, "no next stage or extra sample");
        Ok(Duration::from_millis(2))
    })
    .unwrap();
    assert_eq!(calls, 3);
    assert_eq!(iterations, 1);
    assert_eq!(evidence, target_trace(1));
}

#[test]
fn cap_outcomes_use_the_three_sample_median_and_exact_boundaries() {
    for (ns, outcome) in [
        (199_999, CalibrationOutcome::Unresolved),
        (200_000, CalibrationOutcome::CapLimited),
        (1_999_999, CalibrationOutcome::CapLimited),
        (2_000_000, CalibrationOutcome::TargetReached),
    ] {
        let mut evidence = Calibration::new(Mode::Gate);
        let mut calls = Vec::new();
        let result = calibrate(&mut evidence, "cap", 2, MAX_CELL_WALK, |count| {
            calls.push(count);
            Ok(Duration::from_nanos(if count == 1 << 20 { ns } else { 0 }))
        });
        assert_eq!(calls.len(), 63);
        for (at, samples) in calls.chunks_exact(3).enumerate() {
            assert_eq!(samples, [1 << at; 3]);
        }
        assert_eq!(*outcome_mut(&mut evidence), outcome);
        if outcome == CalibrationOutcome::Unresolved {
            assert!(result.unwrap_err().0.contains("CalibrationUnresolved"));
            assert!(
                evidence
                    .validate(Mode::Gate, 1 << 20, MAX_CELL_WALK)
                    .is_err()
            );
        } else {
            assert_eq!(result.unwrap(), 1 << 20);
            evidence
                .validate(Mode::Gate, 1 << 20, MAX_CELL_WALK)
                .unwrap();
        }
    }
}

#[test]
fn real_calibration_batches_execute_three_times_each_tested_count() {
    let probe = &super::super::REGISTRY[1];
    let parsed = Parsed::new(probe, 4).unwrap();
    let index = parsed.collect(probe).unwrap();
    let operation = Prepared::new(probe, 4, &parsed, &index).unwrap();
    let before = big_code_analysis_ast::documentation::observations();
    let mut evidence = Calibration::new(Mode::Gate);
    let mut completed = Vec::new();
    let count = calibrate(&mut evidence, probe.id, 4, MAX_CELL_WALK, |count| {
        let before = big_code_analysis_ast::documentation::observations();
        batch(&operation, count)?;
        let after = big_code_analysis_ast::documentation::observations();
        assert_eq!(after.renders - before.renders, 4 * count as usize);
        assert_eq!(
            after.visits - before.visits,
            parsed.nodes().len() * count as usize
        );
        completed.push(count);
        Ok(if count == 4 {
            Duration::from_millis(2)
        } else {
            Duration::ZERO
        })
    })
    .unwrap();
    let after = big_code_analysis_ast::documentation::observations();
    assert_eq!(count, 4);
    assert_eq!(completed, [1, 1, 1, 2, 2, 2, 4, 4, 4]);
    // expected: four documents for each of 3*(1+2+4) real executions.
    assert_eq!(after.renders - before.renders, 84);
    assert_eq!(after.visits - before.visits, 21 * parsed.nodes().len());
}

#[test]
fn trace_validation_rejects_forged_decisions_and_count_disagreement() {
    for mode in [Mode::Gate, Mode::ReportOnly] {
        let valid = target_trace(4);
        valid.validate(mode, 4, MAX_CELL_WALK).unwrap();
        for mutation in 0..16 {
            let mut invalid = valid.clone();
            let mut iterations = 4;
            match mutation {
                0 => {
                    stages_mut(&mut invalid).remove(1);
                }
                1 => stages_mut(&mut invalid)[0].iterations = 0,
                2 => stages_mut(&mut invalid)[1].iterations = 3,
                3 => stages_mut(&mut invalid)[1].iterations = 1,
                4 => stages_mut(&mut invalid)[2].iterations = u32::MAX,
                5 => {
                    stages_mut(&mut invalid)[2].samples.pop();
                }
                6 => stages_mut(&mut invalid)[0].samples.push(Duration::ZERO),
                7 => stages_mut(&mut invalid)[1].samples = vec![Duration::from_millis(2); 3],
                8 => stages_mut(&mut invalid)[2].samples = vec![Duration::from_nanos(1_999_999); 3],
                9 => *outcome_mut(&mut invalid) = CalibrationOutcome::CapLimited,
                10 => *outcome_mut(&mut invalid) = CalibrationOutcome::Pending,
                11 => iterations = 2,
                12 => stages_mut(&mut invalid).clear(),
                13 => stages_mut(&mut invalid).push(CalibrationStage {
                    iterations: 8,
                    samples: vec![Duration::from_millis(2); 3],
                }),
                14 => stages_mut(&mut invalid).swap(0, 1),
                _ => {
                    invalid = Calibration::Smoke {
                        elapsed: Some(Duration::ZERO),
                    }
                }
            }
            assert!(
                invalid.validate(mode, iterations, MAX_CELL_WALK).is_err(),
                "mutation {mutation}, {mode:?}"
            );
        }
        let mut too_long = target_trace(1 << 20);
        stages_mut(&mut too_long).push(CalibrationStage {
            iterations: 1 << 21,
            samples: vec![Duration::ZERO; 3],
        });
        assert!(too_long.validate(mode, 1 << 21, MAX_CELL_WALK).is_err());
        for ns in [0, 199_999, 2_000_000] {
            let mut invalid = target_trace(1 << 20);
            *outcome_mut(&mut invalid) = CalibrationOutcome::CapLimited;
            stages_mut(&mut invalid).last_mut().unwrap().samples =
                vec![Duration::from_nanos(ns); 3];
            assert!(
                invalid.validate(mode, 1 << 20, MAX_CELL_WALK).is_err(),
                "cap median {ns}"
            );
        }
    }
}

#[test]
fn custom_budget_is_checked_for_every_observation_before_the_next_call() {
    // The median qualifies, but a single outlier still exceeds the exact budget.
    let mut trace = target_trace(1);
    stages_mut(&mut trace)[0].samples[1] = Duration::from_nanos(2_000_001);
    assert!(
        trace
            .validate(Mode::Gate, 1, Duration::from_millis(2))
            .is_err()
    );
    trace
        .validate(Mode::Gate, 1, Duration::from_nanos(2_000_001))
        .unwrap();

    for failing_call in 0..6 {
        let mut evidence = Calibration::new(Mode::Gate);
        let mut calls = 0;
        let error = calibrate(
            &mut evidence,
            "budget",
            9,
            Duration::from_nanos(10),
            |count| {
                let ns = if calls == failing_call {
                    u64::from(count) * 10 + 1
                } else {
                    0
                };
                calls += 1;
                Ok(Duration::from_nanos(ns))
            },
        )
        .unwrap_err();
        assert_eq!(calls, failing_call + 1);
        assert!(error.0.contains("AbandonedCell"));
        assert!(error.0.contains(&format!(
            "stage {} count {} sample {}",
            failing_call / 3,
            1 << (failing_call / 3),
            failing_call % 3
        )));
        assert_eq!(
            *outcome_mut(&mut evidence),
            CalibrationOutcome::BudgetExceeded
        );
        let stages = stages_mut(&mut evidence);
        assert_eq!(stages.len(), failing_call / 3 + 1);
        assert_eq!(stages.last().unwrap().samples.len(), failing_call % 3 + 1);
        assert_eq!(
            stages.last().unwrap().samples.last().unwrap().as_nanos(),
            u128::from(stages.last().unwrap().iterations) * 10 + 1
        );
    }
    check_budget(
        "boundary",
        1,
        Duration::from_nanos(20),
        2,
        Duration::from_nanos(10),
    )
    .unwrap();
    assert!(
        check_budget(
            "boundary",
            1,
            Duration::from_nanos(21),
            2,
            Duration::from_nanos(10)
        )
        .is_err()
    );
    check_budget("largest", 1, Duration::MAX, u32::MAX, Duration::MAX).unwrap();
}

fn pending_cells() -> Vec<RawCell> {
    ["complete", "failing", "must-not-run"]
        .into_iter()
        .map(|id| RawCell {
            counts: SourceCounts::default(),
            id,
            size: 8,
            bytes: 8,
            segments: 1,
            iterations: 1,
            calibration: Calibration::new(Mode::Gate),
            budget: Duration::from_millis(3),
            batches: Vec::new(),
        })
        .collect()
}

#[test]
fn failures_preserve_previous_cells_stages_and_exact_observed_prefix() {
    for budget_failure in [false, true] {
        for failing_sample in 0..3 {
            let mut raw = pending_cells();
            let mut calls = [0; 3];
            let error = calibrate_cells(&mut raw, |at, count| {
                calls[at] += 1;
                if at == 0 {
                    return Ok(Duration::from_millis(2));
                }
                if count == 2 && calls[at] == 4 + failing_sample {
                    return if budget_failure {
                        Ok(Duration::from_nanos(6_000_001))
                    } else {
                        Err(HarnessError("primary-operation-failure".into()))
                    };
                }
                Ok(Duration::from_nanos(17))
            })
            .unwrap_err();
            assert_eq!(calls, [3, 4 + failing_sample, 0]);
            assert_eq!(raw[0].calibration, target_trace(1));
            let Calibration::Batches { stages, outcome } = &raw[1].calibration else {
                panic!("expected trace")
            };
            assert_eq!(
                *outcome,
                if budget_failure {
                    CalibrationOutcome::BudgetExceeded
                } else {
                    CalibrationOutcome::OperationFailed
                }
            );
            assert_eq!(stages.len(), 2);
            assert_eq!(stages[0].samples, [Duration::from_nanos(17); 3]);
            assert_eq!(
                stages[1].samples.len(),
                failing_sample + usize::from(budget_failure)
            );
            let primary = if budget_failure {
                "AbandonedCell"
            } else {
                "primary-operation-failure"
            };
            assert!(error.0.lines().next().unwrap().contains(primary), "{error}");
            assert!(
                error
                    .0
                    .contains(&format!("stage 1 count 2 sample {failing_sample}")),
                "{error}"
            );
            assert!(error.0.contains("complete size 8"), "{error}");
            assert!(!error.0.contains("must-not-run"), "{error}");
            let mut expected = "    calibration-stage-ns: 1 2".to_owned();
            for _ in 0..failing_sample {
                expected.push_str(" 17");
            }
            if budget_failure {
                expected.push_str(" 6000001");
            }
            assert!(error.0.lines().any(|line| line == expected), "{error}");
        }
    }
}

#[test]
fn first_operation_failure_retains_an_empty_stage_and_stops() {
    let mut raw = pending_cells();
    let mut calls = Vec::new();
    let error = calibrate_cells(&mut raw, |at, count| {
        calls.push((at, count));
        Err(HarnessError("first-operation-failed".into()))
    })
    .unwrap_err();
    assert_eq!(calls, [(0, 1)]);
    assert_eq!(
        raw[0].calibration,
        Calibration::Batches {
            stages: vec![CalibrationStage {
                iterations: 1,
                samples: Vec::new()
            }],
            outcome: CalibrationOutcome::OperationFailed,
        }
    );
    assert!(
        error
            .0
            .lines()
            .next()
            .unwrap()
            .contains("calibration stage 0 count 1 sample 0: first-operation-failed"),
        "{error}"
    );
    assert!(
        error
            .0
            .lines()
            .any(|line| line == "    calibration-stage-ns: 0 1"),
        "{error}"
    );
    assert!(!error.0.contains("failing size"), "{error}");
}

#[test]
fn smoke_records_one_setup_and_never_claims_calibration() {
    let mut evidence = Calibration::new(Mode::Smoke);
    let mut calls = Vec::new();
    assert_eq!(
        calibrate(&mut evidence, "smoke", 1, Duration::ZERO, |count| {
            calls.push(count);
            Ok(Duration::ZERO)
        })
        .unwrap(),
        1
    );
    assert_eq!(calls, [1]);
    assert_eq!(
        evidence,
        Calibration::Smoke {
            elapsed: Some(Duration::ZERO)
        }
    );
    evidence.validate(Mode::Smoke, 1, Duration::ZERO).unwrap();
    assert!(evidence.validate(Mode::Smoke, 2, MAX_CELL_WALK).is_err());
    assert!(
        Calibration::new(Mode::Smoke)
            .validate(Mode::Smoke, 1, MAX_CELL_WALK)
            .is_err()
    );
    assert!(
        target_trace(1)
            .validate(Mode::Smoke, 1, MAX_CELL_WALK)
            .is_err()
    );
    let mut text = String::new();
    write_calibration(&mut text, &evidence, Duration::ZERO).unwrap();
    assert_eq!(
        text,
        "    per-operation-budget-ns: 0\n    smoke-preparation-ns: 0; no calibration or performance claim\n"
    );
}

#[test]
fn smoke_setup_failure_has_no_fabricated_elapsed_sample() {
    let mut raw = pending_cells();
    for cell in &mut raw {
        cell.calibration = Calibration::new(Mode::Smoke);
    }
    let mut calls = Vec::new();
    let error = calibrate_cells(&mut raw, |at, count| {
        calls.push((at, count));
        if at == 0 {
            Ok(Duration::from_nanos(19))
        } else {
            Err(HarnessError("setup-failed".into()))
        }
    })
    .unwrap_err();
    assert_eq!(calls, [(0, 1), (1, 1)]);
    assert_eq!(
        raw[0].calibration,
        Calibration::Smoke {
            elapsed: Some(Duration::from_nanos(19))
        }
    );
    assert_eq!(raw[1].calibration, Calibration::Smoke { elapsed: None });
    assert!(
        error
            .0
            .lines()
            .next()
            .unwrap()
            .contains("failing size 8: smoke preparation count 1 sample 0: setup-failed"),
        "{error}"
    );
    assert!(error.0.contains("smoke-preparation-ns: 19;"), "{error}");
    assert!(error.0.contains("smoke-preparation-ns:;"), "{error}");
    assert!(!error.0.contains("must-not-run"), "{error}");
}

#[test]
fn calibration_formatter_preserves_sample_order_and_integer_nanoseconds() {
    let trace = Calibration::Batches {
        stages: vec![CalibrationStage {
            iterations: 1,
            samples: vec![
                Duration::from_nanos(2_000_001),
                Duration::ZERO,
                Duration::from_nanos(2_000_003),
            ],
        }],
        outcome: CalibrationOutcome::TargetReached,
    };
    let mut text = String::new();
    write_calibration(&mut text, &trace, Duration::from_nanos(3_000_007)).unwrap();
    assert_eq!(
        text,
        concat!(
            "    per-operation-budget-ns: 3000007\n",
            "    calibration-stage-ns: 0 1 2000001 0 2000003\n",
            "    calibration-final-ns: 0 1 2000001 0 2000003 target-reached\n"
        )
    );
}
