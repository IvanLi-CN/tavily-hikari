fn format_db_compaction_report_message(report: &tavily_hikari::DbCompactionReport) -> String {
    let total_database_bytes_before =
        report.before.database_bytes + report.before.observability_database_bytes;
    let total_database_bytes_after =
        report.after.database_bytes + report.after.observability_database_bytes;
    let total_wal_bytes_before = report.before.wal_bytes + report.before.observability_wal_bytes;
    let total_wal_bytes_after = report.after.wal_bytes + report.after.observability_wal_bytes;
    let total_freelist_before = report
        .before
        .freelist_count
        .saturating_add(report.before.observability_freelist_count);
    let total_freelist_after = report
        .after
        .freelist_count
        .saturating_add(report.after.observability_freelist_count);

    if report.skipped {
        format!(
            "skipped=true forced={} reason={} database_bytes_before={} database_bytes_after={} observability_database_bytes_before={} observability_database_bytes_after={} total_database_bytes_before={} total_database_bytes_after={} wal_bytes_before={} wal_bytes_after={} observability_wal_bytes_before={} observability_wal_bytes_after={} total_wal_bytes_before={} total_wal_bytes_after={} total_reclaimable_bytes_before={} total_reclaimable_bytes_after={} total_reclaimable_ratio_before={:.6} total_reclaimable_ratio_after={:.6} reclaimable_bytes_before={} reclaimable_bytes_after={} reclaimable_ratio_before={:.6} reclaimable_ratio_after={:.6} observability_reclaimable_bytes_before={} observability_reclaimable_bytes_after={} observability_reclaimable_ratio_before={:.6} observability_reclaimable_ratio_after={:.6} freelist_before={} freelist_after={} observability_freelist_before={} observability_freelist_after={} total_freelist_before={} total_freelist_after={}",
            report.forced,
            report.reason.as_deref().unwrap_or("unknown"),
            report.before.database_bytes,
            report.after.database_bytes,
            report.before.observability_database_bytes,
            report.after.observability_database_bytes,
            total_database_bytes_before,
            total_database_bytes_after,
            report.before.wal_bytes,
            report.after.wal_bytes,
            report.before.observability_wal_bytes,
            report.after.observability_wal_bytes,
            total_wal_bytes_before,
            total_wal_bytes_after,
            report.before.total_reclaimable_bytes,
            report.after.total_reclaimable_bytes,
            report.before.total_reclaimable_ratio,
            report.after.total_reclaimable_ratio,
            report.before.reclaimable_bytes,
            report.after.reclaimable_bytes,
            report.before.reclaimable_ratio,
            report.after.reclaimable_ratio,
            report.before.observability_reclaimable_bytes,
            report.after.observability_reclaimable_bytes,
            report.before.observability_reclaimable_ratio,
            report.after.observability_reclaimable_ratio,
            report.before.freelist_count,
            report.after.freelist_count,
            report.before.observability_freelist_count,
            report.after.observability_freelist_count,
            total_freelist_before,
            total_freelist_after
        )
    } else {
        format!(
            "skipped=false forced={} database_bytes_before={} database_bytes_after={} observability_database_bytes_before={} observability_database_bytes_after={} total_database_bytes_before={} total_database_bytes_after={} wal_bytes_before={} wal_bytes_after={} observability_wal_bytes_before={} observability_wal_bytes_after={} total_wal_bytes_before={} total_wal_bytes_after={} total_reclaimable_bytes_before={} total_reclaimable_bytes_after={} total_reclaimable_ratio_before={:.6} total_reclaimable_ratio_after={:.6} reclaimable_bytes_before={} reclaimable_bytes_after={} reclaimable_ratio_before={:.6} reclaimable_ratio_after={:.6} observability_reclaimable_bytes_before={} observability_reclaimable_bytes_after={} observability_reclaimable_ratio_before={:.6} observability_reclaimable_ratio_after={:.6} freelist_before={} freelist_after={} observability_freelist_before={} observability_freelist_after={} total_freelist_before={} total_freelist_after={}",
            report.forced,
            report.before.database_bytes,
            report.after.database_bytes,
            report.before.observability_database_bytes,
            report.after.observability_database_bytes,
            total_database_bytes_before,
            total_database_bytes_after,
            report.before.wal_bytes,
            report.after.wal_bytes,
            report.before.observability_wal_bytes,
            report.after.observability_wal_bytes,
            total_wal_bytes_before,
            total_wal_bytes_after,
            report.before.total_reclaimable_bytes,
            report.after.total_reclaimable_bytes,
            report.before.total_reclaimable_ratio,
            report.after.total_reclaimable_ratio,
            report.before.reclaimable_bytes,
            report.after.reclaimable_bytes,
            report.before.reclaimable_ratio,
            report.after.reclaimable_ratio,
            report.before.observability_reclaimable_bytes,
            report.after.observability_reclaimable_bytes,
            report.before.observability_reclaimable_ratio,
            report.after.observability_reclaimable_ratio,
            report.before.freelist_count,
            report.after.freelist_count,
            report.before.observability_freelist_count,
            report.after.observability_freelist_count,
            total_freelist_before,
            total_freelist_after
        )
    }
}
