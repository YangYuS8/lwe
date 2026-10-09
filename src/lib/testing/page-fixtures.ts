import type {
  ActionOutcome, DesktopPageSnapshot, LibraryItemDetail, LibraryPageSnapshot,
  SettingsPageSnapshot, WorkshopOnlineSearchResult, WorkshopPageSnapshot
} from '$lib/types';

export const librarySnapshot: LibraryPageSnapshot = {
  items: [{
    id: 'video-7', workshopId: '7', title: 'Forest Video', itemType: 'video',
    coverPath: null, ageRating: 'g', source: 'workshop',
    compatibility: { badge: 'fully_supported', reasonCode: 'ready_for_library', summaryCopy: 'Ready to use' },
    applySupported: true, favorite: false, assignedMonitorLabels: ['Primary']
  }],
  selectedItemId: 'video-7', monitorsAvailable: true, desktopAssignmentsAvailable: true, stale: false
};

export const libraryDetail: LibraryItemDetail = {
  id: 'video-7', title: 'Forest Video', itemType: 'video', coverPath: null, source: 'workshop',
  compatibility: {
    badge: 'fully_supported', reasonCode: 'ready_for_library', summaryCopy: 'Ready to use',
    headline: 'Ready to use', detail: 'Locally synchronized video.', nextStep: 'none', nextStepCopy: null
  },
  applySupported: true, monitorsAvailable: true, desktopAssignmentsAvailable: true,
  assignedMonitorLabels: ['Primary'], description: 'A forest wallpaper.', tags: ['Nature']
};

export const desktopSnapshot: DesktopPageSnapshot = {
  monitors: [{
    monitorId: 'DP-1', displayName: 'Primary', resolution: '1920×1080',
    currentWallpaperTitle: 'Forest Video', currentCoverPath: null, currentItemId: 'video-7',
    clearSupported: true, runtimeStatus: 'running'
  }],
  missingMonitorRestores: [], monitorsAvailable: true, assignmentsAvailable: true, stale: false
};

export const settingsSnapshot: SettingsPageSnapshot = {
  language: 'en', theme: 'light', launchOnLogin: false, launchOnLoginAvailable: true,
  steamWebApiKey: '', workshopQuery: '', workshopAgeRatings: ['g', 'pg_13'],
  workshopItemTypes: ['video', 'scene', 'web', 'application'], steamRequired: false,
  steamStatusMessage: 'Steam is available.', stale: false
};

export const workshopSnapshot: WorkshopPageSnapshot = {
  items: [], selectedItemId: null, stale: false
};

export const searchResult: WorkshopOnlineSearchResult = {
  query: 'forest', page: 2, pageSize: 24, totalApprox: 50, hasMore: true,
  items: [{
    id: '7', title: 'Forest Online', previewUrl: null, tags: ['Nature'], itemType: 'video',
    ageRating: 'g', ageRatingReason: 'General audiences'
  }]
};

export const outcome = <T = null>(overrides: Partial<ActionOutcome<T>> = {}): ActionOutcome<T> => ({
  ok: true, message: null, shellPatch: null, currentUpdate: null, invalidations: [], ...overrides
});
