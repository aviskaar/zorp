//! Hypothesis search over condition atoms, the structured-genome version
//! the 2026-08-19 design left Proposed and the 2026-08-28 design admits
//! to Gated.
//!
//! A hypothesis is a set of claims, each "this condition atom is
//! implicated in admitted anomalies of this direction". The genome is a
//! bitmask over (atom, direction) pairs, fitness is arithmetic against
//! rows the caller supplies, and the parsimony weight lambda is swept
//! the way `families` sweeps theta: only claim sets stable across a
//! contiguous band are reported, with the band recorded.
//!
//! **What is claimed, and what is not.** The search runs on erbga's
//! generic loop through the `Problem` trait. The four erbga benchmarks
//! certify the graph path and nothing here: this module's validation is
//! its planted-structure tests, which certify it on synthetic ledgers
//! only. Running it against the real anomaly ledger is behind the
//! admission gate in `docs/DECISIONS.md` (2026-08-28): at least 12
//! admitted anomalies spanning at least 3 distinct condition keys, and a
//! person's recorded decision. No function in this module reads a
//! database, and no adapter to the real ledger exists.
//!
//! **Integrity.** The input type holds condition atoms and a direction
//! and nothing else. There is no field a model-authored sentence could
//! travel in, so integrity rule 5 holds by construction rather than by
//! filtering.

use crate::families::Direction;
use erbga::Chromosome;

/// One experiment, reduced to what a hypothesis may be scored against.
///
/// Condition atoms as recorded, and whether the experiment produced an
/// admitted anomaly. Held deliberately narrow: see the module header.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExperimentRow {
    /// Condition key and value pairs.
    pub conditions: Vec<(String, String)>,
    /// `Some` if the experiment produced an admitted anomaly, with the
    /// deviation's direction. `None` for an unremarkable run.
    pub outcome: Option<Direction>,
}

/// One claim: this condition atom is implicated in admitted anomalies
/// of this direction.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Claim {
    pub key: String,
    pub value: String,
    pub direction: Direction,
}

/// The distinct condition atoms across a set of rows, sorted, which is
/// what makes genome bit positions stable and a decode deterministic.
///
/// Atoms, not bare keys: a real ledger records the same keys on every
/// row with differing values, so the key set separates nothing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Vocabulary {
    atoms: Vec<(String, String)>,
}

impl Vocabulary {
    pub fn from_rows(rows: &[ExperimentRow]) -> Self {
        let mut atoms: Vec<(String, String)> = rows
            .iter()
            .flat_map(|r| r.conditions.iter().cloned())
            .collect();
        atoms.sort();
        atoms.dedup();
        Vocabulary { atoms }
    }

    pub fn len(&self) -> usize {
        self.atoms.len()
    }

    pub fn is_empty(&self) -> bool {
        self.atoms.is_empty()
    }

    /// Two bits per atom, one per direction.
    pub fn genome_len(&self) -> usize {
        self.atoms.len() * 2
    }

    /// The bit claiming `direction` for the atom at `index`.
    fn bit(index: usize, direction: Direction) -> usize {
        index * 2
            + match direction {
                Direction::Below => 0,
                Direction::Above => 1,
            }
    }

    /// The set bits, as sorted claims.
    pub fn decode(&self, chromosome: &Chromosome) -> Vec<Claim> {
        let mut claims = Vec::new();
        for (index, (key, value)) in self.atoms.iter().enumerate() {
            for direction in [Direction::Below, Direction::Above] {
                if chromosome.get(Self::bit(index, direction)) {
                    claims.push(Claim {
                        key: key.clone(),
                        value: value.clone(),
                        direction,
                    });
                }
            }
        }
        claims.sort();
        claims
    }
}

/// How well a hypothesis fits the rows, in arithmetic a reader can
/// recompute by hand.
///
/// A claim (atom, direction) flags every row carrying that atom, and
/// predicts its deviation runs that way. A row is covered correctly when
/// it produced an admitted anomaly and some claim on one of its atoms
/// matches the direction. A flagged row that is not correctly covered is
/// a wrong flag, whether it was unremarkable or deviated the other way.
///
/// score = (correct - wrong - lambda * set bits) / rows
///
/// Normalizing by row count keeps lambda's meaning stable across ledger
/// sizes. The exact form is certified by the planted-structure tests,
/// not the other way around.
pub(crate) fn score(
    rows: &[ExperimentRow],
    vocabulary: &Vocabulary,
    chromosome: &Chromosome,
    lambda: f64,
) -> f64 {
    if rows.is_empty() {
        return 0.0;
    }
    let mut correct = 0i64;
    let mut wrong = 0i64;
    for r in rows {
        let mut claims_below = false;
        let mut claims_above = false;
        for (index, atom) in vocabulary.atoms.iter().enumerate() {
            if r.conditions.iter().any(|c| c == atom) {
                claims_below |= chromosome.get(Vocabulary::bit(index, Direction::Below));
                claims_above |= chromosome.get(Vocabulary::bit(index, Direction::Above));
            }
        }
        let flagged = claims_below || claims_above;
        let covered = match r.outcome {
            Some(Direction::Below) => claims_below,
            Some(Direction::Above) => claims_above,
            None => false,
        };
        if covered {
            correct += 1;
        } else if flagged {
            wrong += 1;
        }
    }
    let bits = chromosome.count_ones() as f64;
    ((correct - wrong) as f64 - lambda * bits) / rows.len() as f64
}

/// The rows as a search problem. Repair stays the trait's no-op: a
/// condition atom has no degree, so Gene Repair has no meaning here.
#[allow(dead_code)]
struct LedgerFit<'a> {
    rows: &'a [ExperimentRow],
    vocabulary: &'a Vocabulary,
    lambda: f64,
}

impl erbga::Problem for LedgerFit<'_> {
    fn genome_len(&self) -> usize {
        self.vocabulary.genome_len()
    }

    fn fitness(&self, chromosome: &Chromosome) -> f64 {
        score(self.rows, self.vocabulary, chromosome, self.lambda)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(conditions: &[(&str, &str)], outcome: Option<Direction>) -> ExperimentRow {
        ExperimentRow {
            conditions: conditions
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            outcome,
        }
    }

    /// Exhaustive struct literal on purpose: adding any field to
    /// `ExperimentRow`, in particular a text field, breaks this compile
    /// until someone edits it here, in front of the integrity note.
    #[test]
    fn the_input_type_carries_atoms_and_a_direction_and_nothing_else() {
        let _ = ExperimentRow {
            conditions: vec![("k".to_string(), "v".to_string())],
            outcome: Some(Direction::Above),
        };
    }

    #[test]
    fn vocabulary_is_sorted_deduped_atoms() {
        let rows = vec![
            row(&[("b", "2"), ("a", "1")], None),
            row(&[("a", "1"), ("a", "2")], Some(Direction::Below)),
        ];
        let v = Vocabulary::from_rows(&rows);
        assert_eq!(v.len(), 3);
        assert_eq!(v.genome_len(), 6);
        assert!(!v.is_empty());
    }

    #[test]
    fn empty_rows_give_an_empty_vocabulary() {
        let v = Vocabulary::from_rows(&[]);
        assert!(v.is_empty());
        assert_eq!(v.genome_len(), 0);
    }

    #[test]
    fn decode_names_the_set_bits_as_sorted_claims() {
        let rows = vec![row(&[("a", "1"), ("b", "2")], None)];
        let v = Vocabulary::from_rows(&rows);
        // Atoms sort to [(a,1), (b,2)]. Bit layout is atom_index * 2,
        // Below first. Set (a,1,Above) and (b,2,Below).
        let mut c = Chromosome::zeros(v.genome_len());
        c.set(1, true);
        c.set(2, true);
        let claims = v.decode(&c);
        assert_eq!(
            claims,
            vec![
                Claim {
                    key: "a".to_string(),
                    value: "1".to_string(),
                    direction: Direction::Above,
                },
                Claim {
                    key: "b".to_string(),
                    value: "2".to_string(),
                    direction: Direction::Below,
                },
            ]
        );
    }

    /// Encode `claims` for `vocabulary`, panicking on an atom the
    /// vocabulary does not hold. Test-side inverse of `decode`.
    fn encode(v: &Vocabulary, claims: &[(&str, &str, Direction)]) -> Chromosome {
        let mut c = Chromosome::zeros(v.genome_len());
        for (key, value, direction) in claims {
            let index = v
                .atoms
                .iter()
                .position(|(k, val)| k == key && val == value)
                .expect("atom in vocabulary");
            c.set(Vocabulary::bit(index, *direction), true);
        }
        c
    }

    #[test]
    fn a_correct_claim_is_rewarded_and_parsimony_is_charged() {
        let rows = vec![
            row(&[("harness", "b")], Some(Direction::Above)),
            row(&[("harness", "a")], None),
        ];
        let v = Vocabulary::from_rows(&rows);
        let h = encode(&v, &[("harness", "b", Direction::Above)]);
        // One correct cover, no wrong flags, one set bit:
        // (1 - 0 - lambda * 1) / 2 rows.
        let got = score(&rows, &v, &h, 0.1);
        assert!((got - (1.0 - 0.1) / 2.0).abs() < 1e-12);
    }

    #[test]
    fn flagging_an_unremarkable_row_costs() {
        let rows = vec![
            row(&[("harness", "b")], Some(Direction::Above)),
            row(&[("harness", "b")], None),
        ];
        let v = Vocabulary::from_rows(&rows);
        let h = encode(&v, &[("harness", "b", Direction::Above)]);
        // One correct, one wrong flag: (1 - 1 - 0.1) / 2.
        let got = score(&rows, &v, &h, 0.1);
        assert!((got - (-0.1) / 2.0).abs() < 1e-12);
    }

    #[test]
    fn a_claim_in_the_wrong_direction_is_a_wrong_flag() {
        let rows = vec![row(&[("harness", "b")], Some(Direction::Above))];
        let v = Vocabulary::from_rows(&rows);
        let h = encode(&v, &[("harness", "b", Direction::Below)]);
        // Flagged, not correct: (0 - 1 - 0.1) / 1.
        let got = score(&rows, &v, &h, 0.1);
        assert!((got - (-1.1)).abs() < 1e-12);
    }

    #[test]
    fn the_empty_hypothesis_scores_zero() {
        let rows = vec![row(&[("harness", "b")], Some(Direction::Above))];
        let v = Vocabulary::from_rows(&rows);
        let h = Chromosome::zeros(v.genome_len());
        assert_eq!(score(&rows, &v, &h, 0.1), 0.0);
    }

    #[test]
    fn no_rows_score_zero_rather_than_dividing_by_nothing() {
        let v = Vocabulary::from_rows(&[]);
        let h = Chromosome::zeros(0);
        assert_eq!(score(&[], &v, &h, 0.1), 0.0);
    }
}
