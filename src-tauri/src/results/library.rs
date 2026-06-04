use crate::results::workshop::AssessedWorkshopCatalogEntry;

#[derive(Debug, Clone)]
pub struct LibraryProjection {
    pub entries: Vec<AssessedWorkshopCatalogEntry>,
    pub source_catalog_count: usize,
    pub served_from_snapshot: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_policy_library_projection_records_source_catalog_size() {
        let projection = LibraryProjection {
            entries: Vec::new(),
            source_catalog_count: 3,
            served_from_snapshot: true,
        };

        assert_eq!(projection.source_catalog_count, 3);
        assert!(projection.entries.is_empty());
        assert!(projection.served_from_snapshot);
    }
}
