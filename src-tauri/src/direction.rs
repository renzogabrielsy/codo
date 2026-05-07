//! IN/OUT direction helper for warehouse-balance accounting.
//!
//! From docs/schema-extraction.md §4.4: codo replaces the workbook's
//! "last-hyphen-segment of UNIQUE TAG" substring trick with a typed match on
//! `(disposition_kind, source.kind, warehouse)`. The substring trick gets
//! tank-stage partner takebacks WRONG (they show STATE=OUT in the workbook
//! despite touching no warehouse). This helper returns `None` for those rows
//! so the ledger function skips them.

use crate::canonicalize::{Disposition, SourceKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    In,
    Out,
}

/// Compute whether a production_event row contributes to a warehouse ledger,
/// and in which direction.
///
/// Returns `None` for rows that don't touch any warehouse balance — the
/// ledger function skips them.
pub fn direction(
    disposition: Disposition,
    source: SourceKind,
    warehouse_present: bool,
) -> Option<Direction> {
    match (disposition, source, warehouse_present) {
        // CI bagged into a destination warehouse → inflow.
        (Disposition::FlecBagging, _, true) => Some(Direction::In),

        // Partner pulled bagged stock from a warehouse → outflow.
        (
            Disposition::PartnerCrusher(_) | Disposition::PartnerKiln(_),
            SourceKind::WarehouseFlec,
            true,
        ) => Some(Direction::Out),

        // Partner pulled DVO container product (only valid for WHSE 3 — the
        // schema validator forbids this combination with other warehouses or
        // with a NULL warehouse, see validation.rs and §7.1 matrix).
        (
            Disposition::PartnerCrusher(_) | Disposition::PartnerKiln(_),
            SourceKind::DvoContainer,
            true,
        ) => Some(Direction::Out),

        // Partner pulled directly from a tank or plant_direct — no warehouse
        // balance is affected, even if the workbook accidentally has a
        // warehouse value in the column.
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canonicalize::{Disposition, SourceKind};

    #[test]
    fn ci_bagging_from_tank_is_inflow() {
        // Schema §7.1 first VALID row.
        let d = direction(Disposition::FlecBagging, SourceKind::Tank, true);
        assert_eq!(d, Some(Direction::In));
    }

    #[test]
    fn ci_bagging_from_plant_direct_is_inflow() {
        let d = direction(Disposition::FlecBagging, SourceKind::PlantDirect, true);
        assert_eq!(d, Some(Direction::In));
    }

    #[test]
    fn partner_pull_from_warehouse_flec_is_outflow() {
        let d = direction(
            Disposition::PartnerCrusher(1),
            SourceKind::WarehouseFlec,
            true,
        );
        assert_eq!(d, Some(Direction::Out));

        let d = direction(Disposition::PartnerKiln(3), SourceKind::WarehouseFlec, true);
        assert_eq!(d, Some(Direction::Out));
    }

    #[test]
    fn partner_pull_from_dvo_is_outflow() {
        let d = direction(
            Disposition::PartnerCrusher(1),
            SourceKind::DvoContainer,
            true,
        );
        assert_eq!(d, Some(Direction::Out));
    }

    #[test]
    fn partner_pull_from_tank_is_no_warehouse_event() {
        // Schema §7.1 explicit: tank-stage partner takebacks DON'T touch a
        // warehouse balance. The substring-of-unique_tag trick in Excel gets
        // this wrong. We return None so the ledger skips these rows.
        let d = direction(Disposition::PartnerCrusher(2), SourceKind::Tank, false);
        assert_eq!(d, None);
        // Even if a stray warehouse value snuck in, this combo is forbidden
        // and would be rejected by the validator before reaching the ledger.
        // direction() defensively returns None too.
        let d = direction(Disposition::PartnerCrusher(2), SourceKind::Tank, true);
        assert_eq!(d, None);
    }

    #[test]
    fn partner_pull_from_plant_direct_is_no_warehouse_event() {
        let d = direction(
            Disposition::PartnerKiln(2),
            SourceKind::PlantDirect,
            false,
        );
        assert_eq!(d, None);
    }

    #[test]
    fn flec_bagging_without_warehouse_is_unmapped() {
        // The validator forbids this row entirely — direction() defensively
        // returns None so the ledger never tries to apply it.
        let d = direction(Disposition::FlecBagging, SourceKind::Tank, false);
        assert_eq!(d, None);
    }
}
