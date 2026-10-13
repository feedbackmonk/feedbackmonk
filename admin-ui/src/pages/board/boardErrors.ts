import axios from "axios";
import { isSignInRequired } from "../../shared/types.gen";

// How the board pages read a failed C43 request (Contract C43, FR-FBR-43).
//
// The two answers that are NOT failures must never be confused:
//   - 404 → the board is disabled, or the item is not on it (C29 inv. 2): a
//     terminal "not available" state;
//   - 401 `{"error":"SignInRequired"}` → the viewer must sign in: a
//     `signed_in` board read anonymously, or a write that needs a person.
// Any other error is a transport failure the page may retry.

export function isNotFound(err: unknown): boolean {
  return axios.isAxiosError(err) && err.response?.status === 404;
}

export function needsSignIn(err: unknown): boolean {
  return (
    axios.isAxiosError(err) &&
    err.response?.status === 401 &&
    isSignInRequired(err.response?.data)
  );
}

/** Retry policy for a board query: the two terminal answers never retry. */
export function retryBoardQuery(failureCount: number, err: unknown): boolean {
  if (isNotFound(err) || needsSignIn(err)) return false;
  return failureCount < 2;
}

export function boardPath(projectId: string): string {
  return `/public/projects/${encodeURIComponent(projectId)}/board`;
}

export function boardItemPath(projectId: string, shortCode: string): string {
  return `${boardPath(projectId)}/items/${encodeURIComponent(shortCode)}`;
}

export function prioritiesPath(projectId: string): string {
  return `${boardPath(projectId)}/priorities`;
}
