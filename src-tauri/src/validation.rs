//! Application-layer row-level validation.
//!
//! Implements the validity matrix from docs/schema-extraction.md §7.1 and the
//! SRC↔PLANT pairing from §7.2. Every write site calls
//! [`validate_production_event`] before issuing the INSERT. Test suite in
//! `tests/validity_matrix.rs` walks every cell of the matrix.

use crate::canonicalize::{Disposition, Plant, SourceCode, SourceKind, Warehouse};
use thiserror::Error;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ValidationError {
    #[error("forbidden: {reason}")]
    Forbidden { reason: String },

    #[error("plant_id mismatch: source {source_code} requires plant {expected}, got {got:?}")]
    PlantMismatch {
        source_code: &'static str,
        expected: &'static str,
        got: Option<&'static str>,
    },

    #[error("missing required field: {0}")]
    MissingField(&'static str),
}

impl ValidationError {
    fn forbidden<S: Into<String>>(reason: S) -> Self {
        ValidationError::Forbidden {
            reason: reason.into(),
        }
    }
}

/// The minimal set of canonicalized fields needed to gate a production_event
/// insert. Caller has already canonicalized everything.
#[derive(Debug, Clone, Copy)]
pub struct EventShape {
    pub disposition: Disposition,
    pub source: SourceCode,
    pub warehouse: Option<Warehouse>,
    pub plant: Option<Plant>,
}

/// Apply §7.1 + §7.2. Returns `Ok(())` for VALID rows, `Err(ValidationError)`
/// for FORBIDDEN rows with the documented reason.
pub fn validate_production_event(e: EventShape) -> Result<(), ValidationError> {
    let source_kind = e.source.kind();

    // ── §7.1 validity matrix ────────────────────────────────────────────────
    match (e.disposition, source_kind, e.warehouse) {
        // VALID: CI bagged from a tank into WHSE 1/2/5/7.
        (Disposition::FlecBagging, SourceKind::Tank, Some(w)) => {
            if w == Warehouse::W3 {
                return Err(ValidationError::forbidden(
                    "WHSE 3 is DVO-only; CI doesn't bag CI Cebu product into WHSE 3",
                ));
            }
        }
        (Disposition::FlecBagging, SourceKind::Tank, None) => {
            return Err(ValidationError::forbidden(
                "a bagging event must have a destination warehouse",
            ))
        }

        // VALID: CI bagged direct from W6 plant into WHSE 1/2/5/7.
        (Disposition::FlecBagging, SourceKind::PlantDirect, Some(w)) => {
            if w == Warehouse::W3 {
                return Err(ValidationError::forbidden("WHSE 3 is DVO-only"));
            }
        }
        (Disposition::FlecBagging, SourceKind::PlantDirect, None) => {
            return Err(ValidationError::forbidden(
                "a bagging event must have a destination warehouse",
            ))
        }

        // FORBIDDEN: bagging from already-bagged or DVO sources.
        (Disposition::FlecBagging, SourceKind::WarehouseFlec, _) => {
            return Err(ValidationError::forbidden(
                "the product is already bagged; you can't bag bags",
            ))
        }
        (Disposition::FlecBagging, SourceKind::DvoContainer, _) => {
            return Err(ValidationError::forbidden(
                "DVO charcoal arrives in PP sacks, not flec bags; CI doesn't run \
                 flec_bagging events on DVO product",
            ))
        }

        // VALID: Partner pulled from a tank — no warehouse touched.
        (
            Disposition::PartnerCrusher(_) | Disposition::PartnerKiln(_),
            SourceKind::Tank,
            None,
        ) => {}
        (
            Disposition::PartnerCrusher(_) | Disposition::PartnerKiln(_),
            SourceKind::Tank,
            Some(_),
        ) => {
            return Err(ValidationError::forbidden(
                "tank-stage partner takebacks don't touch a warehouse",
            ))
        }

        // VALID: Partner pulled direct from plant — rare but real.
        (
            Disposition::PartnerCrusher(_) | Disposition::PartnerKiln(_),
            SourceKind::PlantDirect,
            None,
        ) => {}
        (
            Disposition::PartnerCrusher(_) | Disposition::PartnerKiln(_),
            SourceKind::PlantDirect,
            Some(_),
        ) => {
            return Err(ValidationError::forbidden(
                "plant-direct partner takebacks don't touch a warehouse",
            ))
        }

        // VALID: Partner pulled bagged stock from WHSE 1/2/5/7.
        (
            Disposition::PartnerCrusher(_) | Disposition::PartnerKiln(_),
            SourceKind::WarehouseFlec,
            Some(w),
        ) => {
            if w == Warehouse::W3 {
                return Err(ValidationError::forbidden(
                    "WHSE 3 holds DVO product in PP sacks, not flec bags; \
                     warehouse_flec source kind only applies to WHSE 1/2/5/7",
                ));
            }
        }
        (
            Disposition::PartnerCrusher(_) | Disposition::PartnerKiln(_),
            SourceKind::WarehouseFlec,
            None,
        ) => {
            return Err(ValidationError::forbidden(
                "a warehouse_flec source must identify which warehouse the flec was \
                 pulled from",
            ))
        }

        // VALID: Partner pulled DVO product from WHSE 3 only.
        (
            Disposition::PartnerCrusher(_) | Disposition::PartnerKiln(_),
            SourceKind::DvoContainer,
            Some(Warehouse::W3),
        ) => {}
        (
            Disposition::PartnerCrusher(_) | Disposition::PartnerKiln(_),
            SourceKind::DvoContainer,
            Some(_),
        ) => {
            return Err(ValidationError::forbidden(
                "DVO product is only stored in WHSE 3",
            ))
        }
        (
            Disposition::PartnerCrusher(_) | Disposition::PartnerKiln(_),
            SourceKind::DvoContainer,
            None,
        ) => {
            return Err(ValidationError::forbidden(
                "a dvo_container source must identify WHSE 3 as the warehouse",
            ))
        }
    }

    // ── §7.2 SRC↔PLANT pairing ──────────────────────────────────────────────
    require_plant_pairing(e.source, e.plant)?;
    Ok(())
}

fn require_plant_pairing(
    source: SourceCode,
    plant: Option<Plant>,
) -> Result<(), ValidationError> {
    let (expected_code, expected_plant): (&'static str, Option<Plant>) = match source {
        SourceCode::Tnk1 | SourceCode::Tnk2 | SourceCode::Tnk3 | SourceCode::Tnk4 => {
            ("W6", Some(Plant::W6))
        }
        SourceCode::W7 => ("W7", Some(Plant::W7)),
        SourceCode::W6 => ("W6", Some(Plant::W6)),
        SourceCode::Dvo => ("DVO", Some(Plant::Dvo)),
        // FLEC: plant_id is informational; rule 27 allows any plant or NULL.
        SourceCode::Flec => return Ok(()),
    };

    match plant {
        Some(p) if Some(p) == expected_plant => Ok(()),
        Some(p) => Err(ValidationError::PlantMismatch {
            source_code: source.code(),
            expected: expected_code,
            got: Some(p.code()),
        }),
        None => Err(ValidationError::PlantMismatch {
            source_code: source.code(),
            expected: expected_code,
            got: None,
        }),
    }
}
