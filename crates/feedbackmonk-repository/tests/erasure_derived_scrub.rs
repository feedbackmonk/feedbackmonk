//! Scrutiny P0-1 — behavioral proof that `delete_for_end_user` erasure SCRUBS
//! the P5 analysis corpus (cluster summary / recommendation body / work-order
//! instructions / sweep digest). None of those tables FK-cascades from feedback
//! (they reference the CLUSTER, D1), so a bare `DELETE FROM feedback` would leave
//! the erased submitter's verbatim/paraphrased text alive in admin-readable
//! prose. Lives in the repository crate because it seeds the derived corpus with
//! raw SQL (the derived tables have no scope-layer create-with-text API, and the
//! `multi-tenant-isolation-check` oracle permits raw SQL only inside this crate).

use feedbackmonk_core::FeedbackKind;
use feedbackmonk_repository::{
    FeedbackRepo, ProjectRepo, SqlxFeedbackRepo, SqlxProjectRepo, SqlxTenantRepo, TenantRepo,
};
use sqlx::PgPool;
use uuid::Uuid;

const PII: &str = "SUPERSECRETPII777-user-typed-this";

#[sqlx::test(migrations = "../../migrations")]
async fn erasure_scrubs_p5_derived_text(pool: PgPool) {
    let trepo = SqlxTenantRepo::new(pool.clone());
    let prepo = SqlxProjectRepo::new(pool.clone());
    let frepo = SqlxFeedbackRepo::new(pool.clone());

    let t = trepo.create("scrub@example.com", "hash").await.unwrap();
    let tscope = trepo.scope_for(t.id).await.unwrap();
    let p = prepo.create(&tscope, "Proj", "p-scrub").await.unwrap();
    let pscope = prepo.open(&tscope, p.id).await.unwrap();
    let (tid, pid) = (pscope.tenant_id(), pscope.project_id());

    let fb = frepo
        .submit_authenticated(&pscope, "user-A", Some("a@x.com"), None, None, None, PII, None, FeedbackKind::Bug)
        .await
        .unwrap();
    let fb_uuid: Uuid = sqlx::query_scalar(
        "SELECT id FROM feedback WHERE tenant_id=$1 AND project_id=$2 AND short_code=$3",
    )
    .bind(tid).bind(pid).bind(fb.as_str())
    .fetch_one(&pool).await.unwrap();

    // Seed the derived corpus quoting the feedback body; point the feedback at
    // the cluster (as the analyst does on submit).
    let cluster_id: Uuid = sqlx::query_scalar(
        "INSERT INTO feedback_clusters (tenant_id, project_id, label, summary, priority_rationale) \
         VALUES ($1,$2,'orig-label',$3,$3) RETURNING id",
    )
    .bind(tid).bind(pid).bind(PII)
    .fetch_one(&pool).await.unwrap();
    sqlx::query("UPDATE feedback SET cluster_id=$1 WHERE id=$2")
        .bind(cluster_id).bind(fb_uuid).execute(&pool).await.unwrap();
    let sweep_id: Uuid = sqlx::query_scalar(
        "INSERT INTO analysis_sweeps (tenant_id, project_id, triggered_by, digest_summary) \
         VALUES ($1,$2,'schedule',$3) RETURNING id",
    )
    .bind(tid).bind(pid).bind(PII)
    .fetch_one(&pool).await.unwrap();
    let rec_id: Uuid = sqlx::query_scalar(
        "INSERT INTO recommendations (tenant_id, project_id, cluster_id, sweep_id, action_type, title, body, rationale) \
         VALUES ($1,$2,$3,$4,'bug_fix','rec-title',$5,$5) RETURNING id",
    )
    .bind(tid).bind(pid).bind(cluster_id).bind(sweep_id).bind(PII)
    .fetch_one(&pool).await.unwrap();
    sqlx::query(
        "INSERT INTO work_orders (tenant_id, project_id, recommendation_id, cluster_id, action_type, title, instructions, autonomy_rung) \
         VALUES ($1,$2,$3,$4,'bug_fix','wo-title',$5,0)",
    )
    .bind(tid).bind(pid).bind(rec_id).bind(cluster_id).bind(PII)
    .execute(&pool).await.unwrap();

    // Erase.
    let deleted = frepo.delete_for_end_user(&pscope, "user-A", &fb).await.unwrap();
    assert!(deleted, "the owner's feedback must be erased");

    // The feedback row is gone…
    let fb_count: i64 = sqlx::query_scalar("SELECT count(*) FROM feedback WHERE id=$1")
        .bind(fb_uuid).fetch_one(&pool).await.unwrap();
    assert_eq!(fb_count, 0, "feedback row must be erased");

    // …and NO derived table retains the PII.
    let cluster_summary: Option<String> =
        sqlx::query_scalar("SELECT summary FROM feedback_clusters WHERE id=$1")
            .bind(cluster_id).fetch_one(&pool).await.unwrap();
    assert_eq!(cluster_summary, None, "cluster summary must be scrubbed");
    let cluster_rationale: Option<String> =
        sqlx::query_scalar("SELECT priority_rationale FROM feedback_clusters WHERE id=$1")
            .bind(cluster_id).fetch_one(&pool).await.unwrap();
    assert_eq!(cluster_rationale, None, "cluster priority_rationale must be scrubbed");
    let rec_body: String = sqlx::query_scalar("SELECT body FROM recommendations WHERE id=$1")
        .bind(rec_id).fetch_one(&pool).await.unwrap();
    assert!(!rec_body.contains(PII), "recommendation body must be scrubbed: {rec_body:?}");
    let wo_instructions: String =
        sqlx::query_scalar("SELECT instructions FROM work_orders WHERE cluster_id=$1 LIMIT 1")
            .bind(cluster_id).fetch_one(&pool).await.unwrap();
    assert!(!wo_instructions.contains(PII), "work-order instructions must be scrubbed");
    let sweep_digest: Option<String> =
        sqlx::query_scalar("SELECT digest_summary FROM analysis_sweeps WHERE id=$1")
            .bind(sweep_id).fetch_one(&pool).await.unwrap();
    assert_eq!(sweep_digest, None, "sweep digest must be scrubbed");
}
