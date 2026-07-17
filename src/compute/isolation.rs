//! Hard isolation defaults — container never owns the host.

/// Docker run args for maximum practical containment (no host mounts).
pub fn docker_isolation_args(cpus: f64, memory_mb: u64, network: bool) -> Vec<String> {
    let mut args = vec![
        "--read-only".into(),
        "--tmpfs".into(),
        "/tmp:rw,noexec,nosuid,size=64m".into(),
        "--cap-drop".into(),
        "ALL".into(),
        "--security-opt".into(),
        "no-new-privileges".into(),
        "--pids-limit".into(),
        "256".into(),
        "--memory".into(),
        format!("{memory_mb}m"),
        "--cpus".into(),
        format!("{cpus}"),
        "--user".into(),
        "65534:65534".into(), // nobody
    ];
    if network {
        args.push("--network".into());
        args.push("bridge".into());
    } else {
        args.push("--network".into());
        args.push("none".into());
    }
    // Never: --privileged, --pid=host, --net=host, -v /:/host
    args
}
