use lwe_library::WorkshopProjectType;

pub fn supports_first_release(project_type: WorkshopProjectType) -> bool {
    matches!(project_type, WorkshopProjectType::Video)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lwe_library::WorkshopProjectType;

    #[test]
    fn first_release_support_only_includes_video() {
        assert!(supports_first_release(WorkshopProjectType::Video));
        assert!(!supports_first_release(WorkshopProjectType::Scene));
        assert!(!supports_first_release(WorkshopProjectType::Web));
        assert!(!supports_first_release(WorkshopProjectType::Other));
    }

    #[test]
    fn shared_policy_filter_covers_support_policy() {
        first_release_support_only_includes_video();
    }
}
