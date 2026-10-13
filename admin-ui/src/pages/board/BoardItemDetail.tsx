// Board detail view (FR-FBR-57/58/59, Contract C43).
//
// CONTRACT STUB — the `contracts` piece froze the route and props; the
// `public-ui` piece builds the page against `fetchBoardItemDetail` (and, for a
// signed-in viewer, the vote / not-needed / priority actions). It must render
// only the C43 detail shape: body verbatim, kind, status, votes, accepted date,
// the owner's board note, and the status timeline (status + date only).
export interface BoardItemDetailProps {
  projectId: string;
  shortCode: string;
}

export function BoardItemDetail(_props: BoardItemDetailProps) {
  return null;
}
