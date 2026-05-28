use lwe_library::{ThumbnailGenerator, WorkshopCatalogEntry};

use crate::policies::shared::cover_policy::{CoverArtSource, cover_art_source};

pub fn detail_cover_path(entry: &WorkshopCatalogEntry) -> Option<String> {
    let bundled_cover_path = entry
        .cover_path
        .as_ref()
        .map(|path| path.to_string_lossy().into_owned());

    match cover_art_source(bundled_cover_path) {
        CoverArtSource::Bundled(path) => Some(path),
        CoverArtSource::Placeholder => None,
    }
}

pub fn card_cover_path(entry: &WorkshopCatalogEntry) -> Option<String> {
    let generator = ThumbnailGenerator::for_card_grid();
    entry
        .cover_path
        .as_ref()
        .and_then(|path| generator.card_grid_cover_path(path))
        .and_then(|path| cover_art_source(Some(path.to_string_lossy().into_owned())).into_path())
}

trait CoverArtSourceExt {
    fn into_path(self) -> Option<String>;
}

impl CoverArtSourceExt for CoverArtSource {
    fn into_path(self) -> Option<String> {
        match self {
            CoverArtSource::Bundled(path) => Some(path),
            CoverArtSource::Placeholder => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::DynamicImage;
    use lwe_library::{WorkshopProjectType, WorkshopSyncState};
    use std::path::{Path, PathBuf};
    use tempfile::TempDir;

    fn entry_with_cover(path: PathBuf) -> WorkshopCatalogEntry {
        WorkshopCatalogEntry {
            workshop_id: 42,
            title: "Preview".to_string(),
            project_type: WorkshopProjectType::Video,
            project_dir: path.parent().unwrap_or(Path::new("/")).to_path_buf(),
            cover_path: Some(path),
            sync_state: WorkshopSyncState::Synced,
            supported_first_release: true,
            library_item_id: Some("video-42".to_string()),
        }
    }

    #[test]
    fn card_cover_uses_existing_thumbnail_instead_of_raw_gif() {
        let _env = crate::test_env::env_lock();
        let temp_dir = TempDir::new().unwrap();
        let previous_home = std::env::var_os("HOME");
        unsafe {
            std::env::set_var("HOME", temp_dir.path());
        }
        let gif_path = temp_dir.path().join("preview.gif");
        let img = DynamicImage::new_rgb8(800, 600);
        img.save(&gif_path).unwrap();

        let generator = ThumbnailGenerator::for_card_grid();
        let cache_path = generator.generate_cached_path(&gif_path).unwrap();
        let entry = entry_with_cover(gif_path);

        let cover = card_cover_path(&entry).unwrap();

        assert_eq!(cover, cache_path.to_string_lossy());
        assert!(!cover.ends_with("preview.gif"));

        let _ = std::fs::remove_file(cache_path);
        match previous_home {
            Some(value) => unsafe { std::env::set_var("HOME", value) },
            None => unsafe { std::env::remove_var("HOME") },
        }
    }
}
