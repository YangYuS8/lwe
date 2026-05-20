import type { CopyDictionary } from '$lib/i18n';
import type { DesktopPageSnapshot } from '$lib/types';

type DesktopPageState = {
  monitorAvailabilityLabel: string;
  assignmentAvailabilityLabel: string;
  issueMessages: string[];
  emptyMessage: string | null;
};

export const resolveDesktopPageState = (
  snapshot: DesktopPageSnapshot,
  copyValue: CopyDictionary
): DesktopPageState => {
  const issueMessages: string[] = [];
  const desktopCopy = copyValue.desktop;

  if (snapshot.monitorDiscoveryIssue) {
    issueMessages.push(snapshot.monitorDiscoveryIssue);
  } else if (!snapshot.monitorsAvailable) {
    issueMessages.push(desktopCopy.discoveryUnavailable);
  }

  if (snapshot.persistenceIssue) {
    issueMessages.push(snapshot.persistenceIssue);
  } else if (!snapshot.assignmentsAvailable) {
    issueMessages.push(desktopCopy.assignmentPersistenceUnavailable);
  }

  if (snapshot.runtimeIssue) {
    issueMessages.push(snapshot.runtimeIssue);
  }

  issueMessages.push(...(snapshot.restoreIssues ?? []));

  return {
    monitorAvailabilityLabel: snapshot.monitorsAvailable ? desktopCopy.yes : desktopCopy.no,
    assignmentAvailabilityLabel: snapshot.assignmentsAvailable ? desktopCopy.yes : desktopCopy.no,
    issueMessages,
    emptyMessage: snapshot.monitors.length
      ? null
      : snapshot.monitorsAvailable
        ? desktopCopy.empty
        : desktopCopy.snapshotUnavailable
  };
};
