use crate::models::{
    WorkshopAgeRating, WorkshopOnlineItem, WorkshopOnlineItemType, WorkshopOnlineSearchInput,
    WorkshopOnlineSearchResult,
};
use crate::results::settings_persistence::SettingsPersistenceLoad;
use crate::results::workshop::{WorkshopInspection, WorkshopRefreshResult};
use crate::services::compatibility_service::CompatibilityService;
use crate::services::settings_persistence_service::SettingsPersistenceService;
use lwe_library::{SteamLibrary, WorkshopCatalogEntry, WorkshopScanner};
use serde_json::Value;
use std::sync::{Mutex, OnceLock};

static WORKSHOP_REFRESH_CACHE: OnceLock<Mutex<Option<WorkshopRefreshResult>>> = OnceLock::new();

fn workshop_refresh_cache() -> &'static Mutex<Option<WorkshopRefreshResult>> {
    WORKSHOP_REFRESH_CACHE.get_or_init(|| Mutex::new(None))
}

pub struct WorkshopService;

impl WorkshopService {
    fn cached_refresh() -> Option<WorkshopRefreshResult> {
        workshop_refresh_cache()
            .lock()
            .ok()
            .and_then(|cache| cache.as_ref().cloned())
            .map(|mut cached| {
                cached.served_from_snapshot = true;
                cached
            })
    }

    fn store_refresh_cache(result: &WorkshopRefreshResult) {
        if let Ok(mut cache) = workshop_refresh_cache().lock() {
            let mut cached = result.clone();
            cached.served_from_snapshot = false;
            *cache = Some(cached);
        }
    }

    pub fn invalidate_snapshot() {
        if let Ok(mut cache) = workshop_refresh_cache().lock() {
            *cache = None;
        }
    }

    fn truthy_value(value: &Value) -> bool {
        match value {
            Value::Bool(flag) => *flag,
            Value::Number(number) => number.as_u64().unwrap_or(0) > 0,
            Value::String(text) => {
                let normalized = text.trim().to_ascii_lowercase();
                normalized == "1" || normalized == "true" || normalized == "yes"
            }
            _ => false,
        }
    }

    fn load_steam_web_api_key() -> Result<String, String> {
        let persistence = SettingsPersistenceService::for_user_path()?;
        let settings = match persistence.load_settings() {
            SettingsPersistenceLoad::Loaded(settings) => settings,
            SettingsPersistenceLoad::Unavailable { reason } => return Err(reason),
        };

        if settings.steam_web_api_key.trim().is_empty() {
            return Err(
                "Steam Web API key is required for online Workshop search. Add it in Settings."
                    .to_string(),
            );
        }

        Ok(settings.steam_web_api_key)
    }

    fn marker_score(haystack: &str, markers: &[&str]) -> Vec<String> {
        markers
            .iter()
            .filter(|marker| haystack.contains(**marker))
            .map(|marker| marker.to_string())
            .collect()
    }

    fn infer_item_type(
        tags: &[String],
        metadata: Option<&str>,
        title: &str,
    ) -> WorkshopOnlineItemType {
        let mut blob = String::new();
        blob.push_str(&tags.join(" ").to_lowercase());
        blob.push(' ');
        blob.push_str(&title.to_lowercase());
        if let Some(metadata) = metadata {
            blob.push(' ');
            blob.push_str(&metadata.to_lowercase());
        }

        let application_markers = [
            "application",
            "app",
            "program",
            "software",
            "utility",
            "executable",
        ];
        if !Self::marker_score(&blob, &application_markers).is_empty() {
            return WorkshopOnlineItemType::Application;
        }

        let web_markers = ["web", "browser", "html", "website"];
        if !Self::marker_score(&blob, &web_markers).is_empty() {
            return WorkshopOnlineItemType::Web;
        }

        let scene_markers = ["scene", "3d", "particle", "environment"];
        if !Self::marker_score(&blob, &scene_markers).is_empty() {
            return WorkshopOnlineItemType::Scene;
        }

        WorkshopOnlineItemType::Video
    }

    fn infer_age_rating(
        tags: &[String],
        metadata: Option<&str>,
        title: &str,
        short_description: Option<&str>,
        maybe_inappropriate_sex: bool,
        maybe_inappropriate_violence: bool,
    ) -> (WorkshopAgeRating, String) {
        if maybe_inappropriate_sex {
            return (
                WorkshopAgeRating::R18,
                "Steam metadata flagged sexual/inappropriate content".to_string(),
            );
        }

        if maybe_inappropriate_violence {
            return (
                WorkshopAgeRating::Pg13,
                "Steam metadata flagged violent/inappropriate content".to_string(),
            );
        }

        let mut blob = String::new();
        blob.push_str(&tags.join(" ").to_lowercase());
        blob.push(' ');
        blob.push_str(&title.to_lowercase());
        if let Some(metadata) = metadata {
            blob.push(' ');
            blob.push_str(&metadata.to_lowercase());
        }
        if let Some(short_description) = short_description {
            blob.push(' ');
            blob.push_str(&short_description.to_lowercase());
        }

        let explicit_markers = [
            "nsfw",
            "adult",
            "explicit",
            "porn",
            "nude",
            "nudity",
            "sexual",
            "sex",
            "erotic",
            "lewd",
            "fetish",
            "hentai",
            "ecchi",
            "r18",
            "r-18",
            "18+",
            "18 plus",
            "x-rated",
            "mature sexual",
            "成人",
            "色情",
            "裸露",
            "性暗示",
            "限制级",
        ];
        let explicit_hits = Self::marker_score(&blob, &explicit_markers);
        if !explicit_hits.is_empty() {
            return (
                WorkshopAgeRating::R18,
                format!(
                    "Contains explicit adult markers: {}",
                    explicit_hits.join(", ")
                ),
            );
        }

        let mature_markers = [
            "mature",
            "suggestive",
            "violence",
            "violent",
            "blood",
            "gore",
            "horror",
            "disturbing",
            "questionable",
            "teen",
            "dark theme",
            "sensitive",
            "暴力",
            "血腥",
            "惊悚",
            "恐怖",
            "家长指导",
            "pg-13",
        ];
        let mature_hits = Self::marker_score(&blob, &mature_markers);
        if !mature_hits.is_empty() {
            return (
                WorkshopAgeRating::Pg13,
                format!(
                    "Contains mature content markers: {}",
                    mature_hits.join(", ")
                ),
            );
        }

        (
            WorkshopAgeRating::G,
            "No mature or explicit content markers were detected".to_string(),
        )
    }

    fn matches_filters(
        item: &WorkshopOnlineItem,
        age_ratings: &[WorkshopAgeRating],
        item_types: &[WorkshopOnlineItemType],
    ) -> bool {
        age_ratings.contains(&item.age_rating) && item_types.contains(&item.item_type)
    }

    fn parse_online_items(
        payload: &Value,
        input: &WorkshopOnlineSearchInput,
    ) -> Vec<WorkshopOnlineItem> {
        payload["response"]["publishedfiledetails"]
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .map(|entry| {
                        let id = entry["publishedfileid"]
                            .as_str()
                            .unwrap_or_default()
                            .to_string();
                        let title = entry["title"].as_str().unwrap_or("Untitled").to_string();
                        let metadata = entry["metadata"].as_str().map(str::to_string);
                        let short_description =
                            entry["short_description"].as_str().map(str::to_string);
                        let maybe_inappropriate_sex =
                            Self::truthy_value(&entry["maybe_inappropriate_sex"]);
                        let maybe_inappropriate_violence =
                            Self::truthy_value(&entry["maybe_inappropriate_violence"]);
                        let tags = entry["tags"]
                            .as_array()
                            .map(|tags| {
                                tags.iter()
                                    .filter_map(|tag| {
                                        tag["tag"]
                                            .as_str()
                                            .or_else(|| tag["display_name"].as_str())
                                            .map(str::to_string)
                                    })
                                    .collect::<Vec<_>>()
                            })
                            .unwrap_or_default();
                        let preview_url = entry["preview_url"]
                            .as_str()
                            .map(str::to_string)
                            .or_else(|| {
                                entry["previews"]
                                    .as_array()
                                    .and_then(|previews| previews.first())
                                    .and_then(|preview| preview["url"].as_str())
                                    .map(str::to_string)
                            });

                        let inferred_type =
                            Self::infer_item_type(&tags, metadata.as_deref(), &title);
                        let (age_rating, age_rating_reason) = Self::infer_age_rating(
                            &tags,
                            metadata.as_deref(),
                            &title,
                            short_description.as_deref(),
                            maybe_inappropriate_sex,
                            maybe_inappropriate_violence,
                        );

                        WorkshopOnlineItem {
                            id,
                            title,
                            preview_url,
                            tags,
                            item_type: inferred_type,
                            age_rating,
                            age_rating_reason,
                        }
                    })
                    .filter(|item| {
                        Self::matches_filters(item, &input.age_ratings, &input.item_types)
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    fn infer_local_entry_age_rating(
        entry: &crate::results::workshop::AssessedWorkshopCatalogEntry,
    ) -> String {
        let mut blob = String::new();
        blob.push_str(&entry.entry.title.to_lowercase());

        if let Some(description) = &entry.project_metadata.description {
            blob.push(' ');
            blob.push_str(&description.to_lowercase());
        }

        if !entry.project_metadata.tags.is_empty() {
            blob.push(' ');
            blob.push_str(&entry.project_metadata.tags.join(" ").to_lowercase());
        }

        if blob.contains("nsfw")
            || blob.contains("adult")
            || blob.contains("r18")
            || blob.contains("explicit")
            || blob.contains("色情")
            || blob.contains("成人")
        {
            return "r_18".to_string();
        }

        if blob.contains("mature")
            || blob.contains("suggestive")
            || blob.contains("violent")
            || blob.contains("blood")
            || blob.contains("暴力")
            || blob.contains("血腥")
        {
            return "pg_13".to_string();
        }

        "g".to_string()
    }

    fn parse_total_results(payload: &Value) -> Option<u32> {
        payload["response"]["total"]
            .as_u64()
            .map(|value| value as u32)
    }

    fn normalized_pagination(input: &WorkshopOnlineSearchInput) -> (u32, u32) {
        let page = input.page.max(1);
        let page_size = input.page_size.clamp(12, 96);
        (page, page_size)
    }

    pub fn search_online(
        input: WorkshopOnlineSearchInput,
    ) -> Result<WorkshopOnlineSearchResult, String> {
        let api_key = Self::load_steam_web_api_key()?;
        let query = input.query.trim().to_string();
        let (page, page_size) = Self::normalized_pagination(&input);
        let client = reqwest::blocking::Client::new();
        let mut request = client
            .get("https://api.steampowered.com/IPublishedFileService/QueryFiles/v1/")
            .query(&[
                ("key", api_key.as_str()),
                ("appid", "431960"),
                ("page", &page.to_string()),
                ("numperpage", &page_size.to_string()),
                ("return_tags", "1"),
                ("return_metadata", "1"),
                ("return_previews", "1"),
                ("return_short_description", "1"),
                ("return_vote_data", "1"),
                ("return_for_sale_data", "1"),
            ]);

        if !query.is_empty() {
            request = request.query(&[("search_text", query.as_str())]);
        }

        let response = request
            .send()
            .map_err(|error| format!("Failed to call Steam Workshop QueryFiles: {error}"))?;

        let payload = response
            .error_for_status()
            .map_err(|error| format!("Steam Workshop QueryFiles returned an error: {error}"))?
            .json::<Value>()
            .map_err(|error| {
                format!("Failed to parse Steam Workshop QueryFiles response: {error}")
            })?;

        let total_approx = Self::parse_total_results(&payload);
        let items = Self::parse_online_items(&payload, &input);
        let has_more = total_approx
            .map(|total| page.saturating_mul(page_size) < total)
            .unwrap_or(false);

        Ok(WorkshopOnlineSearchResult {
            query,
            page,
            page_size,
            has_more,
            total_approx,
            items,
        })
    }

    fn scan_catalog() -> Result<Vec<WorkshopCatalogEntry>, String> {
        let steam = SteamLibrary::discover()
            .map_err(|error| format!("Steam Workshop is unavailable: {error}"))?;
        if !steam.has_wallpaper_engine() {
            return Err(
                "Wallpaper Engine Workshop content is unavailable on this machine".to_string(),
            );
        }

        let mut scanner = WorkshopScanner::new(steam);

        scanner
            .scan_catalog()
            .map_err(|error| format!("Failed to scan the Steam Workshop catalog: {error}"))
    }

    pub fn refresh_catalog() -> Result<WorkshopRefreshResult, String> {
        let mut assessed = CompatibilityService::assess_catalog_entries(Self::scan_catalog()?);

        for entry in &mut assessed {
            if entry.project_metadata.inferred_age_rating.is_none() {
                entry.project_metadata.inferred_age_rating =
                    Some(Self::infer_local_entry_age_rating(entry));
            }
        }

        let result = WorkshopRefreshResult {
            catalog_entries: assessed,
            library_refresh_required: true,
            served_from_snapshot: false,
        };

        Self::store_refresh_cache(&result);

        Ok(result)
    }

    pub fn load_catalog_snapshot() -> Result<WorkshopRefreshResult, String> {
        Self::cached_refresh().ok_or_else(|| {
            "Workshop snapshot is not warm yet; a catalog refresh is required".to_string()
        })
    }

    pub fn inspect_item(workshop_id: &str) -> Result<WorkshopInspection, String> {
        let entry = Self::load_catalog_snapshot()
            .or_else(|_| Self::refresh_catalog())?
            .catalog_entries
            .into_iter()
            .find(|entry| entry.entry.workshop_id.to_string() == workshop_id)
            .ok_or_else(|| format!("Workshop item {workshop_id} not found"))?;

        Ok(WorkshopInspection {
            requested_workshop_id: workshop_id.to_string(),
            entry,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::WorkshopService;
    use crate::models::{WorkshopAgeRating, WorkshopOnlineItemType};
    use crate::results::workshop::WorkshopRefreshResult;
    use serde_json::json;

    #[test]
    fn service_layer_workshop_service_returns_application_result_not_page_snapshot() {
        let result = WorkshopRefreshResult {
            catalog_entries: Vec::new(),
            library_refresh_required: true,
            served_from_snapshot: false,
        };

        assert!(result.library_refresh_required);
        assert_eq!(result.catalog_entries.len(), 0);
    }

    #[test]
    fn workshop_snapshot_cache_roundtrip_marks_snapshot_reads() {
        WorkshopService::invalidate_snapshot();

        assert!(WorkshopService::load_catalog_snapshot().is_err());

        WorkshopService::store_refresh_cache(&WorkshopRefreshResult {
            catalog_entries: Vec::new(),
            library_refresh_required: false,
            served_from_snapshot: false,
        });

        let snapshot = WorkshopService::load_catalog_snapshot().unwrap();
        assert!(snapshot.served_from_snapshot);
        assert!(!snapshot.library_refresh_required);

        WorkshopService::invalidate_snapshot();
    }

    #[test]
    fn online_item_type_heuristic_detects_application_from_markers() {
        let item_type = WorkshopService::infer_item_type(
            &["utility".to_string()],
            Some("desktop application"),
            "Tool",
        );
        assert_eq!(item_type, WorkshopOnlineItemType::Application);
    }

    #[test]
    fn online_age_rating_heuristic_returns_reason() {
        let (rating, reason) = WorkshopService::infer_age_rating(
            &["nsfw".to_string()],
            Some("adult"),
            "Example",
            None,
            false,
            false,
        );
        assert_eq!(rating, WorkshopAgeRating::R18);
        assert!(reason.contains("explicit adult markers"));
    }

    #[test]
    fn parse_online_items_applies_type_and_age_filters() {
        let payload = json!({
            "response": {
                "publishedfiledetails": [
                    {
                        "publishedfileid": "1",
                        "title": "Safe Scene",
                        "metadata": "scene",
                        "tags": [{ "tag": "scene" }],
                        "preview_url": "https://example.com/1.jpg"
                    },
                    {
                        "publishedfileid": "2",
                        "title": "Adult Video",
                        "metadata": "nsfw video",
                        "tags": [{ "tag": "video" }],
                        "preview_url": "https://example.com/2.jpg"
                    }
                ]
            }
        });

        let items = WorkshopService::parse_online_items(
            &payload,
            &crate::models::WorkshopOnlineSearchInput {
                query: "video".to_string(),
                age_ratings: vec![WorkshopAgeRating::G],
                item_types: vec![WorkshopOnlineItemType::Scene],
                page: 1,
                page_size: 24,
            },
        );

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, "1");
        assert_eq!(items[0].item_type, WorkshopOnlineItemType::Scene);
        assert_eq!(items[0].age_rating, WorkshopAgeRating::G);
    }
}
