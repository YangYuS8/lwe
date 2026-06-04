export type InvalidatedPage = 'library' | 'workshop' | 'desktop' | 'settings';
export type ItemType = 'video' | 'scene' | 'web' | 'application' | 'other';
export type WorkshopAgeRating = 'g' | 'pg_13' | 'r_18';
export type WorkshopOnlineItemType = 'video' | 'scene' | 'web' | 'application';
export type WorkshopSyncStatus = 'synced' | 'missing_project' | 'missing_asset' | 'unsupported_type';
export type CompatibilityBadge = 'fully_supported' | 'partially_supported' | 'unsupported';
export type CompatibilityNextStep =
  | 'none'
  | 'open_in_steam'
  | 'resync_workshop_item'
  | 'wait_for_future_support';
export type LibrarySource = 'local' | 'workshop' | 'core' | 'other';
export type RuntimeStatus = 'running' | 'idle' | 'unsupported' | 'error';
export type DesktopRestoreState = 'restored' | 'missing_monitor' | 'missing_item' | 'unavailable';

export interface DesktopMissingMonitorRestore {
  monitorId: string;
  currentItemId: string;
  currentWallpaperTitle: string | null;
  restoreState: DesktopRestoreState;
  restoreIssue?: string | null;
}

export interface CompatibilityBaseModel {
  badge: CompatibilityBadge;
  reasonCode: string;
}

export interface CompatibilitySummaryModel extends CompatibilityBaseModel {
  summaryCopy: string;
}

export interface CompatibilityExplanationModel extends CompatibilityBaseModel {
  summaryCopy: string;
  headline: string;
  detail: string;
  nextStep: CompatibilityNextStep;
  nextStepCopy: string | null;
}

export interface AppShellSnapshot {
  appName: string;
  codeName: string;
  steamAvailable: boolean;
  libraryCount: number | null;
  workshopSyncedCount: number | null;
  monitorCount: number | null;
}

export interface WorkshopItemSummary {
  id: string;
  title: string;
  itemType: ItemType;
  coverPath: string | null;
  syncStatus: WorkshopSyncStatus;
  compatibility: CompatibilitySummaryModel;
}

export interface WorkshopPageSnapshot {
  items: WorkshopItemSummary[];
  selectedItemId: string | null;
  servedFromSnapshot?: boolean;
  stale: boolean;
}

export interface WorkshopItemDetail {
  id: string;
  title: string;
  itemType: ItemType;
  coverPath: string | null;
  syncStatus: WorkshopSyncStatus;
  compatibility: CompatibilityExplanationModel;
  tags: string[];
  description: string | null;
}

export interface WorkshopOnlineSearchInput {
  query: string;
  ageRatings: WorkshopAgeRating[];
  itemTypes: WorkshopOnlineItemType[];
  page: number;
  pageSize: number;
}

export interface WorkshopOnlineItem {
  id: string;
  title: string;
  previewUrl: string | null;
  tags: string[];
  itemType: WorkshopOnlineItemType;
  ageRating: WorkshopAgeRating;
  ageRatingReason: string;
}

export interface WorkshopOnlineSearchResult {
  query: string;
  page: number;
  pageSize: number;
  hasMore: boolean;
  totalApprox: number | null;
  items: WorkshopOnlineItem[];
}

export interface LibraryItemSummary {
  id: string;
  workshopId: string;
  title: string;
  itemType: ItemType;
  coverPath: string | null;
  ageRating: WorkshopAgeRating;
  source: LibrarySource;
  compatibility: CompatibilitySummaryModel;
  applySupported: boolean;
  favorite: boolean;
  assignedMonitorLabels?: string[];
}

export interface LibraryPageSnapshot {
  items: LibraryItemSummary[];
  selectedItemId: string | null;
  monitorsAvailable: boolean;
  monitorDiscoveryIssue?: string | null;
  desktopAssignmentIssue?: string | null;
  desktopAssignmentsAvailable: boolean;
  servedFromSnapshot?: boolean;
  stale: boolean;
}

export interface LibraryItemDetail {
  id: string;
  title: string;
  itemType: ItemType;
  coverPath: string | null;
  source: LibrarySource;
  compatibility: CompatibilityExplanationModel;
  applySupported: boolean;
  monitorsAvailable: boolean;
  monitorDiscoveryIssue?: string | null;
  desktopAssignmentIssue?: string | null;
  desktopAssignmentsAvailable: boolean;
  assignedMonitorLabels?: string[];
  description: string | null;
  tags: string[];
}

export interface DesktopMonitorSummary {
  monitorId: string;
  displayName: string;
  resolution: string;
  currentWallpaperTitle: string | null;
  currentCoverPath: string | null;
  currentItemId?: string | null;
  clearSupported: boolean;
  restoreState?: DesktopRestoreState | null;
  restoreIssue?: string | null;
  runtimeStatus: RuntimeStatus;
}

export interface DesktopPageSnapshot {
  monitors: DesktopMonitorSummary[];
  missingMonitorRestores: DesktopMissingMonitorRestore[];
  monitorsAvailable: boolean;
  monitorDiscoveryIssue?: string | null;
  persistenceIssue?: string | null;
  assignmentsAvailable: boolean;
  restoreIssues?: string[];
  runtimeIssue?: string | null;
  stale: boolean;
}

export interface SettingsPageSnapshot {
  language: string;
  theme: string;
  launchOnLogin: boolean;
  launchOnLoginAvailable: boolean;
  steamWebApiKey: string;
  workshopQuery: string;
  workshopAgeRatings: WorkshopAgeRating[];
  workshopItemTypes: WorkshopOnlineItemType[];
  steamRequired: boolean;
  steamStatusMessage: string;
  stale: boolean;
}

export interface SettingsUpdateInput {
  language?: string | null;
  theme?: string | null;
  launchOnLogin?: boolean | null;
  steamWebApiKey?: string | null;
  workshopQuery?: string | null;
  workshopAgeRatings?: WorkshopAgeRating[] | null;
  workshopItemTypes?: WorkshopOnlineItemType[] | null;
}

export interface DiagnosticsSnapshot {
  text: string;
}

export interface AppShellPatch {
  workshopSyncedCount?: number;
  libraryCount?: number;
  monitorCount?: number;
}

export interface ActionOutcome<T> {
  ok: boolean;
  message: string | null;
  shellPatch: AppShellPatch | null;
  currentUpdate: T | null;
  invalidations: InvalidatedPage[];
}
