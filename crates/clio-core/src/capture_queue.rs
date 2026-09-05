//! Read-only, client-local capture spool diagnostics. Never opens capture payloads.
use std::path::Path;

pub fn health(root: &Path) -> serde_json::Value {
    if !root.exists() {
        return serde_json::Value::Null;
    }
    let count = |bucket: &str| -> std::io::Result<(u64, Option<f64>)> {
        let mut n = 0;
        let mut oldest: Option<f64> = None;
        for entry in std::fs::read_dir(root.join(bucket))? {
            let entry = entry?;
            if !entry.file_type()?.is_file()
                || entry.path().extension().and_then(|e| e.to_str()) != Some("json")
            {
                continue;
            }
            n += 1;
            let age = entry
                .metadata()?
                .modified()?
                .elapsed()
                .unwrap_or_default()
                .as_secs_f64();
            oldest = Some(oldest.map_or(age, |o| o.max(age)));
        }
        Ok((n, oldest))
    };
    let mut result = serde_json::json!({"scope": "local", "checked_at": crate::models::now_utc(), "oldest_pending_age_secs": null});
    let mut unavailable = Vec::new();
    for bucket in ["pending", "processing", "dead"] {
        match count(bucket) {
            Ok((count, oldest)) => {
                result[bucket] = count.into();
                if bucket == "pending" {
                    result["oldest_pending_age_secs"] = serde_json::json!(oldest);
                }
            }
            Err(_) => {
                result[bucket] = serde_json::Value::Null;
                unavailable.push(bucket);
            }
        }
    }
    result["unavailable_buckets"] = serde_json::json!(unavailable);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn future_job_timestamps_count_normally_with_zero_age() {
        let root = tempfile::tempdir().unwrap();
        for bucket in ["pending", "processing", "dead"] {
            std::fs::create_dir(root.path().join(bucket)).unwrap();
            let file = std::fs::File::create(root.path().join(bucket).join("future.json")).unwrap();
            file.set_times(std::fs::FileTimes::new().set_modified(
                std::time::SystemTime::now() + std::time::Duration::from_secs(3600),
            ))
            .unwrap();
        }
        let result = health(root.path());
        for bucket in ["pending", "processing", "dead"] {
            assert_eq!(result[bucket], 1);
        }
        assert_eq!(result["oldest_pending_age_secs"], 0.0);
        assert_eq!(result["unavailable_buckets"], serde_json::json!([]));
    }

    #[test]
    fn missing_or_unreadable_buckets_are_unknown_and_only_regular_jobs_count() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("pending")).unwrap();
        std::fs::write(root.path().join("pending/job.json"), "synthetic").unwrap();
        std::fs::create_dir(root.path().join("pending/not-a-job.json")).unwrap();
        std::fs::write(root.path().join("processing"), "not a directory").unwrap();
        let result = health(root.path());
        assert_eq!(result["pending"], 1);
        assert!(result["processing"].is_null());
        assert!(result["dead"].is_null());
        assert_eq!(
            result["unavailable_buckets"],
            serde_json::json!(["processing", "dead"])
        );
        assert_eq!(result["scope"], "local");
        assert!(result["checked_at"].as_str().is_some());
        assert!(health(&root.path().join("absent")).is_null());
    }
}
