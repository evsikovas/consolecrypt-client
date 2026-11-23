use super::*;

impl DockerClient {
    pub async fn image_names(&self) -> Result<Vec<String>, DockerError> {
        let list = self.json(Method::GET, "/images/json", None).await?;
        let items = list.as_array().ok_or(DockerError::Protocol)?;
        let mut names = Vec::new();
        for image in items {
            let before = names.len();
            if let Some(tags) = image["RepoTags"].as_array() {
                names.extend(tags.iter().filter_map(Value::as_str).map(str::to_owned));
            }
            if names.len() == before {
                names.extend(image["Id"].as_str().map(str::to_owned));
            }
        }
        names.sort();
        Ok(names)
    }

    pub async fn telemetry(&self, id: &str) -> Result<Value, DockerError> {
        let id = checked(id)?;
        let stats_path = format!("/containers/{id}/stats?stream=false");
        // Ask for the executable name only. Process arguments may contain credentials.
        let top_path = format!("/containers/{id}/top?ps_args=-eo%20pid,user,pcpu,pmem,etime,comm");
        let (stats, top) = tokio::join!(
            self.json(Method::GET, &stats_path, None),
            self.json(Method::GET, &top_path, None)
        );
        if stats.is_err() && top.is_err() {
            return Err(DockerError::Connection);
        }
        let stats = stats.unwrap_or(Value::Null);
        let top = top.ok().and_then(|v| {
            let titles = v["Titles"].as_array()?;
            if titles.len() != 6 || !matches!(titles[5].as_str(), Some("COMMAND" | "CMD" | "COMM"))
            {
                return None;
            }
            let rows = v["Processes"]
                .as_array()?
                .iter()
                .take(256)
                .filter_map(|row| {
                    let fields = row.as_array()?;
                    if fields.len() != 6 || fields[0].as_str()?.parse::<u64>().is_err() {
                        return None;
                    }
                    fields
                        .iter()
                        .map(|field| {
                            Some(
                                field
                                    .as_str()?
                                    .chars()
                                    .filter(|ch| !ch.is_control())
                                    .take(96)
                                    .collect::<String>(),
                            )
                        })
                        .collect::<Option<Vec<_>>>()
                })
                .collect::<Vec<_>>();
            Some(rows)
        });
        Ok(summarize(&stats, top))
    }
}
fn summarize(s: &Value, top: Option<Vec<Vec<String>>>) -> Value {
    let n = |p: &str| s.pointer(p)?.as_u64();
    let cpu = (|| {
        let delta = n("/cpu_stats/cpu_usage/total_usage")?
            .checked_sub(n("/precpu_stats/cpu_usage/total_usage")?)?;
        let sys =
            n("/cpu_stats/system_cpu_usage")?.checked_sub(n("/precpu_stats/system_cpu_usage")?)?;
        let cores = n("/cpu_stats/online_cpus").or_else(|| {
            Some(
                s.pointer("/cpu_stats/cpu_usage/percpu_usage")?
                    .as_array()?
                    .len() as u64,
            )
        })?;
        (sys > 0 && cores > 0).then(|| delta as f64 / sys as f64 * cores as f64 * 100.0)
    })();
    let memory = n("/memory_stats/usage").map(|v| {
        v.saturating_sub(
            n("/memory_stats/stats/total_inactive_file")
                .or_else(|| n("/memory_stats/stats/inactive_file"))
                .unwrap_or(0),
        )
    });
    let net = |key: &str| {
        s["networks"]
            .as_object()?
            .values()
            .try_fold(0u64, |sum, v| sum.checked_add(v[key].as_u64()?))
    };
    let io = |op: &str| {
        s.pointer("/blkio_stats/io_service_bytes_recursive")?
            .as_array()?
            .iter()
            .filter(|v| v["op"].as_str().is_some_and(|v| v.eq_ignore_ascii_case(op)))
            .try_fold(0u64, |sum, v| sum.checked_add(v["value"].as_u64()?))
    };
    json!({"cpu":cpu,"memory":memory,"limit":n("/memory_stats/limit"),"rx":net("rx_bytes"),"tx":net("tx_bytes"),"read":io("read"),"write":io("write"),"pids":n("/pids_stats/current"),"processes":top})
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn uses_cpu_deltas_and_cgroup_cache_and_preserves_missing_counters() {
        let sample = json!({
           "cpu_stats":{"cpu_usage":{"total_usage":200},"online_cpus":4,"system_cpu_usage":1000},
           "precpu_stats":{"cpu_usage":{"total_usage":100},"system_cpu_usage":600},
           "memory_stats":{"usage":900,"stats":{"inactive_file":300}},
           "networks":{"eth0":{"rx_bytes":100,"tx_bytes":25},"eth1":{"rx_bytes":50,"tx_bytes":5}}
        });
        let view = summarize(&sample, None);
        assert_eq!(view["cpu"], 100.0);
        assert_eq!(view["memory"], 600);
        assert_eq!(view["rx"], 150);
        assert_eq!(view["tx"], 30);
        assert!(view["read"].is_null());
        assert!(view["processes"].is_null());
        let mut reset = sample;
        reset["cpu_stats"]["cpu_usage"]["total_usage"] = json!(1);
        assert!(summarize(&reset, None)["cpu"].is_null());
    }
}
