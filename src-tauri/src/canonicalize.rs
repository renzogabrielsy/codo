//! Canonicalize-at-write functions for every categorical column.
//!
//! PROJECT_BRAIN.md §4.4: every free-text field that becomes a category goes
//! through one of these `canonicalize_<field>` functions before any insert or
//! update. Lock-in tests below cover every observed input variant in
//! docs/schema-extraction.md §2 plus every typo Renzo's workbook contains.

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum CanonError {
    #[error("unknown {field} value: {raw:?}")]
    Unknown { field: &'static str, raw: String },

    #[error("ambiguous {field}: {raw:?}")]
    Ambiguous { field: &'static str, raw: String },
}

// ---------------------------------------------------------------------------
// SHIFT
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Shift {
    M,
    E,
    N,
}

impl Shift {
    pub fn code(self) -> &'static str {
        match self {
            Shift::M => "M",
            Shift::E => "E",
            Shift::N => "N",
        }
    }
}

/// Strip whitespace, drop trailing punctuation noise (`M,` → `M`), uppercase,
/// then match against the closed set.
pub fn canonicalize_shift(raw: &str) -> Result<Shift, CanonError> {
    let cleaned: String = raw
        .chars()
        .filter(|c| c.is_alphabetic())
        .collect::<String>()
        .to_uppercase();
    match cleaned.as_str() {
        "M" => Ok(Shift::M),
        "E" => Ok(Shift::E),
        "N" => Ok(Shift::N),
        _ => Err(CanonError::Unknown {
            field: "shift",
            raw: raw.to_string(),
        }),
    }
}

// ---------------------------------------------------------------------------
// GRADE
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Grade {
    G3x50,
    G2x6,
    G3p5,
    G4x8,
}

impl Grade {
    pub fn code(self) -> &'static str {
        match self {
            Grade::G3x50 => "3X50",
            Grade::G2x6 => "2X6",
            Grade::G3p5 => "3.5",
            Grade::G4x8 => "4X8",
        }
    }
}

pub fn canonicalize_grade(raw: &str) -> Result<Grade, CanonError> {
    let cleaned = raw.trim().to_uppercase().replace(' ', "");
    match cleaned.as_str() {
        "3X50" => Ok(Grade::G3x50),
        "2X6" => Ok(Grade::G2x6),
        "3.5" | "3,5" => Ok(Grade::G3p5),
        "4X8" => Ok(Grade::G4x8),
        _ => Err(CanonError::Unknown {
            field: "grade",
            raw: raw.to_string(),
        }),
    }
}

// ---------------------------------------------------------------------------
// PLANT
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Plant {
    W6,
    W7,
    W6W7,
    Dvo,
}

impl Plant {
    pub fn code(self) -> &'static str {
        match self {
            Plant::W6 => "W6",
            Plant::W7 => "W7",
            Plant::W6W7 => "W6/W7",
            Plant::Dvo => "DVO",
        }
    }
}

pub fn canonicalize_plant(raw: &str) -> Result<Plant, CanonError> {
    let cleaned = raw.trim().to_uppercase().replace(' ', "");
    match cleaned.as_str() {
        "W6" => Ok(Plant::W6),
        "W7" => Ok(Plant::W7),
        "W6/W7" | "W6\\W7" => Ok(Plant::W6W7),
        "DVO" => Ok(Plant::Dvo),
        _ => Err(CanonError::Unknown {
            field: "plant",
            raw: raw.to_string(),
        }),
    }
}

// ---------------------------------------------------------------------------
// WAREHOUSE (the destination column — `W6`/`W7` here are cosmetic noise → None)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Warehouse {
    W1,
    W2,
    W3,
    W5,
    W7,
}

impl Warehouse {
    pub fn code(self) -> &'static str {
        match self {
            Warehouse::W1 => "WHSE 1",
            Warehouse::W2 => "WHSE 2",
            Warehouse::W3 => "WHSE 3",
            Warehouse::W5 => "WHSE 5",
            Warehouse::W7 => "WHSE 7",
        }
    }
}

/// Returns `Ok(None)` for the empty string AND for the cosmetic `W6` / `W7`
/// values that only appear in the destination-warehouse column on
/// pre-auto-fill rows. Renzo confirmed they should logically be NULL —
/// PROJECT_BRAIN.md §4.4 + §6.7 + schema-extraction.md §2.
pub fn canonicalize_warehouse(raw: &str) -> Result<Option<Warehouse>, CanonError> {
    let cleaned = raw.trim().to_uppercase();
    if cleaned.is_empty() {
        return Ok(None);
    }
    // The cosmetic W6/W7 in this column. Migration / new writes treat as NULL.
    if cleaned == "W6" || cleaned == "W7" || cleaned == "W3" {
        return Ok(None);
    }
    let normalized = cleaned.replace(' ', "");
    match normalized.as_str() {
        "WHSE1" => Ok(Some(Warehouse::W1)),
        "WHSE2" => Ok(Some(Warehouse::W2)),
        "WHSE3" => Ok(Some(Warehouse::W3)),
        "WHSE5" => Ok(Some(Warehouse::W5)),
        "WHSE7" => Ok(Some(Warehouse::W7)),
        _ => Err(CanonError::Unknown {
            field: "warehouse",
            raw: raw.to_string(),
        }),
    }
}

// ---------------------------------------------------------------------------
// SOURCE LOCATION
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SourceCode {
    Tnk1,
    Tnk2,
    Tnk3,
    Tnk4,
    W7,
    W6,
    Flec,
    Dvo,
}

impl SourceCode {
    pub fn code(self) -> &'static str {
        match self {
            SourceCode::Tnk1 => "TNK 1",
            SourceCode::Tnk2 => "TNK 2",
            SourceCode::Tnk3 => "TNK 3",
            SourceCode::Tnk4 => "TNK 4",
            SourceCode::W7 => "W7",
            SourceCode::W6 => "W6",
            SourceCode::Flec => "FLEC",
            SourceCode::Dvo => "DVO",
        }
    }
    pub fn kind(self) -> SourceKind {
        match self {
            SourceCode::Tnk1
            | SourceCode::Tnk2
            | SourceCode::Tnk3
            | SourceCode::Tnk4
            | SourceCode::W7 => SourceKind::Tank,
            SourceCode::W6 => SourceKind::PlantDirect,
            SourceCode::Flec => SourceKind::WarehouseFlec,
            SourceCode::Dvo => SourceKind::DvoContainer,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SourceKind {
    Tank,
    PlantDirect,
    WarehouseFlec,
    DvoContainer,
}

pub fn canonicalize_source(raw: &str) -> Result<SourceCode, CanonError> {
    let cleaned = raw.trim().to_uppercase();
    let normalized = cleaned.replace(' ', "");
    match normalized.as_str() {
        "TNK1" => Ok(SourceCode::Tnk1),
        "TNK2" => Ok(SourceCode::Tnk2),
        "TNK3" => Ok(SourceCode::Tnk3),
        "TNK4" => Ok(SourceCode::Tnk4),
        "W7" => Ok(SourceCode::W7),
        "W6" => Ok(SourceCode::W6),
        "FLEC" => Ok(SourceCode::Flec),
        "DVO" => Ok(SourceCode::Dvo),
        _ => Err(CanonError::Unknown {
            field: "source_location",
            raw: raw.to_string(),
        }),
    }
}

// ---------------------------------------------------------------------------
// WHSE SIDE  (LS / RS only — DVO batch codes are parsed separately)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Side {
    Ls,
    Rs,
}

impl Side {
    pub fn code(self) -> &'static str {
        match self {
            Side::Ls => "LS",
            Side::Rs => "RS",
        }
    }
}

pub fn canonicalize_whse_side(raw: &str) -> Result<Option<Side>, CanonError> {
    let cleaned = raw.trim().to_uppercase();
    if cleaned.is_empty() {
        return Ok(None);
    }
    match cleaned.as_str() {
        "LS" => Ok(Some(Side::Ls)),
        "RS" => Ok(Some(Side::Rs)),
        _ => Err(CanonError::Unknown {
            field: "whse_side",
            raw: raw.to_string(),
        }),
    }
}

// ---------------------------------------------------------------------------
// FLEC STAT  — legacy column, schema §6.7. Trim + upper, no validation; if it
// arrives empty we return None so it lands as NULL.
// ---------------------------------------------------------------------------

pub fn canonicalize_flec_stat(raw: &str) -> Option<String> {
    let cleaned = raw.trim().to_uppercase();
    if cleaned.is_empty() {
        None
    } else {
        Some(cleaned)
    }
}

// ---------------------------------------------------------------------------
// DISPOSITION  (the renamed `CCC / FLEC` column)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "equipment_code")]
pub enum Disposition {
    FlecBagging,
    PartnerCrusher(u8), // 1..=4
    PartnerKiln(u8),    // 1..=4
}

impl Disposition {
    pub fn kind_code(self) -> &'static str {
        match self {
            Disposition::FlecBagging => "flec_bagging",
            Disposition::PartnerCrusher(_) => "partner_crusher",
            Disposition::PartnerKiln(_) => "partner_kiln",
        }
    }
    pub fn equipment_code(self) -> Option<String> {
        match self {
            Disposition::FlecBagging => None,
            Disposition::PartnerCrusher(n) => Some(format!("C{n}")),
            Disposition::PartnerKiln(n) => Some(format!("RK{n}")),
        }
    }
    /// Preserves the workbook's `CCC / FLEC` cell value for `unique_tag`
    /// reconstruction (10-segment hyphen-concat from schema §3.1).
    pub fn raw_form(self) -> &'static str {
        match self {
            Disposition::FlecBagging => "FLEC",
            Disposition::PartnerCrusher(1) => "C1",
            Disposition::PartnerCrusher(2) => "C2",
            Disposition::PartnerCrusher(3) => "C3",
            Disposition::PartnerCrusher(4) => "C4",
            Disposition::PartnerKiln(1) => "RK1",
            Disposition::PartnerKiln(2) => "RK2",
            Disposition::PartnerKiln(3) => "RK3",
            Disposition::PartnerKiln(4) => "RK4",
            _ => "",
        }
    }
}

pub fn canonicalize_disposition(raw: &str) -> Result<Disposition, CanonError> {
    let cleaned = raw.trim().to_uppercase().replace(' ', "");
    match cleaned.as_str() {
        "FLEC" => Ok(Disposition::FlecBagging),
        "C1" => Ok(Disposition::PartnerCrusher(1)),
        "C2" => Ok(Disposition::PartnerCrusher(2)),
        "C3" => Ok(Disposition::PartnerCrusher(3)),
        "C4" => Ok(Disposition::PartnerCrusher(4)),
        "RK1" => Ok(Disposition::PartnerKiln(1)),
        "RK2" => Ok(Disposition::PartnerKiln(2)),
        "RK3" => Ok(Disposition::PartnerKiln(3)),
        "RK4" => Ok(Disposition::PartnerKiln(4)),
        _ => Err(CanonError::Unknown {
            field: "disposition",
            raw: raw.to_string(),
        }),
    }
}

// ---------------------------------------------------------------------------
// Lock-in tests — every observed input variant from schema-extraction §2.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shift_observed_variants() {
        // §2: M (711×), 'M,' (1× typo), ' M' (1× typo).
        assert_eq!(canonicalize_shift("M").unwrap(), Shift::M);
        assert_eq!(canonicalize_shift("M,").unwrap(), Shift::M);
        assert_eq!(canonicalize_shift(" M").unwrap(), Shift::M);
        assert_eq!(canonicalize_shift("m").unwrap(), Shift::M);
        // Legend-listed but unobserved.
        assert_eq!(canonicalize_shift("E").unwrap(), Shift::E);
        assert_eq!(canonicalize_shift("N").unwrap(), Shift::N);
        // Garbage rejected.
        assert!(canonicalize_shift("X").is_err());
        assert!(canonicalize_shift("").is_err());
    }

    #[test]
    fn grade_observed_variants() {
        assert_eq!(canonicalize_grade("3X50").unwrap(), Grade::G3x50);
        assert_eq!(canonicalize_grade("3x50").unwrap(), Grade::G3x50);
        assert_eq!(canonicalize_grade("2X6").unwrap(), Grade::G2x6);
        assert_eq!(canonicalize_grade("3.5").unwrap(), Grade::G3p5);
        assert_eq!(canonicalize_grade("4X8").unwrap(), Grade::G4x8);
        // Excel renders 3.5 with optional whitespace from numeric coercion.
        assert_eq!(canonicalize_grade(" 3.5 ").unwrap(), Grade::G3p5);
        assert!(canonicalize_grade("").is_err());
        assert!(canonicalize_grade("ABC").is_err());
    }

    #[test]
    fn plant_observed_variants() {
        // §2: W6 (371), W7 (188), DVO (120), 'W6 / W7' (87), 'W6 /W7' (1 typo),
        //     'W' (1 garbage), '37.0' (1 garbage).
        assert_eq!(canonicalize_plant("W6").unwrap(), Plant::W6);
        assert_eq!(canonicalize_plant("W7").unwrap(), Plant::W7);
        assert_eq!(canonicalize_plant("DVO").unwrap(), Plant::Dvo);
        assert_eq!(canonicalize_plant("W6/W7").unwrap(), Plant::W6W7);
        assert_eq!(canonicalize_plant("W6 / W7").unwrap(), Plant::W6W7);
        assert_eq!(canonicalize_plant("W6 /W7").unwrap(), Plant::W6W7);
        // Garbage that should be drift-logged.
        assert!(canonicalize_plant("W").is_err());
        assert!(canonicalize_plant("37.0").is_err());
    }

    #[test]
    fn warehouse_real_values_keep() {
        assert_eq!(canonicalize_warehouse("WHSE 1").unwrap(), Some(Warehouse::W1));
        assert_eq!(canonicalize_warehouse("WHSE 2").unwrap(), Some(Warehouse::W2));
        assert_eq!(canonicalize_warehouse("WHSE 3").unwrap(), Some(Warehouse::W3));
        assert_eq!(canonicalize_warehouse("WHSE 5").unwrap(), Some(Warehouse::W5));
        assert_eq!(canonicalize_warehouse("WHSE 7").unwrap(), Some(Warehouse::W7));
        // Spacing tolerance.
        assert_eq!(canonicalize_warehouse("whse7").unwrap(), Some(Warehouse::W7));
    }

    #[test]
    fn warehouse_cosmetic_w6_w7_become_none() {
        // §4.4 + §6.7: W6/W7 in the warehouse column are cosmetic noise.
        assert_eq!(canonicalize_warehouse("W6").unwrap(), None);
        assert_eq!(canonicalize_warehouse("W7").unwrap(), None);
        assert_eq!(canonicalize_warehouse("W3").unwrap(), None);
        assert_eq!(canonicalize_warehouse("").unwrap(), None);
        assert_eq!(canonicalize_warehouse("   ").unwrap(), None);
    }

    #[test]
    fn warehouse_garbage_rejected() {
        assert!(canonicalize_warehouse("FOO").is_err());
    }

    #[test]
    fn source_observed_variants() {
        // §2 frequencies: FLEC (165), W7 (146), TNK 1 (137), DVO (120),
        //                 TNK 2 (101), TNK 3 (50), W6 (27), TNK 4 (16).
        assert_eq!(canonicalize_source("FLEC").unwrap(), SourceCode::Flec);
        assert_eq!(canonicalize_source("W7").unwrap(), SourceCode::W7);
        assert_eq!(canonicalize_source("W6").unwrap(), SourceCode::W6);
        assert_eq!(canonicalize_source("TNK 1").unwrap(), SourceCode::Tnk1);
        assert_eq!(canonicalize_source("TNK 2").unwrap(), SourceCode::Tnk2);
        assert_eq!(canonicalize_source("TNK 3").unwrap(), SourceCode::Tnk3);
        assert_eq!(canonicalize_source("TNK 4").unwrap(), SourceCode::Tnk4);
        assert_eq!(canonicalize_source("DVO").unwrap(), SourceCode::Dvo);
        // Tolerate the workbook's spacing variations.
        assert_eq!(canonicalize_source("tnk1").unwrap(), SourceCode::Tnk1);
        assert!(canonicalize_source("XYZ").is_err());
    }

    #[test]
    fn source_kind_pairing() {
        // SRC↔kind pairing per schema §6.1.
        assert_eq!(SourceCode::Tnk1.kind(), SourceKind::Tank);
        assert_eq!(SourceCode::W7.kind(), SourceKind::Tank);
        assert_eq!(SourceCode::W6.kind(), SourceKind::PlantDirect);
        assert_eq!(SourceCode::Flec.kind(), SourceKind::WarehouseFlec);
        assert_eq!(SourceCode::Dvo.kind(), SourceKind::DvoContainer);
    }

    #[test]
    fn whse_side_variants() {
        assert_eq!(canonicalize_whse_side("LS").unwrap(), Some(Side::Ls));
        assert_eq!(canonicalize_whse_side("RS").unwrap(), Some(Side::Rs));
        assert_eq!(canonicalize_whse_side("ls").unwrap(), Some(Side::Ls));
        assert_eq!(canonicalize_whse_side("").unwrap(), None);
        // DVO batch codes are NOT canonicalized here — they're handled by a
        // separate parser before the canonicalize layer sees the value.
        assert!(canonicalize_whse_side("NOVEMBER2025RIGHT").is_err());
    }

    #[test]
    fn disposition_observed_variants() {
        // §2 frequencies: C1 (345), FLEC (238), C2 (76), RK4 (58), RK3 (26),
        //                 RK2 (24), RK1 (1), 'FLEC ' (1 typo).
        assert_eq!(canonicalize_disposition("FLEC").unwrap(), Disposition::FlecBagging);
        assert_eq!(canonicalize_disposition("FLEC ").unwrap(), Disposition::FlecBagging);
        assert_eq!(canonicalize_disposition("C1").unwrap(), Disposition::PartnerCrusher(1));
        assert_eq!(canonicalize_disposition("C2").unwrap(), Disposition::PartnerCrusher(2));
        assert_eq!(canonicalize_disposition("C3").unwrap(), Disposition::PartnerCrusher(3));
        assert_eq!(canonicalize_disposition("C4").unwrap(), Disposition::PartnerCrusher(4));
        assert_eq!(canonicalize_disposition("RK1").unwrap(), Disposition::PartnerKiln(1));
        assert_eq!(canonicalize_disposition("RK2").unwrap(), Disposition::PartnerKiln(2));
        assert_eq!(canonicalize_disposition("RK3").unwrap(), Disposition::PartnerKiln(3));
        assert_eq!(canonicalize_disposition("RK4").unwrap(), Disposition::PartnerKiln(4));
        assert!(canonicalize_disposition("C5").is_err());
        assert!(canonicalize_disposition("RK5").is_err());
    }
}
