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
}
