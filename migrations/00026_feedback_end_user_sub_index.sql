-- 00026: partial index on (project_id, end_user_sub) for the end-user hot path.
--
-- Scrutiny P2-14: the JWT-sub-scoped reads/erasure
-- (get_for_end_user / list_*_for_end_user / delete_for_end_user /
-- erase_all_for_end_user) filter `WHERE tenant_id AND project_id AND
-- end_user_sub`, but no index covered `end_user_sub` — the query fell back to a
-- project-wide scan then filtered row-by-row. GitCellar Desktop's "My Feedback"
-- list + poll + erase is a hot path; at sole-backend volume this degrades to a
-- near-scan.
--
-- Partial (WHERE end_user_sub IS NOT NULL) so it stays small: the anonymous
-- majority (end_user_sub NULL) is excluded — those rows are never reached
-- through the /me/* surface.
CREATE INDEX IF NOT EXISTS feedback_project_sub_idx
    ON feedback (project_id, end_user_sub)
    WHERE end_user_sub IS NOT NULL;
