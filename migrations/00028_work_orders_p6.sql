-- P6 Autopilot control-surface upgrades (Contract C31).
--
-- 1. Owner-authored work orders (FR-FBR-22 extension): provenance FKs become
--    nullable — `recommendation_id IS NULL` ⇔ the order was authored by the
--    owner directly, not derived from an analyst recommendation (D-P6-2: the
--    discriminator is derivational; no `origin` column). The paired CHECK makes
--    half-null provenance unrepresentable.
-- 2. Named-runner routing (D-P6-3): `routing_label` targets a work order at a
--    specific runner identity (the runner token `sub`). NULL = any runner may
--    claim (first-claim-wins). Poll filtering + claim enforcement are
--    server-side against the verified token sub — coordination metadata, NOT a
--    trust boundary (a runner token still cannot author `approve`, C22 inv. 2).

ALTER TABLE work_orders
    ALTER COLUMN recommendation_id DROP NOT NULL,
    ALTER COLUMN cluster_id DROP NOT NULL,
    ADD COLUMN routing_label TEXT NULL
        CHECK (routing_label IS NULL OR length(routing_label) BETWEEN 1 AND 128),
    ADD CONSTRAINT work_orders_provenance_pair
        CHECK ((recommendation_id IS NULL) = (cluster_id IS NULL));
