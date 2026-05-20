import { describe, expect, it } from 'vitest';

import { getCopyForLanguage } from '$lib/i18n';
import { resolveDesktopPageState } from './page-state';

const copy = getCopyForLanguage('en');

describe('desktop page state', () => {
  it('keeps the zh-CN missing filter label in the centralized dictionary', () => {
    expect(getCopyForLanguage('zh-CN').desktop.filterOptions.missing).toBe('缺失恢复项');
  });

  it('surfaces monitor discovery and persistence issues instead of treating the snapshot as empty', () => {
    expect(
      resolveDesktopPageState({
        monitors: [],
        missingMonitorRestores: [],
        restoreIssues: [],
        monitorsAvailable: false,
        monitorDiscoveryIssue: 'Monitor discovery is unavailable.',
        persistenceIssue: 'Assignment persistence is unavailable.',
        assignmentsAvailable: false,
        stale: true
      }, copy)
    ).toEqual({
      monitorAvailabilityLabel: 'no',
      assignmentAvailabilityLabel: 'no',
      issueMessages: ['Monitor discovery is unavailable.', 'Assignment persistence is unavailable.'],
      emptyMessage: 'Desktop monitor data is currently unavailable in this snapshot.'
    });
  });

  it('keeps the normal empty copy when monitor data is available but no monitors are present', () => {
    expect(
      resolveDesktopPageState({
        monitors: [],
        missingMonitorRestores: [],
        restoreIssues: [],
        monitorsAvailable: true,
        monitorDiscoveryIssue: null,
        persistenceIssue: null,
        assignmentsAvailable: true,
        stale: false
      }, copy)
    ).toEqual({
      monitorAvailabilityLabel: 'yes',
      assignmentAvailabilityLabel: 'yes',
      issueMessages: [],
      emptyMessage: 'No monitors are available in the current snapshot.'
    });
  });

  it('includes restore issues in the visible desktop issue list', () => {
    expect(
      resolveDesktopPageState({
        monitors: [],
        missingMonitorRestores: [],
        restoreIssues: ['Saved assignment for missing monitor DISPLAY-3 still points to Forest Scene (scene-7).'],
        monitorsAvailable: true,
        monitorDiscoveryIssue: null,
        persistenceIssue: null,
        assignmentsAvailable: true,
        stale: true
      }, copy)
    ).toEqual({
      monitorAvailabilityLabel: 'yes',
      assignmentAvailabilityLabel: 'yes',
      issueMessages: ['Saved assignment for missing monitor DISPLAY-3 still points to Forest Scene (scene-7).'],
      emptyMessage: 'No monitors are available in the current snapshot.'
    });
  });

  it('includes runtime issues before restore issues in the visible desktop issue list', () => {
    expect(
      resolveDesktopPageState({
        monitors: [],
        missingMonitorRestores: [],
        restoreIssues: ['Saved assignment could not be restored.'],
        runtimeIssue: 'Desktop runtime status is unavailable.',
        monitorsAvailable: true,
        monitorDiscoveryIssue: null,
        persistenceIssue: null,
        assignmentsAvailable: true,
        stale: true
      }, copy)
    ).toEqual({
      monitorAvailabilityLabel: 'yes',
      assignmentAvailabilityLabel: 'yes',
      issueMessages: ['Desktop runtime status is unavailable.', 'Saved assignment could not be restored.'],
      emptyMessage: 'No monitors are available in the current snapshot.'
    });
  });
});
