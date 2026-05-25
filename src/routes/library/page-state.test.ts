import { describe, expect, it } from 'vitest';

import { getCopyForLanguage } from '$lib/i18n';
import { resolveLibraryApplyRefreshState, resolveLibraryPageState } from './page-state';

const libraryCopy = getCopyForLanguage('en').library;

describe('library page state', () => {
  it('reloads the refreshed library selection detail when apply invalidates library and desktop', () => {
    expect(
      resolveLibraryApplyRefreshState({
        invalidations: ['library', 'desktop'],
        selectedItemId: 'scene-7'
      })
    ).toEqual({
      refreshLibrarySnapshot: true,
      refreshDesktopSnapshot: true,
      refreshLibraryDetailId: 'scene-7'
    });
  });

  it('does not refresh selected detail from a stale library snapshot when library refresh fails', () => {
    expect(
      resolveLibraryApplyRefreshState({
        invalidations: ['library', 'desktop'],
        selectedItemId: 'scene-7',
        librarySnapshotRefreshSucceeded: false
      })
    ).toEqual({
      refreshLibrarySnapshot: true,
      refreshDesktopSnapshot: true,
      refreshLibraryDetailId: null
    });
  });

  it('refreshes selected detail when apply only invalidates desktop state', () => {
    expect(
      resolveLibraryApplyRefreshState({
        invalidations: ['desktop'],
        selectedItemId: 'scene-7'
      })
    ).toEqual({
      refreshLibrarySnapshot: false,
      refreshDesktopSnapshot: true,
      refreshLibraryDetailId: 'scene-7'
    });
  });

  it('surfaces desktop assignment degradation alongside an empty library snapshot', () => {
    expect(
      resolveLibraryPageState({
        items: [],
        selectedItemId: null,
        monitorsAvailable: false,
        monitorDiscoveryIssue: 'Monitor discovery is unavailable.',
        desktopAssignmentIssue: 'Desktop assignments are unavailable.',
        desktopAssignmentsAvailable: false,
        stale: true
      }, libraryCopy)
    ).toEqual({
      issueMessages: ['Monitor discovery is unavailable.', 'Desktop assignments are unavailable.'],
      emptyMessage:
        'No Library items are available in the current snapshot. Desktop assignment data is currently unavailable.'
    });
  });

  it('uses the neutral empty copy when assignment data is available', () => {
    expect(
      resolveLibraryPageState({
        items: [],
        selectedItemId: null,
        monitorsAvailable: true,
        monitorDiscoveryIssue: null,
        desktopAssignmentIssue: null,
        desktopAssignmentsAvailable: true,
        stale: false
      }, libraryCopy)
    ).toEqual({
      issueMessages: [],
      emptyMessage: 'No Library items are available in the current snapshot.'
    });
  });
});
