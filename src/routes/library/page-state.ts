import type { InvalidatedPage, LibraryItemDetail, LibraryPageSnapshot } from '$lib/types';

type LibraryAvailabilitySource = Pick<
  LibraryPageSnapshot,
  'monitorsAvailable' | 'monitorDiscoveryIssue' | 'desktopAssignmentsAvailable' | 'desktopAssignmentIssue'
>;

type LibraryPageState = {
  issueMessages: string[];
  emptyMessage: string | null;
};

type LibraryApplyRefreshState = {
  refreshLibrarySnapshot: boolean;
  refreshDesktopSnapshot: boolean;
  refreshLibraryDetailId: string | null;
};

export type LibraryCopy = {
  empty: string;
  monitorDiscoveryUnavailable: string;
  desktopAssignmentsUnavailable: string;
  desktopAssignmentDataUnavailable: string;
};

export const resolveLibraryApplyRefreshState = ({
  invalidations,
  selectedItemId,
  librarySnapshotRefreshSucceeded = true
}: {
  invalidations: InvalidatedPage[];
  selectedItemId: string | null;
  librarySnapshotRefreshSucceeded?: boolean;
}): LibraryApplyRefreshState => {
  const refreshLibrarySnapshot = invalidations.includes('library');
  const refreshDesktopSnapshot = invalidations.includes('desktop');
  const refreshLibraryDetailId =
    (refreshLibrarySnapshot || refreshDesktopSnapshot) && librarySnapshotRefreshSucceeded
      ? selectedItemId
      : null;

  return {
    refreshLibrarySnapshot,
    refreshDesktopSnapshot,
    refreshLibraryDetailId
  };
};

export const resolveLibraryAvailabilityIssues = (
  source: LibraryAvailabilitySource | LibraryItemDetail,
  copy: Pick<LibraryCopy, 'monitorDiscoveryUnavailable' | 'desktopAssignmentsUnavailable'>
): string[] => {
  const issueMessages: string[] = [];

  if (source.monitorDiscoveryIssue) {
    issueMessages.push(source.monitorDiscoveryIssue);
  } else if (!source.monitorsAvailable) {
    issueMessages.push(copy.monitorDiscoveryUnavailable);
  }

  if (source.desktopAssignmentIssue) {
    issueMessages.push(source.desktopAssignmentIssue);
  } else if (!source.desktopAssignmentsAvailable) {
    issueMessages.push(copy.desktopAssignmentsUnavailable);
  }

  return issueMessages;
};

export const resolveLibraryPageState = (
  snapshot: LibraryPageSnapshot,
  copy: LibraryCopy
): LibraryPageState => {
  const issueMessages = resolveLibraryAvailabilityIssues(snapshot, copy);
  let emptyMessage: string | null = null;

  if (!snapshot.items.length) {
    emptyMessage = copy.empty;

    if (!snapshot.desktopAssignmentsAvailable) {
      emptyMessage += ` ${copy.desktopAssignmentDataUnavailable}`;
    } else if (!snapshot.monitorsAvailable) {
      emptyMessage += ` ${copy.monitorDiscoveryUnavailable}`;
    }
  }

  return {
    issueMessages,
    emptyMessage
  };
};
